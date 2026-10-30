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
    /// Listening progress from 0 to 1; 0 when never started.
    pub progress: f64,
    pub is_finished: bool,
    /// A finished download exists.
    pub downloaded: bool,
}

/// Which items of a library a page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemQuery<'q> {
    pub library_id: &'q str,
    pub sort: ItemSort,
    pub desc: bool,
    pub filter: ProgressFilter,
}

impl<'q> ItemQuery<'q> {
    /// All items by title.
    pub fn new(library_id: &'q str) -> Self {
        Self {
            library_id,
            sort: ItemSort::Title,
            desc: false,
            filter: ProgressFilter::All,
        }
    }
}

/// Filter by listening state, or to finished downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressFilter {
    All,
    NotStarted,
    InProgress,
    Finished,
    Downloaded,
}

impl ProgressFilter {
    fn condition(self) -> &'static str {
        match self {
            Self::All => "1",
            Self::NotStarted => "coalesce(p.position, 0) = 0 AND coalesce(p.is_finished, 0) = 0",
            Self::InProgress => "p.position > 0 AND p.is_finished = 0",
            Self::Finished => "p.is_finished = 1",
            Self::Downloaded => "dl.item_id IS NOT NULL",
        }
    }
}

/// Sort orders for [`Catalog::items_page`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemSort {
    Title,
    Author,
    AddedAt,
    Duration,
    /// When the download was queued; items without a download have no value.
    DownloadedAt,
}

impl ItemSort {
    pub(super) fn order_by(self, desc: bool) -> String {
        let column = match self {
            Self::Title => "item.title_sort COLLATE NOCASE",
            Self::Author => "item.author COLLATE NOCASE",
            Self::AddedAt => "item.added_at",
            Self::Duration => "item.duration",
            Self::DownloadedAt => "dl.created_at",
        };
        let direction = if desc { "DESC" } else { "ASC" };
        format!("{column} {direction}, item.title_sort COLLATE NOCASE, item.remote_id")
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

    /// Number of items matching the query.
    pub fn item_count(&self, account_id: &str, query: &ItemQuery<'_>) -> DbResult<i64> {
        let sql = format!(
            "SELECT count(*) {FROM_ITEM} WHERE item.account_id = ?1 AND item.library_id = ?2
             AND {}",
            query.filter.condition()
        );
        Ok(self
            .conn
            .query_row(&sql, [account_id, query.library_id], |r| r.get(0))?)
    }

    /// One page of a library, for the grid.
    pub fn items_page(
        &self,
        account_id: &str,
        query: &ItemQuery<'_>,
        offset: u64,
        limit: u64,
    ) -> DbResult<Vec<ItemSummary>> {
        let sql = format!(
            "{SELECT_SUMMARY} {FROM_ITEM} WHERE item.account_id = ?1 AND item.library_id = ?2
             AND {} ORDER BY {} LIMIT ?3 OFFSET ?4",
            query.filter.condition(),
            query.sort.order_by(query.desc)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![account_id, query.library_id, limit as i64, offset as i64],
            summary_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The items with these ids, in the given order; unknown ids are skipped.
    pub fn items_by_ids(&self, account_id: &str, ids: &[&str]) -> DbResult<Vec<ItemSummary>> {
        let sql = format!(
            "{SELECT_SUMMARY} {FROM_ITEM} WHERE item.account_id = ?1 AND item.remote_id = ?2"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let mut items = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(item) = stmt.query_row([account_id, id], summary_row).optional()? {
                items.push(item);
            }
        }
        Ok(items)
    }

    /// Items of a library with this name among their comma-separated authors, by title.
    pub fn items_by_author(
        &self,
        account_id: &str,
        library_id: &str,
        author: &str,
    ) -> DbResult<Vec<ItemSummary>> {
        let sql = format!(
            "{SELECT_SUMMARY} {FROM_ITEM} WHERE item.account_id = ?1 AND item.library_id = ?2
             AND instr(', ' || item.author || ', ', ', ' || ?3 || ', ') > 0
             ORDER BY {}",
            ItemSort::Title.order_by(false)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([account_id, library_id, author], summary_row)?;
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
             {JOIN_PROGRESS} WHERE item_fts MATCH ?1 AND (?2 IS NULL OR item.account_id = ?2)
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
    item.author, item.narrator, item.series, item.duration, item.cover_etag,
    coalesce(p.progress, 0), coalesce(p.is_finished, 0), dl.item_id IS NOT NULL";

const JOIN_PROGRESS: &str =
    "LEFT JOIN progress p ON p.account_id = item.account_id AND p.item_id = item.remote_id
    LEFT JOIN download dl ON dl.account_id = item.account_id AND dl.item_id = item.remote_id
        AND dl.state = 'done'";

const FROM_ITEM: &str = "FROM item LEFT JOIN progress p
    ON p.account_id = item.account_id AND p.item_id = item.remote_id
    LEFT JOIN download dl ON dl.account_id = item.account_id AND dl.item_id = item.remote_id
        AND dl.state = 'done'";

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
        progress: row.get(9)?,
        is_finished: row.get(10)?,
        downloaded: row.get(11)?,
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
