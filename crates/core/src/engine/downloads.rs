//! The engine's downloads: a queue in the database, at most two files at once and
//! one per server, files in order within an item.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::task::JoinHandle;

use super::worker::{Shared, lock_map, sessions};
use super::{Command, Event};
use crate::account::{Session, lock};
use crate::db::{DownloadFile, DownloadRow, DownloadState, NewDownload};
use crate::downloads::{self, DownloadError, ROOT_SETTING};

const MAX_RUNNING: usize = 2;
const MAX_ATTEMPTS: u32 = 5;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);
const COVER_WIDTH: u32 = 800;

type Key = (String, String);

struct Running {
    server_id: String,
    task: JoinHandle<()>,
}

/// Download tasks in flight, by account and item.
#[derive(Default)]
pub(super) struct Tasks(HashMap<Key, Running>);

pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    let shared = shared.clone();
    match command {
        Command::Download {
            account_id,
            item_id,
        } => {
            tokio::spawn(queue(shared, account_id, item_id));
        }
        Command::PauseDownload {
            account_id,
            item_id,
        } => {
            tokio::spawn(pause(shared, account_id, item_id));
        }
        Command::ResumeDownload {
            account_id,
            item_id,
        } => {
            set_state(&shared, &account_id, &item_id, DownloadState::Queued, None);
            changed(&shared, account_id, item_id);
            schedule(&shared);
        }
        Command::DeleteDownload {
            account_id,
            item_id,
        } => {
            tokio::spawn(delete(shared, account_id, item_id));
        }
        Command::SetDownloadRoot { path } => {
            let db = lock(&shared.db);
            let settings = db.store().settings();
            let _ = match path {
                Some(path) => settings.set(ROOT_SETTING, path.to_string_lossy()),
                None => settings.remove(ROOT_SETTING),
            };
        }
        _ => {}
    }
}

/// At start: downloads cut off by the last exit go back in the queue, and finished
/// ones with missing or short files are fetched again.
pub(super) fn recover(shared: &Shared) {
    let db = lock(&shared.db);
    let downloads = db.store().downloads();
    let _ = downloads.requeue_running();
    for row in downloads.list().unwrap_or_default() {
        let files = downloads
            .files(&row.account_id, &row.item_id)
            .unwrap_or_default();
        if row.state == DownloadState::Done && !downloads::check(&row.dir, &files) {
            let _ = downloads.set_state(&row.account_id, &row.item_id, DownloadState::Queued, None);
        }
    }
}

/// The folder new downloads go to.
pub(super) fn root(shared: &Shared) -> PathBuf {
    lock(&shared.db)
        .store()
        .settings()
        .get(ROOT_SETTING)
        .ok()
        .flatten()
        .map_or_else(|| shared.download_root.clone(), PathBuf::from)
}

async fn queue(shared: Arc<Shared>, account_id: String, item_id: String) {
    stop(&shared, &account_id, &item_id).await;
    if let Err(e) = add(&shared, &account_id, &item_id).await {
        (shared.emit)(Event::RequestFailed {
            account_id: account_id.clone(),
            message: e.to_string(),
        });
    }
    changed(&shared, account_id, item_id);
    schedule(&shared);
}

async fn add(shared: &Shared, account_id: &str, item_id: &str) -> Result<(), DownloadError> {
    let session = sessions(shared, Some(account_id))
        .into_iter()
        .next()
        .ok_or(DownloadError::Offline)?;
    if !session.login.user.permissions.download {
        return Err(DownloadError::NotAllowed);
    }
    let item = session.client.item(item_id).await?;
    let (files, manifest) = downloads::plan(account_id, item)?;
    let root = root(shared);
    let dir = downloads::item_dir(&root, account_id, item_id);
    downloads::ensure_space(&root, files.iter().map(|f| f.size).sum())?;
    downloads::write_manifest(&dir, &manifest)?;
    let new = NewDownload {
        account_id,
        item_id,
        title: &manifest.title,
        dir: &dir,
        files: &files,
    };
    lock(&shared.db).store().downloads().insert(&new)?;
    Ok(())
}

async fn pause(shared: Arc<Shared>, account_id: String, item_id: String) {
    stop(&shared, &account_id, &item_id).await;
    set_state(&shared, &account_id, &item_id, DownloadState::Paused, None);
    changed(&shared, account_id, item_id);
    schedule(&shared);
}

async fn delete(shared: Arc<Shared>, account_id: String, item_id: String) {
    stop(&shared, &account_id, &item_id).await;
    let row = lock(&shared.db)
        .store()
        .downloads()
        .get(&account_id, &item_id);
    if let Ok(Some(row)) = row {
        let _ = downloads::delete_dir(&row.dir);
    }
    let _ = lock(&shared.db)
        .store()
        .downloads()
        .remove(&account_id, &item_id);
    changed(&shared, account_id, item_id);
    schedule(&shared);
}

/// Deletes every download of an account, before the account goes.
pub(super) async fn forget_account(shared: &Shared, account_id: &str) {
    let rows = lock(&shared.db)
        .store()
        .downloads()
        .list()
        .unwrap_or_default();
    for row in rows.into_iter().filter(|r| r.account_id == account_id) {
        stop(shared, &row.account_id, &row.item_id).await;
        let _ = downloads::delete_dir(&row.dir);
    }
}

