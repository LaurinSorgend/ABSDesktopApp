use std::path::PathBuf;

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{DbResult, now_ms};

/// Where a download stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
}

impl DownloadState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn parse(text: &str) -> Self {
        match text {
            "running" => Self::Running,
            "paused" => Self::Paused,
            "done" => Self::Done,
            "failed" => Self::Failed,
            _ => Self::Queued,
        }
    }
}

/// A downloaded or downloading item, with the sums over its files.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadRow {
    pub account_id: String,
    pub item_id: String,
    pub title: String,
    /// The item's folder.
    pub dir: PathBuf,
    pub state: DownloadState,
    pub error: Option<String>,
    pub size: u64,
    pub bytes_done: u64,
    pub created_at: i64,
}

/// One file of a download, in playback order.
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadFile {
    pub ino: String,
    /// Name inside the item folder, `<ino>.<ext>`.
    pub file_name: String,
    pub size: u64,
    pub bytes_done: u64,
}

/// A download to add.
#[derive(Debug, Clone)]
pub struct NewDownload<'a> {
    pub account_id: &'a str,
    pub item_id: &'a str,
    pub title: &'a str,
    pub dir: &'a std::path::Path,
    pub files: &'a [DownloadFile],
}

const SELECT: &str = "SELECT d.account_id, d.item_id, d.title, d.dir, d.state, d.error,
        COALESCE(SUM(f.size), 0), COALESCE(SUM(f.bytes_done), 0), d.created_at
    FROM download d
    LEFT JOIN download_file f ON f.account_id = d.account_id AND f.item_id = d.item_id";

/// Downloads and their files.
pub struct Downloads<'a> {
    conn: &'a Connection,
}

