//! Data the UI asks for: covers, item details, shelves, search, groups and edits.

use std::sync::Arc;

use abs_api::models::{LibraryItem, ShelfContent};
use abs_api::{ItemFilter, ItemsQuery};

use super::worker::{Shared, sessions};
use super::{COVER_CACHE_SETTING, Command, Event, Group, GroupKind, Shelf};
use crate::account::{AccountError, Session, lock};
use crate::cover_cache::Slot;
use crate::db::{BookmarkRow, ProgressRow};

const SHELF_LIMIT: u32 = 12;
const SEARCH_LIMIT: u32 = 25;
const GROUP_PAGE: u32 = 100;

/// Runs a request command on its own task.
pub(super) fn handle(shared: &Arc<Shared>, command: Command) {
    let shared = shared.clone();
    match command {
        Command::LoadCover {
            account_id,
            item_id,
            etag,
            width,
        } => {
            let slot = Slot {
                account_id,
                item_id,
                width,
            };
            tokio::spawn(load_cover(shared, slot, etag));
        }
        Command::SetCoverCacheSize { megabytes } => set_cover_cache_size(&shared, megabytes),
        command => {
            tokio::spawn(run_online(shared, command));
        }
    }
}

/// Requests that need the server. Errors become `RequestFailed`.
async fn run_online(shared: Arc<Shared>, command: Command) {
    let Some(account_id) = account_of(&command) else {
        return;
    };
    let Some(session) = sessions(&shared, Some(&account_id)).into_iter().next() else {
        (shared.emit)(Event::RequestFailed {
            account_id,
            message: "the server is not connected".into(),
        });
        return;
    };
    if let Err(e) = run(&shared, &session, command).await {
        (shared.emit)(Event::RequestFailed {
            account_id,
            message: e.to_string(),
        });
    }
}

fn account_of(command: &Command) -> Option<String> {
    match command {
        Command::LoadItem { account_id, .. }
        | Command::LoadShelves { account_id, .. }
        | Command::SearchServer { account_id, .. }
        | Command::LoadGroups { account_id, .. }
        | Command::LoadAuthorItems { account_id, .. }
        | Command::SetFinished { account_id, .. }
        | Command::AddBookmark { account_id, .. }
        | Command::RemoveBookmark { account_id, .. } => Some(account_id.clone()),
        _ => None,
    }
}

async fn run(shared: &Shared, session: &Session, command: Command) -> Result<(), AccountError> {
    let event = match command {
        Command::LoadItem { item_id, .. } => load_item(session, item_id).await?,
        Command::LoadShelves { library_id, .. } => shelves(session, library_id).await?,
        Command::SearchServer {
            library_id, text, ..
        } => search(session, &library_id, text).await?,
        Command::LoadGroups {
            library_id, kind, ..
        } => groups(session, library_id, kind).await?,
        Command::LoadAuthorItems {
            library_id,
            author_id,
            ..
        } => author_items(session, &library_id, author_id).await?,
        Command::SetFinished {
            item_id, finished, ..
        } => set_finished(shared, session, item_id, finished).await?,
        Command::AddBookmark {
            item_id,
            time,
            title,
            ..
        } => add_bookmark(shared, session, item_id, time, &title).await?,
        Command::RemoveBookmark { item_id, time, .. } => {
            remove_bookmark(shared, session, item_id, time).await?
        }
        _ => return Ok(()),
    };
    (shared.emit)(event);
    Ok(())
}

/// Disk cache first, so covers show while offline; then the server, a few at a time.
async fn load_cover(shared: Arc<Shared>, slot: Slot, etag: Option<String>) {
    let bytes = match etag {
        Some(etag) => cover_bytes(&shared, &slot, &etag).await,
        None => None,
    };
    (shared.emit)(Event::CoverLoaded {
        account_id: slot.account_id,
        item_id: slot.item_id,
        width: slot.width,
        bytes: bytes.map(Arc::new),
    });
}

async fn cover_bytes(shared: &Arc<Shared>, slot: &Slot, etag: &str) -> Option<Vec<u8>> {
    if let Some(bytes) = shared.covers.get(slot, etag) {
        return Some(bytes);
    }
    let session = sessions(shared, Some(&slot.account_id))
        .into_iter()
        .next()?;
    let _permit = shared.cover_downloads.acquire().await.ok()?;
    let bytes = session
        .client
        .cover(&slot.item_id, slot.width)
        .await
        .ok()??;
    let _ = shared.covers.put(slot, etag, &bytes);
    Some(bytes)
}

fn set_cover_cache_size(shared: &Shared, megabytes: u64) {
    let megabytes = megabytes.max(1);
    let _ = lock(&shared.db)
        .store()
        .settings()
        .set(COVER_CACHE_SETTING, megabytes);
    shared.covers.set_capacity(megabytes * 1024 * 1024);
}

async fn load_item(session: &Session, item_id: String) -> Result<Event, AccountError> {
    let item = session.client.item(&item_id).await?;
    Ok(Event::ItemLoaded {
        account_id: session.account.id.clone(),
        item_id,
        json: item.json().to_owned(),
    })
}

