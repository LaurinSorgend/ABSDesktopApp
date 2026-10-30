use abs_api::models::{Bookmark, MediaProgress};
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::DbResult;

/// Progress of one item as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct ProgressRow {
    pub item_id: String,
    pub current_time: f64,
    pub progress: f64,
    pub is_finished: bool,
    pub ebook_location: Option<String>,
    pub ebook_progress: Option<f64>,
    /// Server `lastUpdate`, in milliseconds.
    pub updated_at: i64,
}

impl ProgressRow {
    pub fn from_api(progress: &MediaProgress) -> Self {
        Self {
            item_id: progress.library_item_id.clone(),
            current_time: progress.current_time.unwrap_or_default(),
            progress: progress.progress.unwrap_or_default(),
            is_finished: progress.is_finished,
            ebook_location: progress.ebook_location.clone(),
            ebook_progress: progress.ebook_progress,
            updated_at: progress.last_update,
        }
    }
}

/// A bookmark as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct BookmarkRow {
    pub item_id: String,
    pub time: f64,
    pub title: String,
}

impl BookmarkRow {
    pub fn from_api(bookmark: &Bookmark) -> Self {
        Self {
            item_id: bookmark.library_item_id.clone(),
            time: bookmark.time,
            title: bookmark.title.clone(),
        }
    }
}

/// Listening progress and bookmarks.
pub struct Progress<'a> {
    conn: &'a Connection,
}

