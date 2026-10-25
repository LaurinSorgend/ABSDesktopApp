//! Keeps the works up to date: one rebuild shortly after any burst of catalog changes.

use std::sync::Arc;
use std::time::Duration;

use super::worker::Shared;
use super::{Command, Event};
use crate::account::lock;
use crate::db::DbResult;

/// Wait after a change before rebuilding, so a sync of several libraries rebuilds once.
const SETTLE: Duration = Duration::from_millis(500);

/// Rebuilds whenever [`request`] was called, until the engine stops.
pub(super) async fn run(shared: Arc<Shared>) {
    loop {
        shared.works_dirty.notified().await;
        tokio::time::sleep(SETTLE).await;
        let result = lock(&shared.db).write(crate::works::rebuild);
        match result {
            Ok(report) => (shared.emit)(Event::WorksChanged(report)),
            Err(e) => (shared.emit)(Event::RequestFailed {
                account_id: String::new(),
                message: e.to_string(),
            }),
        }
    }
}

/// Asks for a rebuild soon.
pub(super) fn request(shared: &Shared) {
    shared.works_dirty.notify_one();
}

pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    let result = store_decision(shared, command);
    match result {
        Ok(()) => request(shared),
        Err(e) => (shared.emit)(Event::RequestFailed {
            account_id: String::new(),
            message: e.to_string(),
        }),
    }
}

fn store_decision(shared: &Shared, command: Command) -> DbResult<()> {
    let db = lock(&shared.db);
    let works = db.store().works();
    match command {
        Command::MergeWorks {
            account_id,
            item_id,
            target_account_id,
            target_item_id,
        } => works.merge(
            (&account_id, &item_id),
            (&target_account_id, &target_item_id),
        ),
        Command::SplitFromWork {
            account_id,
            item_id,
        } => works.split(&account_id, &item_id),
        Command::ResetWorkMatch {
            account_id,
            item_id,
        } => works.clear_override(&account_id, &item_id),
        Command::SetPreferredAccount {
            work_id,
            account_id,
        } => works.set_preferred_account(work_id, account_id.as_deref()),
        _ => Ok(()),
    }
}
