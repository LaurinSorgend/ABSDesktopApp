//! The engine's side of reading: finds the book file and saves reading positions,
//! queueing them while the server is away.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use abs_api::models::EbookLocation;

use super::worker::{Shared, clock_offset, sessions, went_offline};
use super::{Command, Event};
use crate::account::{AccountError, Session, lock};
use crate::db::now_ms;
use crate::ebooks::{self, EbookError, EbookOpen, PendingPosition, QUEUE_KIND};

/// Queue entries sent per flush.
const FLUSH_BATCH: u64 = 500;

pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    match command {
        Command::OpenEbook {
            account_id,
            item_id,
        } => {
            tokio::spawn(open(shared.clone(), account_id, item_id));
        }
        Command::SaveEbookProgress {
            account_id,
            item_id,
            location,
            progress,
        } => {
            let position = PendingPosition {
                item_id,
                location,
                progress,
            };
            let Some(id) = store(shared, &account_id, &position) else {
                return;
            };
            (shared.emit)(Event::ProgressChanged {
                account_id: account_id.clone(),
                item_ids: vec![position.item_id.clone()],
            });
            tokio::spawn(send_queued(shared.clone(), account_id, position, id));
        }
        _ => {}
    }
}

async fn open(shared: Arc<Shared>, account_id: String, item_id: String) {
    let event = match locate(&shared, &account_id, &item_id).await {
        Ok((path, format, title)) => Event::EbookReady(Box::new(EbookOpen {
            location: stored_location(&shared, &account_id, &item_id),
            account_id,
            item_id,
            title,
            path,
            format,
        })),
        Err(e) => Event::EbookFailed {
            account_id,
            item_id,
            message: e.to_string(),
        },
    };
    (shared.emit)(event);
}

/// The download first, then the server (which refreshes the cache), then the cache.
async fn locate(
    shared: &Arc<Shared>,
    account_id: &str,
    item_id: &str,
) -> Result<(PathBuf, String, String), EbookError> {
    if let Some((path, format)) = ebooks::from_download(&shared.db, account_id, item_id) {
        return Ok((path, format, stored_title(shared, account_id, item_id)));
    }
    if let Some(session) = sessions(shared, Some(account_id)).into_iter().next() {
        match ebooks::fetch(&session, &shared.cache_root, item_id).await {
            Err(EbookError::Account(e)) if e.is_network() => went_offline(shared, account_id, &e),
            result => return result,
        }
    }
    let (path, format) =
        ebooks::from_cache(&shared.cache_root, account_id, item_id).ok_or(EbookError::Offline)?;
    Ok((path, format, stored_title(shared, account_id, item_id)))
}

fn stored_title(shared: &Shared, account_id: &str, item_id: &str) -> String {
    let items = lock(&shared.db)
        .store()
        .catalog()
        .items_by_ids(account_id, &[item_id])
        .unwrap_or_default();
    items
        .into_iter()
        .next()
        .map(|i| i.title)
        .unwrap_or_default()
}

fn stored_location(shared: &Shared, account_id: &str, item_id: &str) -> Option<String> {
    let row = lock(&shared.db).store().progress().get(account_id, item_id);
    row.ok().flatten()?.ebook_location
}

/// Stores the position and queues it at once, so it survives the app quitting
/// mid-request.
fn store(shared: &Shared, account_id: &str, position: &PendingPosition) -> Option<i64> {
    let stored = match &position.location {
        EbookLocation::Cfi(cfi) => cfi.clone(),
        EbookLocation::Page(page) => page.to_string(),
    };
    let payload = serde_json::to_string(position).ok()?;
    let updated_at = crate::offline::server_time(now_ms(), clock_offset(shared, account_id));
    let db = lock(&shared.db);
    let store = db.store();
    let _ = store.progress().set_ebook(
        account_id,
        &position.item_id,
        &stored,
        position.progress,
        updated_at,
    );
    store.queue().push(account_id, QUEUE_KIND, &payload).ok()
}

/// Sends a queued position and removes it, unless the server could not be reached.
async fn send_queued(shared: Arc<Shared>, account_id: String, position: PendingPosition, id: i64) {
    let Some(session) = sessions(&shared, Some(&account_id)).into_iter().next() else {
        return;
    };
    let result = send(&session, &position).await;
    match result {
        Err(e) if e.is_network() => went_offline(&shared, &account_id, &e),
        Err(e) => {
            let _ = lock(&shared.db).store().queue().remove(id);
            (shared.emit)(Event::RequestFailed {
                account_id,
                message: e.to_string(),
            });
        }
        Ok(()) => {
            let _ = lock(&shared.db).store().queue().remove(id);
        }
    }
}

async fn send(session: &Session, position: &PendingPosition) -> Result<(), AccountError> {
    let client = &session.client;
    client
        .set_ebook_progress(&position.item_id, &position.location, position.progress)
        .await?;
    Ok(())
}

/// Sends the newest queued position of each item. Stops at a network error, so the
/// rest waits for the next flush; positions the server refuses are dropped.
pub(super) async fn flush_queue(shared: &Shared, session: &Session) -> Result<(), AccountError> {
    let account_id = &session.account.id;
    let entries = lock(&shared.db)
        .store()
        .queue()
        .peek(account_id, FLUSH_BATCH)?;
    let mut latest: BTreeMap<String, (PendingPosition, Vec<i64>)> = BTreeMap::new();
    for entry in entries.into_iter().filter(|e| e.kind == QUEUE_KIND) {
        let Ok(position) = serde_json::from_str::<PendingPosition>(&entry.payload) else {
            let _ = lock(&shared.db).store().queue().remove(entry.id);
            continue;
        };
        let slot = latest
            .entry(position.item_id.clone())
            .or_insert_with(|| (position.clone(), Vec::new()));
        slot.0 = position;
        slot.1.push(entry.id);
    }
    for (position, ids) in latest.into_values() {
        match send(session, &position).await {
            Err(e) if e.is_network() => return Err(e),
            _ => {
                let db = lock(&shared.db);
                ids.iter().for_each(|id| {
                    let _ = db.store().queue().remove(*id);
                });
            }
        }
    }
    Ok(())
}
