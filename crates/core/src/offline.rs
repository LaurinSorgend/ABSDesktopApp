//! Sessions listened to without a server: sent on reconnect with `local-all`.
//!
//! When another device moved the position meanwhile by more than the threshold,
//! the sessions of that item wait until the user keeps the local position or takes
//! the server's. Listening time is sent either way.

use std::collections::HashMap;
use std::sync::Mutex;

use abs_api::models::{LocalSession, LocalSessions, MediaProgress};

use crate::account::{AccountError, Session, lock};
use crate::db::{Db, Decision, LocalSessionRow, ProgressRow, now_ms};
use crate::playback::device_info;

/// Setting key for the conflict threshold in minutes.
pub const CONFLICT_SETTING: &str = "conflict_minutes";
/// Default conflict threshold in minutes.
pub const DEFAULT_CONFLICT_MINUTES: u64 = 5;

/// Local and server positions of an item disagree; the user decides.
#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub account_id: String,
    pub item_id: String,
    pub title: String,
    pub local_position: f64,
    pub server_position: f64,
}

/// The server's progress of an item when the flush runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ServerProgress {
    pub position: f64,
    /// Server clock, milliseconds.
    pub last_update: i64,
}

/// What a flush did.
#[derive(Debug, Default)]
pub struct FlushReport {
    /// Items whose sessions reached the server.
    pub sent: Vec<String>,
    /// Items waiting for the user.
    pub conflicts: Vec<Conflict>,
}

/// `local_ms` on the server's clock.
pub fn server_time(local_ms: i64, offset_ms: Option<i64>) -> i64 {
    local_ms + offset_ms.unwrap_or(0)
}

/// True when the server position changed after `started_at` (server clock) and is
/// more than `threshold` seconds away from the local one.
pub fn is_conflict(
    local_position: f64,
    server: Option<ServerProgress>,
    started_at: i64,
    threshold: f64,
) -> bool {
    server.is_some_and(|s| {
        s.last_update > started_at && (s.position - local_position).abs() > threshold
    })
}

/// Sends the account's local sessions except `running`. Sessions of conflicting
/// items stay queued and are reported.
pub async fn flush(
    session: &Session,
    db: &Mutex<Db>,
    running: Option<&str>,
    threshold: f64,
) -> Result<FlushReport, AccountError> {
    let rows: Vec<LocalSessionRow> = lock(db)
        .store()
        .local_sessions()
        .list(&session.account.id)?
        .into_iter()
        .filter(|r| Some(r.id.as_str()) != running)
        .collect();
    let mut report = FlushReport::default();
    let mut ready = Vec::new();
    let mut taken = Vec::new();
    for (item_id, item_rows) in by_item(rows) {
        let progress = session.client.item_progress(&item_id).await?;
        let server = progress.as_ref().map(server_progress);
        match prepare(session, &item_rows, server, threshold) {
            Some(sessions) => {
                ready.extend(item_rows.iter().map(|r| r.id.clone()).zip(sessions));
                report.sent.push(item_id);
                let decision = item_rows.last().and_then(|r| r.decision);
                if decision == Some(Decision::TakeServer) {
                    taken.extend(progress.as_ref().map(ProgressRow::from_api));
                }
            }
            None => report.conflicts.push(conflict(&item_rows, server)),
        }
    }
    send(session, db, ready).await?;
    let db = lock(db);
    for row in &taken {
        db.store().progress().replace(&session.account.id, row)?;
    }
    Ok(report)
}

fn by_item(rows: Vec<LocalSessionRow>) -> Vec<(String, Vec<LocalSessionRow>)> {
    let mut items: HashMap<String, Vec<LocalSessionRow>> = HashMap::new();
    for row in rows {
        items.entry(row.item_id.clone()).or_default().push(row);
    }
    items.into_iter().collect()
}

fn server_progress(progress: &MediaProgress) -> ServerProgress {
    ServerProgress {
        position: progress.current_time.unwrap_or_default(),
        last_update: progress.last_update,
    }
}

