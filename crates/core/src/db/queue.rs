use rusqlite::{Connection, params};

use super::{DbResult, now_ms};

/// A server write that has not been sent yet, such as an offline progress sync.
#[derive(Debug, Clone, PartialEq)]
pub struct PendingSync {
    pub id: i64,
    pub account_id: String,
    pub kind: String,
    /// JSON body to send.
    pub payload: String,
    pub created_at: i64,
}

/// The queue of pending server writes, oldest first.
pub struct Queue<'a> {
    conn: &'a Connection,
}

impl<'a> Queue<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Appends an entry and returns its id.
    pub fn push(&self, account_id: &str, kind: &str, payload: &str) -> DbResult<i64> {
        self.conn.execute(
            "INSERT INTO pending_sync(account_id, kind, payload, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![account_id, kind, payload, now_ms()],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// The oldest entries of one account.
    pub fn peek(&self, account_id: &str, limit: u64) -> DbResult<Vec<PendingSync>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, account_id, kind, payload, created_at FROM pending_sync
             WHERE account_id = ?1 ORDER BY id LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![account_id, limit as i64], |r| {
            Ok(PendingSync {
                id: r.get(0)?,
                account_id: r.get(1)?,
                kind: r.get(2)?,
                payload: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Removes an entry after it was sent.
    pub fn remove(&self, id: i64) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM pending_sync WHERE id = ?1", [id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::{Db, NewAccount};

    #[test]
    fn fifo_per_account_and_cascade() {
        let db = Db::open_in_memory().unwrap();
        let store = db.store();
        let server = store.accounts().add_server("Test", &[]).unwrap();
        let new = NewAccount {
            server_id: &server,
            user_id: "u1",
            username: "listener",
        };
        let account = store.accounts().add_account(&new).unwrap().id;
        let queue = store.queue();
        let first = queue.push(&account, "progress", "{}").unwrap();
        queue.push(&account, "bookmark", "{}").unwrap();
        let kinds: Vec<_> = queue
            .peek(&account, 10)
            .unwrap()
            .into_iter()
            .map(|p| p.kind)
            .collect();
        assert_eq!(kinds, ["progress", "bookmark"]);
        queue.remove(first).unwrap();
        assert_eq!(queue.peek(&account, 10).unwrap().len(), 1);
        store.accounts().remove_account(&account).unwrap();
        assert!(queue.peek(&account, 10).unwrap().is_empty());
    }
}