impl<'a> Progress<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Stores progress if it is newer than the stored row. Returns true when it was written.
    pub fn upsert(&self, account_id: &str, row: &ProgressRow) -> DbResult<bool> {
        let changed = self.conn.execute(
            "INSERT INTO progress(account_id, item_id, position, progress, is_finished,
                 ebook_location, ebook_progress, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT DO UPDATE SET position = excluded.position,
                 progress = excluded.progress, is_finished = excluded.is_finished,
                 ebook_location = excluded.ebook_location,
                 ebook_progress = excluded.ebook_progress, updated_at = excluded.updated_at
             WHERE excluded.updated_at > progress.updated_at",
            params![
                account_id,
                row.item_id,
                row.current_time,
                row.progress,
                row.is_finished,
                row.ebook_location,
                row.ebook_progress,
                row.updated_at
            ],
        )?;
        Ok(changed > 0)
    }

    /// Stores an audio position from this machine; the reading position and the finished
    /// flag stay.
    pub fn set_position(
        &self,
        account_id: &str,
        item_id: &str,
        position: f64,
        progress: f64,
        updated_at: i64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO progress(account_id, item_id, position, progress, is_finished,
                 ebook_location, ebook_progress, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, NULL, NULL, ?5)
             ON CONFLICT DO UPDATE SET position = excluded.position,
                 progress = excluded.progress, updated_at = excluded.updated_at",
            params![account_id, item_id, position, progress, updated_at],
        )?;
        Ok(())
    }

    /// Stores a reading position from this machine; the audio position stays.
    pub fn set_ebook(
        &self,
        account_id: &str,
        item_id: &str,
        location: &str,
        progress: f64,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO progress(account_id, item_id, position, progress, is_finished,
                 ebook_location, ebook_progress, updated_at)
             VALUES (?1, ?2, 0, 0, 0, ?3, ?4, ?5)
             ON CONFLICT DO UPDATE SET ebook_location = excluded.ebook_location,
                 ebook_progress = excluded.ebook_progress, updated_at = excluded.updated_at",
            params![account_id, item_id, location, progress, super::now_ms()],
        )?;
        Ok(())
    }

    pub fn get(&self, account_id: &str, item_id: &str) -> DbResult<Option<ProgressRow>> {
        let sql = format!("{SELECT_PROGRESS} WHERE account_id = ?1 AND item_id = ?2");
        Ok(self
            .conn
            .query_row(&sql, [account_id, item_id], progress_row)
            .optional()?)
    }

    /// Started, unfinished items, most recently played first.
    pub fn in_progress(&self, account_id: &str, limit: u64) -> DbResult<Vec<ProgressRow>> {
        let sql = format!(
            "{SELECT_PROGRESS} WHERE account_id = ?1 AND is_finished = 0 AND position > 0
             ORDER BY updated_at DESC LIMIT ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![account_id, limit as i64], progress_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replaces all bookmarks of the account, as after reading the user payload.
    pub fn replace_bookmarks(&self, account_id: &str, bookmarks: &[BookmarkRow]) -> DbResult<()> {
        self.conn
            .execute("DELETE FROM bookmark WHERE account_id = ?1", [account_id])?;
        bookmarks
            .iter()
            .try_for_each(|b| self.put_bookmark(account_id, b))
    }

    /// Adds a bookmark or renames the one at the same time.
    pub fn put_bookmark(&self, account_id: &str, bookmark: &BookmarkRow) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO bookmark(account_id, item_id, time, title) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT DO UPDATE SET title = excluded.title",
            params![account_id, bookmark.item_id, bookmark.time, bookmark.title],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, account_id: &str, item_id: &str, time: f64) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM bookmark WHERE account_id = ?1 AND item_id = ?2 AND time = ?3",
            params![account_id, item_id, time],
        )?;
        Ok(())
    }

    /// Bookmarks of one item by time.
    pub fn bookmarks(&self, account_id: &str, item_id: &str) -> DbResult<Vec<BookmarkRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT item_id, time, title FROM bookmark
             WHERE account_id = ?1 AND item_id = ?2 ORDER BY time",
        )?;
        let rows = stmt.query_map([account_id, item_id], |r| {
            Ok(BookmarkRow {
                item_id: r.get(0)?,
                time: r.get(1)?,
                title: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

const SELECT_PROGRESS: &str = "SELECT item_id, position, progress, is_finished,
    ebook_location, ebook_progress, updated_at FROM progress";

fn progress_row(row: &Row<'_>) -> rusqlite::Result<ProgressRow> {
    Ok(ProgressRow {
        item_id: row.get(0)?,
        current_time: row.get(1)?,
        progress: row.get(2)?,
        is_finished: row.get(3)?,
        ebook_location: row.get(4)?,
        ebook_progress: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, NewAccount};

    fn account(db: &Db) -> String {
        let accounts = db.store().accounts();
        let server = accounts.add_server("Test", &[]).unwrap();
        let new = NewAccount {
            server_id: &server,
            user_id: "u1",
            username: "listener",
        };
        accounts.add_account(&new).unwrap().id
    }

    fn at(time: f64, updated_at: i64) -> ProgressRow {
        ProgressRow {
            item_id: "item".into(),
            current_time: time,
            progress: time / 100.0,
            is_finished: false,
            ebook_location: None,
            ebook_progress: None,
            updated_at,
        }
    }

    #[test]
    fn older_progress_never_overwrites_newer() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db);
        let progress = db.store().progress();
        assert!(progress.upsert(&id, &at(10.0, 2_000)).unwrap());
        assert!(!progress.upsert(&id, &at(5.0, 1_000)).unwrap());
        assert!(progress.upsert(&id, &at(20.0, 3_000)).unwrap());
        assert_eq!(
            progress.get(&id, "item").unwrap().unwrap().current_time,
            20.0
        );
        assert_eq!(progress.in_progress(&id, 10).unwrap().len(), 1);
    }

    #[test]
    fn reading_position_keeps_the_audio_position() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db);
        let progress = db.store().progress();
        progress.set_ebook(&id, "new", "3", 0.1).unwrap();
        let row = progress.get(&id, "new").unwrap().unwrap();
        assert_eq!(
            (row.ebook_location.as_deref(), row.current_time),
            (Some("3"), 0.0)
        );
        progress.upsert(&id, &at(30.0, 1_000)).unwrap();
        progress
            .set_ebook(&id, "item", "epubcfi(/6/2!/4)", 0.5)
            .unwrap();
        let row = progress.get(&id, "item").unwrap().unwrap();
        assert_eq!(row.current_time, 30.0);
        assert_eq!(row.ebook_location.as_deref(), Some("epubcfi(/6/2!/4)"));
        assert_eq!(row.ebook_progress, Some(0.5));
    }

    #[test]
    fn audio_position_keeps_the_reading_position_and_finished_flag() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db);
        let progress = db.store().progress();
        progress.set_position(&id, "new", 12.0, 0.1, 500).unwrap();
        let row = progress.get(&id, "new").unwrap().unwrap();
        assert_eq!((row.current_time, row.is_finished), (12.0, false));
        let read = ProgressRow {
            is_finished: true,
            ebook_location: Some("3".into()),
            ebook_progress: Some(0.5),
            ..at(30.0, 1_000)
        };
        progress.upsert(&id, &read).unwrap();
        progress.set_position(&id, "item", 40.0, 0.4, 900).unwrap();
        let row = progress.get(&id, "item").unwrap().unwrap();
        assert_eq!(row.current_time, 40.0);
        assert_eq!(row.updated_at, 900, "a local write always counts");
        assert!(row.is_finished);
        assert_eq!(row.ebook_location.as_deref(), Some("3"));
        assert_eq!(row.ebook_progress, Some(0.5));
    }

    #[test]
    fn bookmarks_put_rename_replace() {
        let db = Db::open_in_memory().unwrap();
        let id = account(&db);
        let progress = db.store().progress();
        let mark = |time: f64, title: &str| BookmarkRow {
            item_id: "item".into(),
            time,
            title: title.into(),
        };
        progress.put_bookmark(&id, &mark(42.0, "a")).unwrap();
        progress.put_bookmark(&id, &mark(42.0, "b")).unwrap();
        progress.put_bookmark(&id, &mark(7.0, "c")).unwrap();
        let titles: Vec<_> = progress
            .bookmarks(&id, "item")
            .unwrap()
            .into_iter()
            .map(|b| b.title)
            .collect();
        assert_eq!(titles, ["c", "b"]);
        progress.remove_bookmark(&id, "item", 7.0).unwrap();
        progress
            .replace_bookmarks(&id, &[mark(1.0, "only")])
            .unwrap();
        assert_eq!(
            progress.bookmarks(&id, "item").unwrap(),
            [mark(1.0, "only")]
        );
    }
}
