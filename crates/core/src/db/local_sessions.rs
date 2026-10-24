use rusqlite::{Connection, Row, params};

use super::DbResult;

/// What the user chose when local and server positions disagreed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    KeepLocal,
    TakeServer,
}

impl Decision {
    fn as_str(self) -> &'static str {
        match self {
            Self::KeepLocal => "local",
            Self::TakeServer => "server",
        }
    }

    fn parse(text: Option<String>) -> Option<Self> {
        match text.as_deref() {
            Some("local") => Some(Self::KeepLocal),
            Some("server") => Some(Self::TakeServer),
            _ => None,
        }
    }
}

/// Listening done without a server session, waiting to be sent.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalSessionRow {
    /// `play_local_<uuid>`.
    pub id: String,
    pub account_id: String,
    pub item_id: String,
    pub library_id: String,
    pub title: String,
    pub author: String,
    pub duration: f64,
    pub start_position: f64,
    pub position: f64,
    /// Seconds actually listened.
    pub time_listening: f64,
    /// Local clock, milliseconds.
    pub started_at: i64,
    pub updated_at: i64,
    pub decision: Option<Decision>,
}

/// The queue of local sessions.
pub struct LocalSessions<'a> {
    conn: &'a Connection,
}

impl<'a> LocalSessions<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Inserts or updates a session by id.
    pub fn save(&self, row: &LocalSessionRow) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO local_session(id, account_id, item_id, library_id, title, author,
                 duration, start_position, position, time_listening, started_at, updated_at, decision)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET position = excluded.position,
                 time_listening = excluded.time_listening, updated_at = excluded.updated_at",
            params![
                row.id,
                row.account_id,
                row.item_id,
                row.library_id,
                row.title,
                row.author,
                row.duration,
                row.start_position,
                row.position,
                row.time_listening,
                row.started_at,
                row.updated_at,
                row.decision.map(Decision::as_str),
            ],
        )?;
        Ok(())
    }

    /// One account's sessions, oldest first.
    pub fn list(&self, account_id: &str) -> DbResult<Vec<LocalSessionRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, item_id, library_id, title, author, duration, start_position,
                 position, time_listening, started_at, updated_at, decision
             FROM local_session WHERE account_id = ?1 ORDER BY started_at, id",
        )?;
        let rows = stmt.query_map([account_id], local_session_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Records the user's choice for all waiting sessions of an item.
    pub fn decide(&self, account_id: &str, item_id: &str, decision: Decision) -> DbResult<()> {
        self.conn.execute(
            "UPDATE local_session SET decision = ?3 WHERE account_id = ?1 AND item_id = ?2",
            params![account_id, item_id, decision.as_str()],
        )?;
        Ok(())
    }

    pub fn remove(&self, id: &str) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM local_session WHERE id = ?1", [id])?;
        Ok(())
    }
}

fn local_session_row(r: &Row<'_>) -> rusqlite::Result<LocalSessionRow> {
    Ok(LocalSessionRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        item_id: r.get(2)?,
        library_id: r.get(3)?,
        title: r.get(4)?,
        author: r.get(5)?,
        duration: r.get(6)?,
        start_position: r.get(7)?,
        position: r.get(8)?,
        time_listening: r.get(9)?,
        started_at: r.get(10)?,
        updated_at: r.get(11)?,
        decision: Decision::parse(r.get(12)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, NewAccount};

    #[test]
    fn save_update_decide_and_remove() {
        let db = Db::open_in_memory().unwrap();
        let store = db.store();
        let server = store.accounts().add_server("Test", &[]).unwrap();
        let new = NewAccount {
            server_id: &server,
            user_id: "u1",
            username: "listener",
        };
        let account = store.accounts().add_account(&new).unwrap().id;
        let sessions = store.local_sessions();
        let mut row = LocalSessionRow {
            id: "play_local_1".into(),
            account_id: account.clone(),
            item_id: "item".into(),
            library_id: "lib".into(),
            title: "Book".into(),
            author: "Author".into(),
            duration: 100.0,
            start_position: 10.0,
            position: 10.0,
            time_listening: 0.0,
            started_at: 1,
            updated_at: 1,
            decision: None,
        };
        sessions.save(&row).unwrap();
        row.position = 40.0;
        row.time_listening = 30.0;
        row.updated_at = 2;
        sessions.save(&row).unwrap();
        assert_eq!(sessions.list(&account).unwrap(), [row.clone()]);
        sessions
            .decide(&account, "item", Decision::TakeServer)
            .unwrap();
        assert_eq!(
            sessions.list(&account).unwrap()[0].decision,
            Some(Decision::TakeServer)
        );
        sessions.remove(&row.id).unwrap();
        assert!(sessions.list(&account).unwrap().is_empty());
    }
}
