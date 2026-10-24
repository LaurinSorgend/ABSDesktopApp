//! SQLite storage. All SQL of the app lives in this module.

mod accounts;
mod catalog;
mod downloads;
mod progress;
mod queue;
mod settings;

use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

pub use accounts::{Account, Accounts, NewAccount};
pub use catalog::{Catalog, ItemQuery, ItemRow, ItemSort, ItemSummary, LibraryRow, ProgressFilter};
pub use downloads::{DownloadFile, DownloadRow, DownloadState, Downloads, NewDownload};
pub use progress::{BookmarkRow, Progress, ProgressRow};
pub use queue::{PendingSync, Queue};
pub use settings::Settings;

static MIGRATIONS: LazyLock<Migrations<'static>> = LazyLock::new(|| {
    Migrations::new(vec![
        M::up(include_str!("sql/0001_initial.sql")),
        M::up(include_str!("sql/0002_downloads.sql")),
    ])
});

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// Storage errors.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database migration failed: {0}")]
    Migration(#[from] rusqlite_migration::Error),
}

pub type DbResult<T> = Result<T, DbError>;

/// The app database: one SQLite connection with WAL and foreign keys on.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Opens or creates the database file and migrates it to the latest schema.
    pub fn open(path: &Path) -> DbResult<Self> {
        Self::setup(Connection::open(path)?)
    }

    /// A private in-memory database, for tests.
    pub fn open_in_memory() -> DbResult<Self> {
        Self::setup(Connection::open_in_memory()?)
    }

    fn setup(mut conn: Connection) -> DbResult<Self> {
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        MIGRATIONS.to_latest(&mut conn)?;
        Ok(Self { conn })
    }

    /// Repositories for reads and single writes.
    pub fn store(&self) -> Store<'_> {
        Store { conn: &self.conn }
    }

    /// Runs `f` in one transaction. It commits when `f` returns `Ok`.
    pub fn write<T>(&mut self, f: impl FnOnce(&Store<'_>) -> DbResult<T>) -> DbResult<T> {
        let tx = self.conn.transaction()?;
        let value = f(&Store { conn: &tx })?;
        tx.commit()?;
        Ok(value)
    }
}

/// Access to the repositories over a connection or a transaction.
#[derive(Clone, Copy)]
pub struct Store<'a> {
    conn: &'a Connection,
}

impl<'a> Store<'a> {
    pub fn accounts(&self) -> Accounts<'a> {
        Accounts::new(self.conn)
    }

    pub fn catalog(&self) -> Catalog<'a> {
        Catalog::new(self.conn)
    }

    pub fn downloads(&self) -> Downloads<'a> {
        Downloads::new(self.conn)
    }

    pub fn progress(&self) -> Progress<'a> {
        Progress::new(self.conn)
    }

    pub fn queue(&self) -> Queue<'a> {
        Queue::new(self.conn)
    }

    pub fn settings(&self) -> Settings<'a> {
        Settings::new(self.conn)
    }
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        MIGRATIONS.validate().unwrap();
    }

    #[test]
    fn file_database_uses_wal_and_foreign_keys() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("app.db")).unwrap();
        let mode: String = db
            .conn
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .unwrap();
        let fk: bool = db
            .conn
            .pragma_query_value(None, "foreign_keys", |r| r.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert!(fk);
    }
}
