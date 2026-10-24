//! The engine's side of offline listening: sends local sessions when the server is
//! there and asks the user about conflicting positions once per item.

use std::sync::Arc;

use super::worker::{Shared, lock_map, sessions};
use super::{Command, Event};
use crate::account::{Session, lock};
use crate::db::Decision;
use crate::offline::{self, CONFLICT_SETTING, DEFAULT_CONFLICT_MINUTES};
use crate::sync::sync_progress;

pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    if let Command::ResolveConflict {
        account_id,
        item_id,
        keep_local,
    } = command
    {
        let decision = if keep_local {
            Decision::KeepLocal
        } else {
            Decision::TakeServer
        };
        let _ = lock(&shared.db)
            .store()
            .local_sessions()
            .decide(&account_id, &item_id, decision);
        lock_map(&shared.conflicts_shown).remove(&(account_id.clone(), item_id));
        spawn_flush(shared, &account_id);
    }
}

/// Flushes the account's local sessions in the background when it is connected.
pub(super) fn spawn_flush(shared: &Arc<Shared>, account_id: &str) {
    if let Some(session) = sessions(shared, Some(account_id)).into_iter().next() {
        tokio::spawn(flush(shared.clone(), session));
    }
}

/// Sends what can go, pulls the resulting progress and announces new conflicts.
pub(super) async fn flush(shared: Arc<Shared>, session: Arc<Session>) {
    let account_id = session.account.id.clone();
    let pending = lock(&shared.db)
        .store()
        .local_sessions()
        .list(&account_id)
        .unwrap_or_default();
    if pending.is_empty() {
        return;
    }
    let running = lock_map(&shared.running_local).clone();
    let result = offline::flush(&session, &shared.db, running.as_deref(), threshold(&shared)).await;
    let report = match result {
        Ok(report) => report,
        Err(e) => {
            (shared.emit)(Event::SyncFailed {
                account_id,
                message: e.to_string(),
            });
            return;
        }
    };
    if !report.sent.is_empty()
        && let Ok(item_ids) = sync_progress(&session, &shared.db).await
    {
        (shared.emit)(Event::ProgressChanged {
            account_id: account_id.clone(),
            item_ids: item_ids.into_iter().chain(report.sent).collect(),
        });
    }
    for conflict in report.conflicts {
        let key = (conflict.account_id.clone(), conflict.item_id.clone());
        if lock_map(&shared.conflicts_shown).insert(key) {
            (shared.emit)(Event::ProgressConflict(conflict));
        }
    }
}

/// Seconds the positions may differ before the user is asked.
fn threshold(shared: &Shared) -> f64 {
    let minutes = lock(&shared.db)
        .store()
        .settings()
        .get_or(CONFLICT_SETTING, DEFAULT_CONFLICT_MINUTES)
        .unwrap_or(DEFAULT_CONFLICT_MINUTES);
    (minutes * 60) as f64
}
