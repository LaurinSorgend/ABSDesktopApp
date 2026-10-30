use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{DbResult, now_ms};

/// A folder on this computer, shown as one library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalFolder {
    pub account_id: String,
    pub library_id: String,
    pub path: PathBuf,
    pub last_scan_at: Option<i64>,
    /// False when the last scan could not reach the folder.
    pub available: bool,
}

/// A book found in a local folder.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalBook {
    pub item_id: String,
    pub dir: PathBuf,
    pub ebook_path: Option<PathBuf>,
    /// An image, or the audio file or EPUB the cover is embedded in.
    pub cover_path: Option<PathBuf>,
    /// File names and sizes of the media files; finds a book that moved.
    pub signature: String,
    /// Paths, sizes and times of all files; unchanged means no need to read them again.
    pub fingerprint: String,
}

/// One audio file of a local book, in play order.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalTrack {
    pub path: PathBuf,
    pub size: u64,
    pub duration: f64,
    pub start_offset: f64,
}

/// Local folders and the files of their books.
pub struct Local<'a> {
    conn: &'a Connection,
}

impl<'a> Local<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Adds a folder and its library.
    pub fn add_folder(
        &self,
        account_id: &str,
        library_id: &str,
        name: &str,
        path: &Path,
    ) -> DbResult<()> {
        let raw_json = serde_json::json!({
            "id": library_id,
            "name": name,
            "mediaType": "book",
        });
        self.conn.execute(
            "INSERT INTO library(account_id, remote_id, name, media_type, display_order, raw_json)
             VALUES (?1, ?2, ?3, 'book', (SELECT count(*) FROM library WHERE account_id = ?1), ?4)",
            params![account_id, library_id, name, raw_json.to_string()],
        )?;
        self.conn.execute(
            "INSERT INTO local_folder(account_id, library_id, path) VALUES (?1, ?2, ?3)",
            params![account_id, library_id, path.to_string_lossy()],
        )?;
        Ok(())
    }

    /// Forgets a folder, its library, items, files and listening state. Files on disk stay.
    pub fn remove_folder(&self, account_id: &str, library_id: &str) -> DbResult<()> {
        for table in ["progress", "bookmark"] {
            self.conn.execute(
                &format!(
                    "DELETE FROM {table} WHERE account_id = ?1 AND item_id IN
                        (SELECT remote_id FROM item WHERE account_id = ?1 AND library_id = ?2)"
                ),
                [account_id, library_id],
            )?;
        }
        self.conn.execute(
            "DELETE FROM library WHERE account_id = ?1 AND remote_id = ?2",
            [account_id, library_id],
        )?;
        Ok(())
    }

    pub fn folders(&self) -> DbResult<Vec<LocalFolder>> {
        let sql = format!("{SELECT_FOLDER} ORDER BY path");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([], folder_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn folder(&self, account_id: &str, library_id: &str) -> DbResult<Option<LocalFolder>> {
        let sql = format!("{SELECT_FOLDER} WHERE account_id = ?1 AND library_id = ?2");
        Ok(self
            .conn
            .query_row(&sql, [account_id, library_id], folder_row)
            .optional()?)
    }

    pub fn folder_by_path(&self, path: &Path) -> DbResult<Option<LocalFolder>> {
        let sql = format!("{SELECT_FOLDER} WHERE path = ?1");
        Ok(self
            .conn
            .query_row(&sql, [path.to_string_lossy()], folder_row)
            .optional()?)
    }

    /// Records the outcome of a scan.
    pub fn set_scanned(&self, account_id: &str, library_id: &str, available: bool) -> DbResult<()> {
        self.conn.execute(
            "UPDATE local_folder SET available = ?3, last_scan_at = ?4
             WHERE account_id = ?1 AND library_id = ?2",
            params![account_id, library_id, available, now_ms()],
        )?;
        Ok(())
    }

    /// Stores a book and its tracks. Its item must exist.
    pub fn save_book(
        &self,
        account_id: &str,
        book: &LocalBook,
        tracks: &[LocalTrack],
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO local_book(account_id, item_id, dir, ebook_path, cover_path,
                 signature, fingerprint)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                account_id,
                book.item_id,
                book.dir.to_string_lossy(),
                book.ebook_path.as_deref().map(Path::to_string_lossy),
                book.cover_path.as_deref().map(Path::to_string_lossy),
                book.signature,
                book.fingerprint
            ],
        )?;
        for (position, track) in tracks.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO local_track(account_id, item_id, position, path, size, duration,
                     start_offset)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    account_id,
                    book.item_id,
                    position as i64,
                    track.path.to_string_lossy(),
                    track.size as i64,
                    track.duration,
                    track.start_offset
                ],
            )?;
        }
        Ok(())
    }

    pub fn book(&self, account_id: &str, item_id: &str) -> DbResult<Option<LocalBook>> {
        let sql = format!("{SELECT_BOOK} WHERE account_id = ?1 AND item_id = ?2");
        Ok(self
            .conn
            .query_row(&sql, [account_id, item_id], book_row)
            .optional()?)
    }

    /// The books of a folder's library.
    pub fn books(&self, account_id: &str, library_id: &str) -> DbResult<Vec<LocalBook>> {
        let sql = format!(
            "{SELECT_BOOK} WHERE account_id = ?1 AND item_id IN
                (SELECT remote_id FROM item WHERE account_id = ?1 AND library_id = ?2)"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([account_id, library_id], book_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn tracks(&self, account_id: &str, item_id: &str) -> DbResult<Vec<LocalTrack>> {
        let mut stmt = self.conn.prepare(
            "SELECT path, size, duration, start_offset FROM local_track
             WHERE account_id = ?1 AND item_id = ?2 ORDER BY position",
        )?;
        let rows = stmt.query_map([account_id, item_id], |r| {
            Ok(LocalTrack {
                path: PathBuf::from(r.get::<_, String>(0)?),
                size: r.get::<_, i64>(1)? as u64,
                duration: r.get(2)?,
                start_offset: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Gives progress and bookmarks of a book that moved to its new id.
    pub fn move_listening_state(&self, account_id: &str, from: &str, to: &str) -> DbResult<()> {
        for table in ["progress", "bookmark"] {
            self.conn.execute(
                &format!("UPDATE {table} SET item_id = ?3 WHERE account_id = ?1 AND item_id = ?2"),
                [account_id, from, to],
            )?;
        }
        Ok(())
    }

    /// Removes the state of books that no longer exist, once nothing took it over.
    pub fn remove_listening_state(&self, account_id: &str, item_id: &str) -> DbResult<()> {
        for table in ["progress", "bookmark"] {
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE account_id = ?1 AND item_id = ?2"),
                [account_id, item_id],
            )?;
        }
        Ok(())
    }
}

const SELECT_FOLDER: &str =
    "SELECT account_id, library_id, path, last_scan_at, available FROM local_folder";

const SELECT_BOOK: &str =
    "SELECT item_id, dir, ebook_path, cover_path, signature, fingerprint FROM local_book";

fn folder_row(row: &Row<'_>) -> rusqlite::Result<LocalFolder> {
    Ok(LocalFolder {
        account_id: row.get(0)?,
        library_id: row.get(1)?,
        path: PathBuf::from(row.get::<_, String>(2)?),
        last_scan_at: row.get(3)?,
        available: row.get(4)?,
    })
}

fn book_row(row: &Row<'_>) -> rusqlite::Result<LocalBook> {
    Ok(LocalBook {
        item_id: row.get(0)?,
        dir: PathBuf::from(row.get::<_, String>(1)?),
        ebook_path: row.get::<_, Option<String>>(2)?.map(PathBuf::from),
        cover_path: row.get::<_, Option<String>>(3)?.map(PathBuf::from),
        signature: row.get(4)?,
        fingerprint: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, ItemRow};

    fn item(id: &str, library_id: &str) -> ItemRow {
        ItemRow {
            remote_id: id.into(),
            library_id: library_id.into(),
            title: id.into(),
            title_sort: id.into(),
            author: None,
            narrator: None,
            series: None,
            asin: None,
            isbn: None,
            duration: None,
            cover_etag: None,
            added_at: 1,
            updated_at: 1,
            raw_json: "{}".into(),
        }
    }

    fn book(id: &str) -> LocalBook {
        LocalBook {
            item_id: id.into(),
            dir: "books/dune".into(),
            ebook_path: Some("books/dune/dune.epub".into()),
            cover_path: None,
            signature: "s".into(),
            fingerprint: "f".into(),
        }
    }

    #[test]
    fn folders_books_and_tracks() {
        let db = Db::open_in_memory().unwrap();
        let store = db.store();
        let account = store.accounts().local_account().unwrap();
        assert!(account.local);
        assert_eq!(store.accounts().local_account().unwrap(), account);

        let local = store.local();
        let path = Path::new("D:/books");
        local.add_folder(&account.id, "lib", "books", path).unwrap();
        assert!(
            local.add_folder(&account.id, "two", "books", path).is_err(),
            "one library per folder"
        );
        let folder = local.folder_by_path(path).unwrap().unwrap();
        assert_eq!(folder.library_id, "lib");
        assert!(folder.available);
        local.set_scanned(&account.id, "lib", false).unwrap();
        assert!(!local.folders().unwrap()[0].available);

        store
            .catalog()
            .upsert_items(&account.id, &[item("a", "lib")])
            .unwrap();
        let track = |n: u64| LocalTrack {
            path: format!("{n}.mp3").into(),
            size: n,
            duration: 10.0,
            start_offset: 10.0 * n as f64,
        };
        local
            .save_book(&account.id, &book("a"), &[track(0), track(1)])
            .unwrap();
        assert_eq!(local.book(&account.id, "a").unwrap(), Some(book("a")));
        assert_eq!(local.books(&account.id, "lib").unwrap(), [book("a")]);
        assert_eq!(local.tracks(&account.id, "a").unwrap()[1], track(1));

        local.remove_folder(&account.id, "lib").unwrap();
        assert!(local.folders().unwrap().is_empty());
        assert_eq!(local.book(&account.id, "a").unwrap(), None);
        assert!(local.tracks(&account.id, "a").unwrap().is_empty());
    }

    #[test]
    fn listening_state_moves_with_a_book() {
        let db = Db::open_in_memory().unwrap();
        let store = db.store();
        let account = store.accounts().local_account().unwrap().id;
        store
            .progress()
            .set_position(&account, "old", 5.0, 0.5, 1)
            .unwrap();
        store
            .local()
            .move_listening_state(&account, "old", "new")
            .unwrap();
        assert!(store.progress().get(&account, "old").unwrap().is_none());
        assert_eq!(
            store
                .progress()
                .get(&account, "new")
                .unwrap()
                .unwrap()
                .current_time,
            5.0
        );
        store
            .local()
            .remove_listening_state(&account, "new")
            .unwrap();
        assert!(store.progress().get(&account, "new").unwrap().is_none());
    }
}