/// The sessions to send for one item, or `None` while it waits for the user.
fn prepare(
    session: &Session,
    rows: &[LocalSessionRow],
    server: Option<ServerProgress>,
    threshold: f64,
) -> Option<Vec<LocalSession>> {
    let offset = session.client.clock_offset_ms();
    let first = rows.first()?;
    let last = rows.last()?;
    let decision = last.decision;
    let started = server_time(first.started_at, offset);
    if decision.is_none() && is_conflict(last.position, server, started, threshold) {
        return None;
    }
    let now = server_time(now_ms(), offset);
    let updated_at = |row: &LocalSessionRow| {
        updated_at(decision, server, server_time(row.updated_at, offset), now)
    };
    Some(
        rows.iter()
            .map(|row| local_session(row, server_time(row.started_at, offset), updated_at(row)))
            .collect(),
    )
}

/// The `updatedAt` to send, on the server clock. The server keeps whichever of its
/// progress and the session is newer, so a decision moves the session past or
/// before the server's last update.
fn updated_at(
    decision: Option<Decision>,
    server: Option<ServerProgress>,
    session_updated: i64,
    now: i64,
) -> i64 {
    match (decision, server) {
        (Some(Decision::KeepLocal), Some(s)) => now.max(s.last_update + 1),
        (Some(Decision::KeepLocal), None) => now,
        (Some(Decision::TakeServer), Some(s)) => session_updated.min(s.last_update - 1),
        _ => session_updated,
    }
}

fn local_session(row: &LocalSessionRow, started_at: i64, updated_at: i64) -> LocalSession {
    LocalSession {
        id: row.id.clone(),
        library_id: row.library_id.clone(),
        library_item_id: row.item_id.clone(),
        media_type: "book".into(),
        display_title: row.title.clone(),
        display_author: row.author.clone(),
        duration: row.duration,
        play_method: 3,
        media_player: "libmpv".into(),
        start_time: row.start_position,
        current_time: row.position,
        time_listening: row.time_listening,
        started_at: started_at.min(updated_at),
        updated_at,
    }
}

fn conflict(rows: &[LocalSessionRow], server: Option<ServerProgress>) -> Conflict {
    let last = rows.last().expect("an item has sessions");
    Conflict {
        account_id: last.account_id.clone(),
        item_id: last.item_id.clone(),
        title: last.title.clone(),
        local_position: last.position,
        server_position: server.map_or(0.0, |s| s.position),
    }
}

/// Sends the sessions in one request and removes those the server took.
async fn send(
    session: &Session,
    db: &Mutex<Db>,
    ready: Vec<(String, LocalSession)>,
) -> Result<(), AccountError> {
    if ready.is_empty() {
        return Ok(());
    }
    let (ids, sessions): (Vec<String>, Vec<LocalSession>) = ready.into_iter().unzip();
    let request = LocalSessions {
        device_info: device_info(session),
        sessions,
    };
    let results = session.client.sync_local_sessions(&request).await?;
    let db = lock(db);
    let local = db.store().local_sessions();
    for (id, result) in ids.iter().zip(&results) {
        if result.success {
            local.remove(id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_offset_moves_local_times_to_the_server_clock() {
        assert_eq!(server_time(1_000, Some(250)), 1_250);
        assert_eq!(server_time(1_000, Some(-400)), 600);
        assert_eq!(server_time(1_000, None), 1_000);
    }

    #[test]
    fn decisions_put_the_session_after_or_before_the_server() {
        let server = Some(ServerProgress {
            position: 4000.0,
            last_update: 10_000,
        });
        assert_eq!(
            updated_at(Some(Decision::KeepLocal), server, 5_000, 9_500),
            10_001
        );
        assert_eq!(
            updated_at(Some(Decision::KeepLocal), server, 5_000, 12_000),
            12_000
        );
        assert_eq!(
            updated_at(Some(Decision::TakeServer), server, 11_000, 12_000),
            9_999
        );
        assert_eq!(
            updated_at(Some(Decision::TakeServer), server, 5_000, 12_000),
            5_000
        );
        assert_eq!(updated_at(None, server, 5_000, 12_000), 5_000);
    }

    #[test]
    fn conflict_needs_a_newer_and_distant_server_position() {
        let threshold = 300.0;
        let server = |position, last_update| {
            Some(ServerProgress {
                position,
                last_update,
            })
        };
        assert!(is_conflict(1000.0, server(2000.0, 50), 10, threshold));
        assert!(
            !is_conflict(1000.0, server(1200.0, 50), 10, threshold),
            "close enough"
        );
        assert!(
            !is_conflict(1000.0, server(2000.0, 5), 10, threshold),
            "older than the session"
        );
        assert!(
            !is_conflict(1000.0, None, 10, threshold),
            "never started on the server"
        );
    }
}