async fn shelves(session: &Session, library_id: String) -> Result<Event, AccountError> {
    let shelves = session
        .client
        .personalized(&library_id, SHELF_LIMIT)
        .await?
        .into_iter()
        .filter_map(|shelf| match shelf.content {
            ShelfContent::Books(books) if !books.is_empty() => Some(Shelf {
                id: shelf.id,
                label: shelf.label,
                item_ids: ids(&books),
            }),
            _ => None,
        })
        .collect();
    Ok(Event::ShelvesLoaded {
        account_id: session.account.id.clone(),
        library_id,
        shelves,
    })
}

async fn search(session: &Session, library_id: &str, text: String) -> Result<Event, AccountError> {
    let results = session
        .client
        .search(library_id, &text, SEARCH_LIMIT)
        .await?;
    let mut item_ids: Vec<String> = results
        .book
        .iter()
        .map(|m| m.library_item.id.clone())
        .collect();
    for series in &results.series {
        item_ids.extend(ids(&series.books));
    }
    let mut seen = std::collections::HashSet::new();
    item_ids.retain(|id| seen.insert(id.clone()));
    Ok(Event::SearchResults {
        account_id: session.account.id.clone(),
        text,
        item_ids,
    })
}

async fn groups(
    session: &Session,
    library_id: String,
    kind: GroupKind,
) -> Result<Event, AccountError> {
    let groups = match kind {
        GroupKind::Series => series(session, &library_id).await?,
        GroupKind::Collections => collections(session, &library_id).await?,
        GroupKind::Authors => authors(session, &library_id).await?,
    };
    Ok(Event::GroupsLoaded {
        account_id: session.account.id.clone(),
        library_id,
        kind,
        groups,
    })
}

async fn series(session: &Session, library_id: &str) -> Result<Vec<Group>, AccountError> {
    let mut groups = Vec::new();
    for page in 0.. {
        let batch = session
            .client
            .library_series(library_id, GROUP_PAGE, page)
            .await?;
        let done = (batch.results.len() as u32) < GROUP_PAGE;
        groups.extend(batch.results.into_iter().map(|s| Group {
            id: s.id,
            name: s.name,
            count: s.books.len() as u64,
            item_ids: ids(&s.books),
        }));
        if done {
            break;
        }
    }
    Ok(groups)
}

async fn collections(session: &Session, library_id: &str) -> Result<Vec<Group>, AccountError> {
    let mut groups = Vec::new();
    for page in 0.. {
        let batch = session
            .client
            .library_collections(library_id, GROUP_PAGE, page)
            .await?;
        let done = (batch.results.len() as u32) < GROUP_PAGE;
        groups.extend(batch.results.into_iter().map(|c| Group {
            id: c.id,
            name: c.name,
            count: c.books.len() as u64,
            item_ids: ids(&c.books),
        }));
        if done {
            break;
        }
    }
    Ok(groups)
}

async fn authors(session: &Session, library_id: &str) -> Result<Vec<Group>, AccountError> {
    let authors = session.client.library_authors(library_id).await?.authors;
    Ok(authors
        .into_iter()
        .map(|a| Group {
            id: a.id,
            name: a.name,
            count: a.num_books.unwrap_or_default(),
            item_ids: Vec::new(),
        })
        .collect())
}

async fn author_items(
    session: &Session,
    library_id: &str,
    author_id: String,
) -> Result<Event, AccountError> {
    let query = ItemsQuery {
        filter: Some(ItemFilter::new("authors", author_id.clone())),
        sort: Some("media.metadata.title".into()),
        ..ItemsQuery::default()
    };
    let page = session.client.library_items(library_id, &query).await?;
    Ok(Event::GroupItems {
        account_id: session.account.id.clone(),
        group_id: author_id,
        item_ids: page.results.iter().map(|i| i.id.clone()).collect(),
    })
}

/// Marks the item on the server, then stores the server's progress row.
async fn set_finished(
    shared: &Shared,
    session: &Session,
    item_id: String,
    finished: bool,
) -> Result<Event, AccountError> {
    session.client.set_finished(&item_id, finished).await?;
    if let Some(progress) = session.client.item_progress(&item_id).await? {
        lock(&shared.db)
            .store()
            .progress()
            .upsert(&session.account.id, &ProgressRow::from_api(&progress))?;
    }
    Ok(Event::ProgressChanged {
        account_id: session.account.id.clone(),
        item_ids: vec![item_id],
    })
}

async fn add_bookmark(
    shared: &Shared,
    session: &Session,
    item_id: String,
    time: f64,
    title: &str,
) -> Result<Event, AccountError> {
    let bookmark = session
        .client
        .create_bookmark(&item_id, time, title)
        .await?;
    lock(&shared.db)
        .store()
        .progress()
        .put_bookmark(&session.account.id, &BookmarkRow::from_api(&bookmark))?;
    Ok(bookmarks_changed(session, item_id))
}

async fn remove_bookmark(
    shared: &Shared,
    session: &Session,
    item_id: String,
    time: f64,
) -> Result<Event, AccountError> {
    session.client.delete_bookmark(&item_id, time).await?;
    lock(&shared.db)
        .store()
        .progress()
        .remove_bookmark(&session.account.id, &item_id, time)?;
    Ok(bookmarks_changed(session, item_id))
}

fn bookmarks_changed(session: &Session, item_id: String) -> Event {
    Event::BookmarksChanged {
        account_id: session.account.id.clone(),
        item_id,
    }
}

fn ids(items: &[LibraryItem]) -> Vec<String> {
    items.iter().map(|i| i.id.clone()).collect()
}
