use abs_api::Raw;
use abs_api::models::Library;
use rusqlite::{Connection, OptionalExtension, Row, params};

use super::DbResult;

/// A library as stored.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryRow {
    pub remote_id: String,
    pub name: String,
    pub media_type: String,
    pub display_order: i64,
    pub raw_json: String,
}

impl LibraryRow {
    pub fn from_api(library: &Raw<Library>) -> Self {
        Self {
            remote_id: library.id.clone(),
            name: library.name.clone(),
            media_type: library.media_type.clone(),
            display_order: library.display_order,
            raw_json: library.json().to_owned(),
        }
    }
}

/// A library item as stored, with the columns used for sorting and search.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemRow {
    pub remote_id: String,
    pub library_id: String,
    pub title: String,
    pub title_sort: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub series: Option<String>,
    pub asin: Option<String>,
    pub isbn: Option<String>,
    pub duration: Option<f64>,
    /// Changes when the cover may have changed; `None` when the item has no cover.
    pub cover_etag: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
    pub raw_json: String,
}

/// The columns a grid or list needs.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemSummary {
    pub account_id: String,
    pub remote_id: String,
    pub library_id: String,
    pub title: String,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub series: Option<String>,
    pub duration: Option<f64>,
    pub cover_etag: Option<String>,
}

/// Sort orders for [`Catalog::items_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSort {
    Title,
    Author,
    AddedAt,
    Duration,
}

impl ItemSort {
    fn order_by(self, desc: bool) -> String {
        let column = match self {
            Self::Title => "title_sort COLLATE NOCASE",
            Self::Author => "author COLLATE NOCASE",
            Self::AddedAt => "added_at",
            Self::Duration => "duration",
        };
        let direction = if desc { "DESC" } else { "ASC" };
        format!("{column} {direction}, title_sort COLLATE NOCASE, remote_id")
    }
}

/// Libraries and items of all accounts.
pub struct Catalog<'a> {
    conn: &'a Connection,
}

