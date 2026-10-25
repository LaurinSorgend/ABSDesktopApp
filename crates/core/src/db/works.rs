use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{DbResult, now_ms};
use crate::works::{Candidate, MatchMethod, Override};

/// An item, by account and remote id.
pub type ItemKey = (String, String);

/// A work as the rebuild wants it stored.
#[derive(Debug, Clone, PartialEq)]
pub struct NewWork {
    /// The id to keep, or `None` for a new work.
    pub id: Option<i64>,
    pub title: String,
    pub author: Option<String>,
    /// The member whose cover and metadata the work shows.
    pub cover: ItemKey,
    pub members: Vec<(ItemKey, MatchMethod)>,
}

/// One item of a work.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkMember {
    pub work_id: i64,
    pub account_id: String,
    pub item_id: String,
    pub method: MatchMethod,
    pub confidence: f64,
}

/// A stored work.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkRow {
    pub id: i64,
    pub title: String,
    pub author: Option<String>,
    pub cover: Option<ItemKey>,
    pub preferred_account_id: Option<String>,
}

/// Works, their members and the user's merge and split decisions.
pub struct Works<'a> {
    conn: &'a Connection,
}

impl<'a> Works<'a> {
    pub(super) fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Book items of enabled accounts, oldest account first, then by title.
    pub fn candidates(&self) -> DbResult<Vec<Candidate>> {
        let mut stmt = self.conn.prepare(
            "SELECT item.account_id, item.remote_id, account.server_id, item.title,
                    item.title_sort, item.author, item.narrator, item.asin, item.isbn,
                    item.duration, json_extract(item.raw_json, '$.media.metadata.abridged'),
                    coalesce(json_extract(item.raw_json, '$.media.ebookFormat'),
                             json_extract(item.raw_json, '$.media.ebookFile.ebookFormat'))
             FROM item
             JOIN account ON account.id = item.account_id
             JOIN library ON library.account_id = item.account_id
                 AND library.remote_id = item.library_id
             WHERE account.enabled = 1 AND library.media_type = 'book'
             ORDER BY account.created_at, account.id, item.title_sort, item.remote_id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(Candidate {
                account_id: r.get(0)?,
                item_id: r.get(1)?,
                server_id: r.get(2)?,
                title: r.get(3)?,
                title_sort: r.get(4)?,
                author: r.get(5)?,
                narrator: r.get(6)?,
                asin: r.get(7)?,
                isbn: r.get(8)?,
                duration: r.get(9)?,
                abridged: r.get(10)?,
                ebook_format: r.get(11)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The user's decisions, oldest first.
    pub fn overrides(&self) -> DbResult<Vec<Override>> {
        let mut stmt = self.conn.prepare(
            "SELECT account_id, item_id, action, target_account_id, target_item_id
             FROM work_override ORDER BY created_at, rowid",
        )?;
        let rows = stmt.query_map([], override_row)?;
        let rows: Vec<Option<Override>> = rows.collect::<Result<_, _>>()?;
        Ok(rows.into_iter().flatten().collect())
    }

    /// The item stands alone from now on. Merges into it are dropped too.
    pub fn split(&self, account_id: &str, item_id: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM work_override WHERE target_account_id = ?1 AND target_item_id = ?2",
            [account_id, item_id],
        )?;
        self.put_override(account_id, item_id, "split", None)
    }

    /// The item's work and the target's work become one.
    pub fn merge(&self, item: (&str, &str), target: (&str, &str)) -> DbResult<()> {
        self.put_override(item.0, item.1, "merge", Some(target))
    }

    /// Forgets the user's decision about an item; the cascade decides again.
    pub fn clear_override(&self, account_id: &str, item_id: &str) -> DbResult<()> {
        self.conn.execute(
            "DELETE FROM work_override WHERE account_id = ?1 AND item_id = ?2",
            [account_id, item_id],
        )?;
        Ok(())
    }

    fn put_override(
        &self,
        account_id: &str,
        item_id: &str,
        action: &str,
        target: Option<(&str, &str)>,
    ) -> DbResult<()> {
        self.conn.execute(
            "INSERT INTO work_override(account_id, item_id, action, target_account_id,
                 target_item_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT DO UPDATE SET action = excluded.action,
                 target_account_id = excluded.target_account_id,
                 target_item_id = excluded.target_item_id, created_at = excluded.created_at",
            params![
                account_id,
                item_id,
                action,
                target.map(|t| t.0),
                target.map(|t| t.1),
                now_ms()
            ],
        )?;
        Ok(())
    }

    /// The work id of every member.
    pub fn member_index(&self) -> DbResult<HashMap<ItemKey, i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT account_id, item_id, work_id FROM work_member")?;
        let rows = stmt.query_map([], |r| Ok(((r.get(0)?, r.get(1)?), r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Replaces all works and members. Works whose id is not kept are deleted.
    pub fn replace(&self, works: &[NewWork]) -> DbResult<()> {
        self.conn.execute("DELETE FROM work_member", [])?;
        self.fill_kept(works)?;
        self.conn.execute(
            "DELETE FROM work WHERE id NOT IN (SELECT id FROM temp.kept_works)",
            [],
        )?;
        let mut member = self.conn.prepare(
            "INSERT INTO work_member(work_id, account_id, item_id, match_method, confidence)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;
        for work in works {
            let id = self.store_work(work)?;
            for ((account_id, item_id), method) in &work.members {
                member.execute(params![
                    id,
                    account_id,
                    item_id,
                    method.as_str(),
                    method.confidence()
                ])?;
            }
        }
        Ok(())
    }

    fn fill_kept(&self, works: &[NewWork]) -> DbResult<()> {
        self.conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS kept_works(id INTEGER PRIMARY KEY);
             DELETE FROM temp.kept_works;",
        )?;
        let mut insert = self
            .conn
            .prepare("INSERT OR IGNORE INTO temp.kept_works(id) VALUES (?1)")?;
        for id in works.iter().filter_map(|w| w.id) {
            insert.execute([id])?;
        }
        Ok(())
    }

    fn store_work(&self, work: &NewWork) -> DbResult<i64> {
        let (account, item) = &work.cover;
        match work.id {
            Some(id) => {
                self.conn.execute(
                    "UPDATE work SET title = ?2, author = ?3, cover_account_id = ?4,
                         cover_item_id = ?5 WHERE id = ?1",
                    params![id, work.title, work.author, account, item],
                )?;
                Ok(id)
            }
            None => {
                self.conn.execute(
                    "INSERT INTO work(title, author, cover_account_id, cover_item_id)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![work.title, work.author, account, item],
                )?;
                Ok(self.conn.last_insert_rowid())
            }
        }
    }

    pub fn work(&self, id: i64) -> DbResult<Option<WorkRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, title, author, cover_account_id, cover_item_id, preferred_account_id
                 FROM work WHERE id = ?1",
                [id],
                work_row,
            )
            .optional()?)
    }

    /// The work an item belongs to.
    pub fn work_of(&self, account_id: &str, item_id: &str) -> DbResult<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT work_id FROM work_member WHERE account_id = ?1 AND item_id = ?2",
                [account_id, item_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// The members of a work, oldest account first.
    pub fn members(&self, work_id: i64) -> DbResult<Vec<WorkMember>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.work_id, m.account_id, m.item_id, m.match_method, m.confidence
             FROM work_member m JOIN account a ON a.id = m.account_id
             WHERE m.work_id = ?1 ORDER BY a.created_at, a.id, m.item_id",
        )?;
        let rows = stmt.query_map([work_id], |r| {
            let method: String = r.get(3)?;
            Ok(WorkMember {
                work_id: r.get(0)?,
                account_id: r.get(1)?,
                item_id: r.get(2)?,
                method: MatchMethod::parse(&method),
                confidence: r.get(4)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The account to play this work from when nothing else decides; `None` clears it.
    pub fn set_preferred_account(&self, work_id: i64, account_id: Option<&str>) -> DbResult<()> {
        self.conn.execute(
            "UPDATE work SET preferred_account_id = ?2 WHERE id = ?1",
            params![work_id, account_id],
        )?;
        Ok(())
    }

    /// Number of works and of works with more than one member.
    pub fn counts(&self) -> DbResult<(i64, i64)> {
        Ok(self.conn.query_row(
            "SELECT count(*), coalesce(sum(n > 1), 0)
             FROM (SELECT count(*) AS n FROM work_member GROUP BY work_id)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}

fn override_row(row: &Row<'_>) -> rusqlite::Result<Option<Override>> {
    let account_id: String = row.get(0)?;
    let item_id: String = row.get(1)?;
    let action: String = row.get(2)?;
    let target_account_id: Option<String> = row.get(3)?;
    let target_item_id: Option<String> = row.get(4)?;
    Ok(match (action.as_str(), target_account_id, target_item_id) {
        ("split", _, _) => Some(Override::Split {
            account_id,
            item_id,
        }),
        ("merge", Some(target_account_id), Some(target_item_id)) => Some(Override::Merge {
            account_id,
            item_id,
            target_account_id,
            target_item_id,
        }),
        _ => None,
    })
}

fn work_row(row: &Row<'_>) -> rusqlite::Result<WorkRow> {
    let cover_account: Option<String> = row.get(3)?;
    let cover_item: Option<String> = row.get(4)?;
    Ok(WorkRow {
        id: row.get(0)?,
        title: row.get(1)?,
        author: row.get(2)?,
        cover: cover_account.zip(cover_item),
        preferred_account_id: row.get(5)?,
    })
}
