//! Browsing several accounts and libraries at once, merged by work or copy by copy.

use rusqlite::{Row, params};

use super::Works;
use crate::db::{DbResult, ItemSort, ItemSummary, ProgressFilter};

/// What the all-accounts page shows.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowseQuery<'q> {
    /// Libraries to include, by account and library id.
    pub libraries: &'q [(String, String)],
    /// One row per work instead of one per item.
    pub merged: bool,
    pub sort: ItemSort,
    pub desc: bool,
    pub filter: ProgressFilter,
}

/// One tile of the all-accounts page.
#[derive(Debug, Clone, PartialEq)]
pub struct BrowseRow {
    /// The item the tile shows. Merged, progress is the furthest of the members shown,
    /// and finished or downloaded is true when any member is.
    pub item: ItemSummary,
    pub work_id: Option<i64>,
    /// Members of the work within the chosen libraries; 1 when not merged.
    pub copies: i64,
}

impl Works<'_> {
    /// The tiles for a query; merged rows show the member of the oldest account.
    pub fn browse(&self, query: &BrowseQuery<'_>) -> DbResult<Vec<BrowseRow>> {
        self.fill_selected(query.libraries)?;
        let partition = if query.merged {
            "coalesce(m.work_id, -item.rowid)"
        } else {
            "-item.rowid"
        };
        let sql = format!(
            "WITH sel AS (
                 SELECT item.account_id, item.remote_id, item.library_id, item.title,
                        item.title_sort, item.author, item.narrator, item.series, item.duration,
                        item.cover_etag, item.added_at, m.work_id,
                        row_number() OVER w AS rn,
                        count(*) OVER w AS copies,
                        max(coalesce(p.progress, 0)) OVER w AS best_progress,
                        max(coalesce(p.is_finished, 0)) OVER w AS any_finished,
                        max(dl.item_id IS NOT NULL) OVER w AS any_download
                 FROM item
                 JOIN temp.selected_libraries s
                     ON s.account_id = item.account_id AND s.library_id = item.library_id
                 JOIN account a ON a.id = item.account_id
                 LEFT JOIN work_member m
                     ON m.account_id = item.account_id AND m.item_id = item.remote_id
                 LEFT JOIN progress p
                     ON p.account_id = item.account_id AND p.item_id = item.remote_id
                 LEFT JOIN download dl ON dl.account_id = item.account_id
                     AND dl.item_id = item.remote_id AND dl.state = 'done'
                 WINDOW w AS (PARTITION BY {partition}
                              ORDER BY a.created_at, a.id, item.remote_id
                              ROWS BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING)
             )
             SELECT account_id, remote_id, library_id, title, author, narrator, series,
                    duration, cover_etag, best_progress, any_finished, any_download,
                    work_id, copies
             FROM sel AS item WHERE rn = 1 AND {} ORDER BY {}",
            condition(query.filter),
            query.sort.order_by(query.desc)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![], browse_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    fn fill_selected(&self, libraries: &[(String, String)]) -> DbResult<()> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS selected_libraries(
                 account_id TEXT NOT NULL, library_id TEXT NOT NULL,
                 PRIMARY KEY (account_id, library_id));
             DELETE FROM temp.selected_libraries;",
        )?;
        let mut insert = self.conn.prepare(
            "INSERT OR IGNORE INTO temp.selected_libraries(account_id, library_id) VALUES (?1, ?2)",
        )?;
        for (account_id, library_id) in libraries {
            insert.execute([account_id, library_id])?;
        }
        Ok(())
    }
}

/// The filter over the per-work values of the `sel` rows.
fn condition(filter: ProgressFilter) -> &'static str {
    match filter {
        ProgressFilter::All => "1",
        ProgressFilter::NotStarted => "best_progress = 0 AND any_finished = 0",
        ProgressFilter::InProgress => "best_progress > 0 AND any_finished = 0",
        ProgressFilter::Finished => "any_finished = 1",
        ProgressFilter::Downloaded => "any_download = 1",
    }
}

fn browse_row(row: &Row<'_>) -> rusqlite::Result<BrowseRow> {
    Ok(BrowseRow {
        item: ItemSummary {
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
        },
        work_id: row.get(12)?,
        copies: row.get(13)?,
    })
}