impl<'a> Catalog<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Replaces the account's libraries. Libraries not in `libraries` are removed with their items.
    pub fn replace_libraries(&self, account_id: &str, libraries: &[LibraryRow]) -> DbResult<()> {
        let mut upsert = self.conn.prepare(
            "INSERT INTO library(account_id, remote_id, name, media_type, display_order, raw_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO UPDATE SET name = excluded.name, media_type = excluded.media_type,
                 display_order = excluded.display_order, raw_json = excluded.raw_json",
        )?;
        for lib in libraries {
            upsert.execute(params![
                account_id,
                lib.remote_id,
                lib.name,
                lib.media_type,
                lib.display_order,
                lib.raw_json
            ])?;
        }
        let keep: Vec<&str> = libraries.iter().map(|l| l.remote_id.as_str()).collect();
        self.fill_keep_ids(&keep)?;
        self.conn.execute(
            "DELETE FROM library WHERE account_id = ?1
             AND remote_id NOT IN (SELECT remote_id FROM temp.keep_ids)",
            [account_id],
        )?;
        Ok(())
    }

    /// The account's libraries in server display order.
    pub fn libraries(&self, account_id: &str) -> DbResult<Vec<LibraryRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT remote_id, name, media_type, display_order, raw_json FROM library
             WHERE account_id = ?1 ORDER BY display_order, name",
        )?;
        let rows = stmt.query_map([account_id], |r| {
            Ok(LibraryRow {
                remote_id: r.get(0)?,
                name: r.get(1)?,
                media_type: r.get(2)?,
                display_order: r.get(3)?,
                raw_json: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Inserts or updates items. Their libraries must exist.
    pub fn upsert_items(&self, account_id: &str, items: &[ItemRow]) -> DbResult<()> {
        let mut stmt = self.conn.prepare(UPSERT_ITEM)?;
        for item in items {
            stmt.execute(params![
                account_id,
                item.remote_id,
                item.library_id,
                item.title,
                item.title_sort,
                item.author,
                item.narrator,
                item.series,
                item.asin,
                item.isbn,
                item.duration,
                item.cover_etag,
                item.added_at,
                item.updated_at,
                item.raw_json
            ])?;
        }
        Ok(())
    }

    /// Deletes the library's items whose ids are not in `keep`. Returns how many were deleted.
    pub fn retain_items(
        &self,
        account_id: &str,
        library_id: &str,
        keep: &[&str],
    ) -> DbResult<usize> {
        self.fill_keep_ids(keep)?;
        let deleted = self.conn.execute(
            "DELETE FROM item WHERE account_id = ?1 AND library_id = ?2
             AND remote_id NOT IN (SELECT remote_id FROM temp.keep_ids)",
            [account_id, library_id],
        )?;
        Ok(deleted)
    }

    /// Deletes one item. Returns true when it existed.
    pub fn remove_item(&self, account_id: &str, item_id: &str) -> DbResult<bool> {
        let deleted = self.conn.execute(
            "DELETE FROM item WHERE account_id = ?1 AND remote_id = ?2",
            [account_id, item_id],
        )?;
        Ok(deleted > 0)
    }

    /// True when the account has this library.
    pub fn has_library(&self, account_id: &str, library_id: &str) -> DbResult<bool> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM library WHERE account_id = ?1 AND remote_id = ?2)",
            [account_id, library_id],
            |r| r.get(0),
        )?)
    }

    pub fn item_count(&self, account_id: &str, library_id: &str) -> DbResult<i64> {
        Ok(self.conn.query_row(
            "SELECT count(*) FROM item WHERE account_id = ?1 AND library_id = ?2",
            [account_id, library_id],
            |r| r.get(0),
        )?)
    }

    /// One page of a library, for the grid.
    pub fn items_page(
        &self,
        account_id: &str,
        library_id: &str,
        sort: ItemSort,
        desc: bool,
        offset: u64,
        limit: u64,
    ) -> DbResult<Vec<ItemSummary>> {
        let sql = format!(
            "{SELECT_SUMMARY} FROM item WHERE account_id = ?1 AND library_id = ?2
             ORDER BY {} LIMIT ?3 OFFSET ?4",
            sort.order_by(desc)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![account_id, library_id, limit as i64, offset as i64],
            summary_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The full server JSON of one item.
    pub fn item_json(&self, account_id: &str, item_id: &str) -> DbResult<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT raw_json FROM item WHERE account_id = ?1 AND remote_id = ?2",
                [account_id, item_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Full-text search over title, author, narrator and series; every word is a prefix match.
    pub fn search(
        &self,
        account_id: Option<&str>,
        text: &str,
        limit: u64,
    ) -> DbResult<Vec<ItemSummary>> {
        let Some(query) = fts_query(text) else {
            return Ok(Vec::new());
        };
        let sql = format!(
            "{SELECT_SUMMARY} FROM item_fts JOIN item ON item.rowid = item_fts.rowid
             WHERE item_fts MATCH ?1 AND (?2 IS NULL OR item.account_id = ?2)
             ORDER BY bm25(item_fts) LIMIT ?3"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![query, account_id, limit as i64], summary_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn fill_keep_ids(&self, ids: &[&str]) -> DbResult<()> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS keep_ids(remote_id TEXT PRIMARY KEY);
             DELETE FROM temp.keep_ids;",
        )?;
        let mut insert = self
            .conn
            .prepare("INSERT OR IGNORE INTO temp.keep_ids(remote_id) VALUES (?1)")?;
        for id in ids {
            insert.execute([id])?;
        }
        Ok(())
    }
}

const UPSERT_ITEM: &str = "INSERT INTO item(account_id, remote_id, library_id, title, title_sort,
        author, narrator, series, asin, isbn, duration, cover_etag, added_at, updated_at, raw_json)
    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
    ON CONFLICT DO UPDATE SET library_id = excluded.library_id, title = excluded.title,
        title_sort = excluded.title_sort, author = excluded.author, narrator = excluded.narrator,
        series = excluded.series, asin = excluded.asin, isbn = excluded.isbn,
        duration = excluded.duration, cover_etag = excluded.cover_etag,
        added_at = excluded.added_at, updated_at = excluded.updated_at, raw_json = excluded.raw_json";

const SELECT_SUMMARY: &str = "SELECT item.account_id, item.remote_id, item.library_id, item.title,
    item.author, item.narrator, item.series, item.duration, item.cover_etag";

fn summary_row(row: &Row<'_>) -> rusqlite::Result<ItemSummary> {
    Ok(ItemSummary {
        account_id: row.get(0)?,
        remote_id: row.get(1)?,
        library_id: row.get(2)?,
        title: row.get(3)?,
        author: row.get(4)?,
        narrator: row.get(5)?,
        series: row.get(6)?,
        duration: row.get(7)?,
        cover_etag: row.get(8)?,
    })
}

/// Turns user input into an FTS5 query of quoted prefix terms. `None` for blank input.
fn fts_query(text: &str) -> Option<String> {
    let terms: Vec<String> = text
        .split_whitespace()
        .map(|word| word.replace('"', ""))
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\"*"))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" "))
}

mod convert;
#[cfg(test)]
mod tests;
