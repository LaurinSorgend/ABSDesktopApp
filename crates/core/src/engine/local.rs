//! The engine's side of local folders: adding and removing them, and scans.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use uuid::Uuid;

use abs_api::models::LibraryItem;

use super::worker::{Shared, is_enabled, lock_map, sync_interval};
use super::{Command, Event, Group, GroupKind, works};
use crate::account::lock;
use crate::db::{DbResult, ItemQuery, ItemSummary, LocalFolder, Store, now_ms};
use crate::local::{ScanReport, folder_name, scan_folder};

/// Progress is sent every so many books.
const PROGRESS_STEP: usize = 10;
/// More books than a folder will have, to read a whole library.
const ALL_ITEMS: u64 = 1_000_000;

pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    match command {
        Command::AddLocalFolder { path } => add_folder(shared, &path),
        Command::RemoveLocalFolder { library_id } => remove_folder(shared, &library_id),
        Command::RescanLocalFolder { library_id } => {
            if let Some(folder) = find(shared, &library_id) {
                start_scan(shared, folder);
            }
        }
        _ => {}
    }
}

/// A folder's series and authors, from the stored items. There are no collections.
pub(super) fn groups(
    store: &Store<'_>,
    account_id: &str,
    library_id: &str,
    kind: GroupKind,
) -> DbResult<Vec<Group>> {
    let catalog = store.catalog();
    let query = ItemQuery::new(library_id);
    let items = catalog.items_page(account_id, &query, 0, ALL_ITEMS)?;
    let mut groups: BTreeMap<String, Vec<(f64, &ItemSummary)>> = BTreeMap::new();
    for item in &items {
        let json = catalog.item_json(account_id, &item.remote_id)?;
        let parsed = json.and_then(|j| serde_json::from_str::<LibraryItem>(&j).ok());
        let Some(parsed) = parsed else {
            continue;
        };
        let metadata = parsed.media.metadata;
        match kind {
            GroupKind::Series => {
                for series in metadata.series {
                    let place = series.sequence.and_then(|s| s.parse().ok());
                    let entry = (place.unwrap_or(f64::MAX), item);
                    groups.entry(series.name).or_default().push(entry);
                }
            }
            GroupKind::Authors => {
                for author in metadata.authors {
                    groups.entry(author.name).or_default().push((0.0, item));
                }
            }
            GroupKind::Collections => {}
        }
    }
    Ok(groups
        .into_iter()
        .map(|(name, mut members)| {
            members.sort_by(|a, b| a.0.total_cmp(&b.0));
            Group {
                id: name.clone(),
                name,
                count: members.len() as u64,
                item_ids: match kind {
                    GroupKind::Authors => Vec::new(),
                    _ => members.iter().map(|m| m.1.remote_id.clone()).collect(),
                },
            }
        })
        .collect())
}

fn add_folder(shared: &Arc<Shared>, path: &Path) {
    let added = {
        let mut db = lock(&shared.db);
        db.write(|store| {
            let accounts = store.accounts();
            let is_new = !accounts.list()?.iter().any(|a| a.local);
            let account = accounts.local_account()?;
            if store.local().folder_by_path(path)?.is_some() {
                return Ok(Err("this folder was already added".to_owned()));
            }
            let library_id = Uuid::new_v4().to_string();
            store
                .local()
                .add_folder(&account.id, &library_id, &folder_name(path), path)?;
            let folder = store.local().folder(&account.id, &library_id)?;
            Ok(Ok((account, is_new, folder)))
        })
    };
    let (account, is_new, folder) = match added {
        Ok(Ok(added)) => added,
        Ok(Err(message)) => return fail(shared, message),
        Err(e) => return fail(shared, e.to_string()),
    };
    if is_new {
        (shared.emit)(Event::AccountAdded(account.clone()));
    }
    (shared.emit)(Event::CatalogChanged {
        account_id: account.id,
    });
    (shared.emit)(Event::LocalFoldersChanged);
    if let Some(folder) = folder {
        start_scan(shared, folder);
    }
}

fn fail(shared: &Shared, message: String) {
    (shared.emit)(Event::RequestFailed {
        account_id: String::new(),
        message,
    });
}

fn remove_folder(shared: &Arc<Shared>, library_id: &str) {
    let Some(folder) = find(shared, library_id) else {
        return;
    };
    let removed = lock(&shared.db)
        .store()
        .local()
        .remove_folder(&folder.account_id, library_id);
    if let Err(e) = removed {
        return fail(shared, e.to_string());
    }
    works::request(shared);
    (shared.emit)(Event::CatalogChanged {
        account_id: folder.account_id,
    });
    (shared.emit)(Event::LocalFoldersChanged);
}

fn find(shared: &Shared, library_id: &str) -> Option<LocalFolder> {
    let folders = lock(&shared.db).store().local().folders().ok()?;
    folders.into_iter().find(|f| f.library_id == library_id)
}

/// Scans every folder of enabled accounts, as at start and on "sync now".
pub(super) fn scan_all(shared: &Arc<Shared>) {
    for folder in folders(shared) {
        start_scan(shared, folder);
    }
}

/// Scans folders whose last scan is longer ago than the sync interval.
pub(super) fn scan_due(shared: &Arc<Shared>) {
    let interval = sync_interval(shared).as_millis() as i64;
    let now = now_ms();
    for folder in folders(shared) {
        if folder.last_scan_at.is_none_or(|at| now - at >= interval) {
            start_scan(shared, folder);
        }
    }
}

fn folders(shared: &Shared) -> Vec<LocalFolder> {
    let folders = lock(&shared.db).store().local().folders();
    let folders = folders.unwrap_or_default();
    folders
        .into_iter()
        .filter(|f| is_enabled(shared, &f.account_id))
        .collect()
}

/// Starts a scan unless the folder is being scanned already. Reading files blocks, so it
/// runs off the engine thread.
fn start_scan(shared: &Arc<Shared>, folder: LocalFolder) {
    if !lock_map(&shared.scanning).insert(folder.library_id.clone()) {
        return;
    }
    (shared.emit)(Event::SyncStarted {
        account_id: folder.account_id.clone(),
    });
    let shared = shared.clone();
    tokio::spawn(async move {
        let task = {
            let (shared, folder) = (shared.clone(), folder.clone());
            tokio::task::spawn_blocking(move || scan(&shared, &folder))
        };
        let report = task.await;
        lock_map(&shared.scanning).remove(&folder.library_id);
        match report {
            Ok(Some(report)) => finished(&shared, folder, report),
            _ => fail(&shared, "the folder could not be scanned".to_owned()),
        }
    });
}

fn scan(shared: &Shared, folder: &LocalFolder) -> Option<ScanReport> {
    let progress = |done: usize, total: usize| {
        if done.is_multiple_of(PROGRESS_STEP) || done == total {
            (shared.emit)(Event::LocalScanProgress {
                library_id: folder.library_id.clone(),
                done,
                total,
            });
        }
    };
    scan_folder(&shared.db, folder, &progress).ok()
}

fn finished(shared: &Arc<Shared>, folder: LocalFolder, report: ScanReport) {
    if report != ScanReport::default() {
        works::request(shared);
    }
    (shared.emit)(Event::CatalogChanged {
        account_id: folder.account_id,
    });
    (shared.emit)(Event::LocalScanFinished {
        library_id: folder.library_id,
        report,
    });
    (shared.emit)(Event::LocalFoldersChanged);
}
