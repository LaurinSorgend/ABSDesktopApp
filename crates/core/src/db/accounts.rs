use rusqlite::{Connection, OptionalExtension, Row, params};
use uuid::Uuid;

use super::{DbResult, now_ms};

/// An account: one user on one server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub id: String,
    pub server_id: String,
    pub user_id: String,
    pub username: String,
    /// Stable per account; sent as `deviceInfo.deviceId`.
    pub device_id: String,
    pub created_at: i64,
}

/// Data for [`Accounts::add_account`].
#[derive(Debug, Clone)]
pub struct NewAccount<'a> {
    pub server_id: &'a str,
    pub user_id: &'a str,
    pub username: &'a str,
}

/// Servers, their URLs and accounts.
pub struct Accounts<'a> {
    conn: &'a Connection,
}

impl<'a> Accounts<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Adds a server with its URLs, first URL first. Returns the new server id.
    pub fn add_server(&self, name: &str, urls: &[&str]) -> DbResult<String> {
        let id = Uuid::new_v4().to_string();
        self.conn
            .execute("INSERT INTO server(id, name) VALUES (?1, ?2)", [&id, name])?;
        for (priority, url) in urls.iter().enumerate() {
            self.add_server_url(&id, url, priority as i64)?;
        }
        Ok(id)
    }

    pub fn add_server_url(&self, server_id: &str, url: &str, priority: i64) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO server_url(server_id, url, priority) VALUES (?1, ?2, ?3)
             ON CONFLICT DO UPDATE SET priority = excluded.priority",
            params![server_id, url, priority],
        )?;
        Ok(())
    }

    /// Replaces the URLs of a server; the first one is tried first.
    pub fn set_server_urls(&self, server_id: &str, urls: &[&str]) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM server_url WHERE server_id = ?1", [server_id])?;
        for (priority, url) in urls.iter().enumerate() {
            self.add_server_url(server_id, url, priority as i64)?;
        }
        Ok(())
    }

    /// URLs of a server, lowest priority value first.
    pub fn server_urls(&self, server_id: &str) -> DbResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT url FROM server_url WHERE server_id = ?1 ORDER BY priority, url")?;
        let urls = stmt.query_map([server_id], |r| r.get(0))?;
        Ok(urls.collect::<Result<_, _>>()?)
    }

    /// The server that has `url` among its URLs.
    pub fn server_by_url(&self, url: &str) -> DbResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT server_id FROM server_url WHERE url = ?1 LIMIT 1",
                [url],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The account of a server user, if it was added before.
    pub fn find(&self, server_id: &str, user_id: &str) -> DbResult<Option<Account>> {
        let sql = format!("{SELECT_ACCOUNT} WHERE server_id = ?1 AND user_id = ?2");
        Ok(self
            .conn
            .query_row(&sql, [server_id, user_id], account_row)
            .optional()?)
    }

    /// Adds an account with a new id and device id.
    pub fn add_account(&self, new: &NewAccount<'_>) -> DbResult<Account> {
        let account = Account {
            id: Uuid::new_v4().to_string(),
            server_id: new.server_id.to_owned(),
            user_id: new.user_id.to_owned(),
            username: new.username.to_owned(),
            device_id: Uuid::new_v4().to_string(),
            created_at: now_ms(),
        };
        self.conn.execute(
            "INSERT INTO account(id, server_id, user_id, username, device_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                account.id,
                account.server_id,
                account.user_id,
                account.username,
                account.device_id,
                account.created_at
            ],
        )?;
        Ok(account)
    }

    pub fn account(&self, id: &str) -> DbResult<Option<Account>> {
        let sql = format!("{SELECT_ACCOUNT} WHERE id = ?1");
        Ok(self.conn.query_row(&sql, [id], account_row).optional()?)
    }

    /// All accounts, oldest first.
    pub fn list(&self) -> DbResult<Vec<Account>> {
        let sql = format!("{SELECT_ACCOUNT} ORDER BY created_at, id");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], account_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Removes an account and everything stored for it.
    pub fn remove_account(&self, id: &str) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM account WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Removes a server, its URLs and its accounts.
    pub fn remove_server(&self, id: &str) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM server WHERE id = ?1", [id])?;
        Ok(())
    }
}

const SELECT_ACCOUNT: &str =
    "SELECT id, server_id, user_id, username, device_id, created_at FROM account";

fn account_row(row: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: row.get(0)?,
        server_id: row.get(1)?,
        user_id: row.get(2)?,
        username: row.get(3)?,
        device_id: row.get(4)?,
        created_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    #[test]
    fn servers_urls_and_accounts() {
        let db = Db::open_in_memory().unwrap();
        let accounts = db.store().accounts();
        let server = accounts
            .add_server("Home", &["https://abs.example", "http://10.0.0.2:13378"])
            .unwrap();
        accounts
            .add_server_url(&server, "http://10.0.0.2:13378", -1)
            .unwrap();
        let urls = accounts.server_urls(&server).unwrap();
        assert_eq!(urls, ["http://10.0.0.2:13378", "https://abs.example"]);
        accounts
            .set_server_urls(&server, &["https://vpn.example", "https://abs.example"])
            .unwrap();
        let urls = accounts.server_urls(&server).unwrap();
        assert_eq!(urls, ["https://vpn.example", "https://abs.example"]);

        let new = NewAccount {
            server_id: &server,
            user_id: "u1",
            username: "laurin",
        };
        let account = accounts.add_account(&new).unwrap();
        assert_ne!(account.device_id, account.id);
        assert!(
            accounts.add_account(&new).is_err(),
            "one account per user and server"
        );
        assert_eq!(
            accounts.account(&account.id).unwrap(),
            Some(account.clone())
        );
        accounts.remove_server(&server).unwrap();
        assert!(accounts.list().unwrap().is_empty());
    }
}