/// Cancels a running task and waits until it let go of its files.
async fn stop(shared: &Shared, account_id: &str, item_id: &str) {
    let key = (account_id.to_owned(), item_id.to_owned());
    let running = lock_map(&shared.download_tasks).0.remove(&key);
    if let Some(running) = running {
        running.task.abort();
        let _ = running.task.await;
    }
}

/// Starts queued downloads while there is room.
pub(super) fn schedule(shared: &Arc<Shared>) {
    let rows = lock(&shared.db)
        .store()
        .downloads()
        .list()
        .unwrap_or_default();
    let mut tasks = lock_map(&shared.download_tasks);
    for row in rows
        .into_iter()
        .filter(|r| r.state == DownloadState::Queued)
    {
        if tasks.0.len() >= MAX_RUNNING {
            break;
        }
        let Some(session) = sessions(shared, Some(&row.account_id)).into_iter().next() else {
            continue;
        };
        let server_id = session.account.server_id.clone();
        if tasks.0.values().any(|r| r.server_id == server_id) {
            continue;
        }
        set_state(
            shared,
            &row.account_id,
            &row.item_id,
            DownloadState::Running,
            None,
        );
        changed(shared, row.account_id.clone(), row.item_id.clone());
        let key = (row.account_id.clone(), row.item_id.clone());
        let task = tokio::spawn(run(shared.clone(), session, row));
        tasks.0.insert(key, Running { server_id, task });
    }
}

async fn run(shared: Arc<Shared>, session: Arc<Session>, row: DownloadRow) {
    let result = fetch_all(&shared, &session, &row).await;
    let key = (row.account_id.clone(), row.item_id.clone());
    lock_map(&shared.download_tasks).0.remove(&key);
    let (state, error) = match result {
        Ok(()) => (DownloadState::Done, None),
        Err(e) => (DownloadState::Failed, Some(e.to_string())),
    };
    set_state(
        &shared,
        &row.account_id,
        &row.item_id,
        state,
        error.as_deref(),
    );
    changed(&shared, row.account_id, row.item_id);
    schedule(&shared);
}

async fn fetch_all(
    shared: &Shared,
    session: &Session,
    row: &DownloadRow,
) -> Result<(), DownloadError> {
    let files = lock(&shared.db)
        .store()
        .downloads()
        .files(&row.account_id, &row.item_id)?;
    let mut reporter = Reporter::new(shared, row, &files);
    for (index, file) in files.iter().enumerate() {
        fetch_with_retry(session, row, file, |done| reporter.update(index, done)).await?;
        reporter.flush(index);
    }
    fetch_cover(session, row).await;
    downloads::commit_manifest(&row.dir)?;
    Ok(())
}

/// Tries a file up to five times, waiting 2, 4, 8 and 16 s between tries.
async fn fetch_with_retry(
    session: &Session,
    row: &DownloadRow,
    file: &DownloadFile,
    mut progress: impl FnMut(u64),
) -> Result<(), DownloadError> {
    let mut attempt = 0;
    loop {
        let result =
            downloads::fetch_file(&session.client, &row.item_id, &row.dir, file, &mut progress)
                .await;
        match result {
            Err(e) if e.is_transient() && attempt + 1 < MAX_ATTEMPTS => {
                attempt += 1;
                tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
            }
            result => return result,
        }
    }
}

/// The cover is nice to have; a failure leaves the download without one.
async fn fetch_cover(session: &Session, row: &DownloadRow) {
    if let Ok(Some(bytes)) = session.client.cover(&row.item_id, COVER_WIDTH).await {
        let _ = tokio::fs::write(row.dir.join(downloads::COVER_FILE), bytes).await;
    }
}

/// Stores and announces progress at most twice a second.
struct Reporter<'a> {
    shared: &'a Shared,
    row: &'a DownloadRow,
    files: &'a [DownloadFile],
    done: Vec<u64>,
    last: Instant,
}

impl<'a> Reporter<'a> {
    fn new(shared: &'a Shared, row: &'a DownloadRow, files: &'a [DownloadFile]) -> Self {
        Self {
            shared,
            row,
            files,
            done: files.iter().map(|f| f.bytes_done).collect(),
            last: Instant::now(),
        }
    }

    fn update(&mut self, index: usize, done: u64) {
        self.done[index] = done;
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.flush(index);
        }
    }

    fn flush(&mut self, index: usize) {
        self.last = Instant::now();
        let _ = lock(&self.shared.db).store().downloads().set_bytes_done(
            &self.row.account_id,
            &self.row.item_id,
            &self.files[index].ino,
            self.done[index],
        );
        (self.shared.emit)(Event::DownloadProgress {
            account_id: self.row.account_id.clone(),
            item_id: self.row.item_id.clone(),
            bytes_done: self.done.iter().sum(),
            size: self.files.iter().map(|f| f.size).sum(),
        });
    }
}

fn set_state(
    shared: &Shared,
    account_id: &str,
    item_id: &str,
    state: DownloadState,
    error: Option<&str>,
) {
    let _ = lock(&shared.db)
        .store()
        .downloads()
        .set_state(account_id, item_id, state, error);
}

fn changed(shared: &Shared, account_id: String, item_id: String) {
    (shared.emit)(Event::DownloadsChanged {
        account_id,
        item_id,
    });
}
