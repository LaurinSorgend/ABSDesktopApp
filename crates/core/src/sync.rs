//! Catalog and progress sync. The server has no "changed since" endpoint, so a
//! catalog sync pages through every library and removes what is gone.

use std::sync::Mutex;

use abs_api::models::{MediaProgress, User};
use abs_api::{Error as ApiError, Feature, ItemsQuery};

use crate::account::{AccountError, Session, lock};
use crate::db::{BookmarkRow, Db, ItemRow, LibraryRow, ProgressRow};

/// Items requested per page.
pub const PAGE_SIZE: u32 = 500;

/// What one catalog sync did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogReport {
    pub libraries: usize,
    pub items: usize,
    pub removed: usize,
}

/// Fetches all libraries and items of the account and mirrors them into the database.
pub async fn sync_catalog(
    session: &Session,
    db: &Mutex<Db>,
) -> Result<CatalogReport, AccountError> {
    let account_id = &session.account.id;
    let libraries = session.client.libraries().await?;
    let rows: Vec<LibraryRow> = libraries.iter().map(LibraryRow::from_api).collect();
    lock(db).write(|store| store.catalog().replace_libraries(account_id, &rows))?;
    let mut report = CatalogReport {
        libraries: rows.len(),
        ..CatalogReport::default()
    };
    for library in &rows {
        let (items, removed) = sync_library(session, db, &library.remote_id).await?;
        report.items += items;
        report.removed += removed;
    }
    Ok(report)
}

/// Pages through one library. Returns (items seen, items removed).
async fn sync_library(
    session: &Session,
    db: &Mutex<Db>,
    library_id: &str,
) -> Result<(usize, usize), AccountError> {
    let account_id = &session.account.id;
    let mut seen = Vec::new();
    for page in 0.. {
        let query = ItemsQuery {
            limit: PAGE_SIZE,
            page,
            sort: Some("addedAt".into()),
            ..ItemsQuery::default()
        };
        let result = session.client.library_items(library_id, &query).await?;
        let rows: Vec<ItemRow> = result.results.iter().map(ItemRow::from_api).collect();
        lock(db).write(|store| store.catalog().upsert_items(account_id, &rows))?;
        seen.extend(rows.into_iter().map(|r| r.remote_id));
        if result.results.len() < PAGE_SIZE as usize || seen.len() as u64 >= result.total {
            break;
        }
    }
    let keep: Vec<&str> = seen.iter().map(String::as_str).collect();
    let removed =
        lock(db).write(|store| store.catalog().retain_items(account_id, library_id, &keep))?;
    Ok((seen.len(), removed))
}

/// Pulls progress and bookmarks. Returns the ids of items whose progress changed.
///
/// Uses `/api/me/progress` where the server has it, else the `/api/me` payload.
pub async fn sync_progress(session: &Session, db: &Mutex<Db>) -> Result<Vec<String>, AccountError> {
    let me = session.client.me().await?;
    let progress = match session.client.all_progress().await {
        Ok(list) => list,
        Err(ApiError::Unsupported(Feature::MeProgress)) => me.media_progress.clone(),
        Err(e) => return Err(e.into()),
    };
    store_progress(db, &session.account.id, &me, &progress)
}

fn store_progress(
    db: &Mutex<Db>,
    account_id: &str,
    me: &User,
    progress: &[MediaProgress],
) -> Result<Vec<String>, AccountError> {
    let bookmarks: Vec<BookmarkRow> = me.bookmarks.iter().map(BookmarkRow::from_api).collect();
    let changed = lock(db).write(|store| {
        let repo = store.progress();
        let mut changed = Vec::new();
        for entry in progress.iter().filter(|p| p.episode_id.is_none()) {
            let row = ProgressRow::from_api(entry);
            if repo.upsert(account_id, &row)? {
                changed.push(row.item_id);
            }
        }
        repo.replace_bookmarks(account_id, &bookmarks)?;
        Ok(changed)
    })?;
    Ok(changed)
}