impl<'a> Downloads<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Adds a queued download, replacing an earlier one of the same item.
    pub fn insert(&self, new: &NewDownload<'_>) -> DbResult<()> {
        self.remove(new.account_id, new.item_id)?;
        self.conn.execute(
            "INSERT INTO download(account_id, item_id, title, dir, state, created_at)
             VALUES (?1, ?2, ?3, ?4, 'queued', ?5)",
            params![
                new.account_id,
                new.item_id,
                new.title,
                new.dir.to_string_lossy(),
                now_ms()
            ],
        )?;
        for (position, file) in new.files.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO download_file(account_id, item_id, ino, position, file_name, size, bytes_done)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    new.account_id,
                    new.item_id,
                    file.ino,
                    position as i64,
                    file.file_name,
                    file.size as i64,
                    file.bytes_done as i64
                ],
            )?;
        }
        Ok(())
    }

    pub fn get(&self, account_id: &str, item_id: &str) -> DbResult<Option<DownloadRow>> {
        let sql = format!("{SELECT} WHERE d.account_id = ?1 AND d.item_id = ?2 GROUP BY d.item_id");
        Ok(self
            .conn
            .query_row(&sql, params![account_id, item_id], download_row)
            .optional()?)
    }

    /// All downloads, oldest first.
    pub fn list(&self) -> DbResult<Vec<DownloadRow>> {
        let sql = format!("{SELECT} GROUP BY d.account_id, d.item_id ORDER BY d.created_at");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], download_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Item ids of one account's finished downloads.
    pub fn done_ids(&self, account_id: &str) -> DbResult<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_id FROM download WHERE account_id = ?1 AND state = 'done'")?;
        let rows = stmt.query_map([account_id], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn files(&self, account_id: &str, item_id: &str) -> DbResult<Vec<DownloadFile>> {
        let mut stmt = self.conn.prepare(
            "SELECT ino, file_name, size, bytes_done FROM download_file
             WHERE account_id = ?1 AND item_id = ?2 ORDER BY position",
        )?;
        let rows = stmt.query_map(params![account_id, item_id], |r| {
            Ok(DownloadFile {
                ino: r.get(0)?,
                file_name: r.get(1)?,
                size: r.get::<_, i64>(2)? as u64,
                bytes_done: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_state(
        &self,
        account_id: &str,
        item_id: &str,
        state: DownloadState,
        error: Option<&str>,
    ) -> DbResult<()> {
        self.conn.execute(
            "UPDATE download SET state = ?3, error = ?4 WHERE account_id = ?1 AND item_id = ?2",
            params![account_id, item_id, state.as_str(), error],
        )?;
        Ok(())
    }

    pub fn set_bytes_done(
        &self,
        account_id: &str,
        item_id: &str,
        ino: &str,
        bytes_done: u64,
    ) -> DbResult<()> {
        self.conn.execute(
            "UPDATE download_file SET bytes_done = ?4
             WHERE account_id = ?1 AND item_id = ?2 AND ino = ?3",
            params![account_id, item_id, ino, bytes_done as i64],
        )?;
        Ok(())
    }

    /// Puts downloads that were running when the app quit back in the queue.
    pub fn requeue_running(&self) -> DbResult<()> {
        self.conn.execute(
            "UPDATE download SET state = 'queued' WHERE state = 'running'",
            [],
        )?;
        Ok(())
    }

    pub fn remove(&self, account_id: &str, item_id: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM download WHERE account_id = ?1 AND item_id = ?2",
            params![account_id, item_id],
        )?;
        Ok(())
    }
}

fn download_row(r: &Row<'_>) -> rusqlite::Result<DownloadRow> {
    Ok(DownloadRow {
        account_id: r.get(0)?,
        item_id: r.get(1)?,
        title: r.get(2)?,
        dir: PathBuf::from(r.get::<_, String>(3)?),
        state: DownloadState::parse(&r.get::<_, String>(4)?),
        error: r.get(5)?,
        size: r.get::<_, i64>(6)? as u64,
        bytes_done: r.get::<_, i64>(7)? as u64,
        created_at: r.get(8)?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::db::{Db, NewAccount};

    fn file(ino: &str, size: u64) -> DownloadFile {
        DownloadFile {
            ino: ino.into(),
            file_name: format!("{ino}.mp3"),
            size,
            bytes_done: 0,
        }
    }

    #[test]
    fn insert_progress_states_and_cascade() {
        let db = Db::open_in_memory().unwrap();
        let store = db.store();
        let server = store.accounts().add_server("Test", &[]).unwrap();
        let new = NewAccount {
            server_id: &server,
            user_id: "u1",
            username: "listener",
        };
        let account = store.accounts().add_account(&new).unwrap().id;
        let downloads = store.downloads();
        let files = [file("2", 100), file("1", 50)];
        let new = NewDownload {
            account_id: &account,
            item_id: "item",
            title: "Book",
            dir: Path::new("/d/item"),
            files: &files,
        };
        downloads.insert(&new).unwrap();
        downloads.set_bytes_done(&account, "item", "1", 20).unwrap();
        downloads
            .set_state(&account, "item", DownloadState::Running, None)
            .unwrap();
        let row = downloads.get(&account, "item").unwrap().unwrap();
        assert_eq!(
            (row.size, row.bytes_done, row.state),
            (150, 20, DownloadState::Running)
        );
        let order: Vec<_> = downloads
            .files(&account, "item")
            .unwrap()
            .into_iter()
            .map(|f| f.ino)
            .collect();
        assert_eq!(order, ["2", "1"]);
        downloads.requeue_running().unwrap();
        assert_eq!(downloads.list().unwrap()[0].state, DownloadState::Queued);
        downloads.insert(&new).unwrap();
        assert_eq!(
            downloads.get(&account, "item").unwrap().unwrap().bytes_done,
            0
        );
        downloads
            .set_state(&account, "item", DownloadState::Done, None)
            .unwrap();
        assert_eq!(downloads.done_ids(&account).unwrap(), ["item"]);
        store.accounts().remove_account(&account).unwrap();
        assert!(downloads.list().unwrap().is_empty());
    }
}
