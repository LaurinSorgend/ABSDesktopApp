use std::str::FromStr;

use rusqlite::{Connection, OptionalExtension, params};

use super::DbResult;

/// Key-value app settings.
pub struct Settings<'a> {
    conn: &'a Connection,
}

impl<'a> Settings<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    pub fn get(&self, key: &str) -> DbResult<Option<String>> {
        let value = self
            .conn
            .query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(value)
    }

    /// The parsed value, or `default` when unset or unparsable.
    pub fn get_or<T: FromStr>(&self, key: &str, default: T) -> DbResult<T> {
        Ok(self
            .get(key)?
            .and_then(|v| v.parse().ok())
            .unwrap_or(default))
    }

    pub fn set(&self, key: &str, value: impl ToString) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO setting(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value.to_string()],
        )?;
        Ok(())
    }

    pub fn remove(&self, key: &str) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM setting WHERE key = ?1", [key])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::db::Db;

    #[test]
    fn set_get_and_default() {
        let db = Db::open_in_memory().unwrap();
        let settings = db.store().settings();
        assert_eq!(settings.get_or("cache_mb", 500u32).unwrap(), 500);
        settings.set("cache_mb", 750).unwrap();
        settings.set("cache_mb", 800).unwrap();
        assert_eq!(settings.get_or("cache_mb", 500u32).unwrap(), 800);
        settings.remove("cache_mb").unwrap();
        assert_eq!(settings.get("cache_mb").unwrap(), None);
    }
}
