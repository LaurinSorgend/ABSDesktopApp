use abs_api::Raw;
use abs_api::models::{Libraries, LibraryItem, Page};

use super::*;
use crate::db::{Db, NewAccount, ProgressRow};

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/../../fixtures/api/2.37.2/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

/// A database with one account and the Audiobooks fixture library and items.
fn seeded() -> (Db, String, String) {
    let mut db = Db::open_in_memory().unwrap();
    let libraries: Libraries = serde_json::from_str(&fixture("libraries")).unwrap();
    let items: Page<Raw<LibraryItem>> = serde_json::from_str(&fixture("library_items")).unwrap();
    let rows: Vec<_> = items.results.iter().map(ItemRow::from_api).collect();
    let library_id = rows[0].library_id.clone();
    let account = db
        .write(|store| {
            let accounts = store.accounts();
            let server = accounts.add_server("Test", &["http://localhost:13379"])?;
            let new = NewAccount {
                server_id: &server,
                user_id: "u1",
                username: "listener",
            };
            let account = accounts.add_account(&new)?;
            let libs: Vec<_> = libraries
                .libraries
                .iter()
                .map(LibraryRow::from_api)
                .collect();
            store.catalog().replace_libraries(&account.id, &libs)?;
            store.catalog().upsert_items(&account.id, &rows)?;
            Ok(account.id)
        })
        .unwrap();
    (db, account, library_id)
}

#[test]
fn converts_minified_items() {
    let items: Page<Raw<LibraryItem>> = serde_json::from_str(&fixture("library_items")).unwrap();
    let row = ItemRow::from_api(&items.results[0]);
    assert_eq!(row.author.as_deref(), Some("Oscar Wilde"));
    assert!(row.title_sort.starts_with("Happy Prince"));
    assert!(row.series.unwrap().starts_with("Fixture series #"));
    assert!(row.cover_etag.is_some());
    assert!(row.raw_json.contains("\"libraryId\""));
}

#[test]
fn converts_expanded_items() {
    let item: Raw<LibraryItem> = serde_json::from_str(&fixture("item_multi_file")).unwrap();
    let row = ItemRow::from_api(&item);
    assert_eq!(row.author.as_deref(), Some("Oscar Wilde"));
    assert_eq!(row.series.as_deref(), Some("Fixture series #2"));
}

#[test]
fn pages_sort_and_count() {
    let (db, account, library) = seeded();
    let catalog = db.store().catalog();
    assert_eq!(
        catalog
            .item_count(&account, &ItemQuery::new(&library))
            .unwrap(),
        2
    );
    let page = catalog
        .items_page(&account, &ItemQuery::new(&library), 0, 1)
        .unwrap();
    assert_eq!(page[0].title, "The Happy Prince and Other Tales");
    let by_duration = ItemQuery {
        sort: ItemSort::Duration,
        desc: true,
        ..ItemQuery::new(&library)
    };
    let page = catalog.items_page(&account, &by_duration, 0, 10).unwrap();
    assert_eq!(page[0].title, "The Happy Prince Marathon");
}

#[test]
fn filters_by_progress_and_keeps_id_order() {
    let (db, account, library) = seeded();
    let store = db.store();
    let catalog = store.catalog();
    let all = catalog
        .items_page(&account, &ItemQuery::new(&library), 0, 10)
        .unwrap();
    let started = ProgressRow {
        item_id: all[1].remote_id.clone(),
        current_time: 60.0,
        progress: 0.25,
        is_finished: false,
        ebook_location: None,
        ebook_progress: None,
        updated_at: 1,
    };
    store.progress().upsert(&account, &started).unwrap();
    let query = |filter| ItemQuery {
        filter,
        ..ItemQuery::new(&library)
    };
    let in_progress = catalog
        .items_page(&account, &query(ProgressFilter::InProgress), 0, 10)
        .unwrap();
    assert_eq!(in_progress.len(), 1);
    assert_eq!(in_progress[0].progress, 0.25);
    let not_started = query(ProgressFilter::NotStarted);
    assert_eq!(catalog.item_count(&account, &not_started).unwrap(), 1);
    let finished = query(ProgressFilter::Finished);
    assert_eq!(catalog.item_count(&account, &finished).unwrap(), 0);
    let downloaded = query(ProgressFilter::Downloaded);
    assert_eq!(catalog.item_count(&account, &downloaded).unwrap(), 0);
    let new = crate::db::NewDownload {
        account_id: &account,
        item_id: &all[0].remote_id,
        title: "t",
        dir: std::path::Path::new("d"),
        files: &[],
    };
    store.downloads().insert(&new).unwrap();
    assert_eq!(
        catalog.item_count(&account, &downloaded).unwrap(),
        0,
        "not done yet"
    );
    let done = crate::db::DownloadState::Done;
    store
        .downloads()
        .set_state(&account, &all[0].remote_id, done, None)
        .unwrap();
    let page = catalog.items_page(&account, &downloaded, 0, 10).unwrap();
    assert_eq!(page.len(), 1);
    assert!(page[0].downloaded);
    store
        .downloads()
        .remove(&account, &all[0].remote_id)
        .unwrap();

    let ids = [
        all[1].remote_id.as_str(),
        "missing",
        all[0].remote_id.as_str(),
    ];
    let by_ids = catalog.items_by_ids(&account, &ids).unwrap();
    assert_eq!(
        by_ids,
        [all[1].clone(), all[0].clone()].map(|mut i| {
            i.progress = if i.remote_id == started.item_id {
                0.25
            } else {
                0.0
            };
            i
        })
    );
}

#[test]
fn search_matches_prefixes_and_survives_updates() {
    let (db, account, library) = seeded();
    let catalog = db.store().catalog();
    let hits = catalog.search(Some(&account), "wil mara", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(catalog.search(None, "\"", 10).unwrap(), []);

    let mut renamed = ItemRow::from_api(
        &serde_json::from_str::<Raw<LibraryItem>>(&fixture("item_single_file")).unwrap(),
    );
    renamed.title = "Completely Different".into();
    catalog.upsert_items(&account, &[renamed]).unwrap();
    assert!(catalog.search(None, "marathon", 10).unwrap().is_empty());
    assert_eq!(catalog.search(None, "complete", 10).unwrap().len(), 1);
    assert_eq!(
        catalog
            .item_count(&account, &ItemQuery::new(&library))
            .unwrap(),
        2
    );
}

#[test]
fn items_by_author_matches_whole_names() {
    let (db, account, library) = seeded();
    let catalog = db.store().catalog();
    assert_eq!(
        catalog
            .items_by_author(&account, &library, "Oscar Wilde")
            .unwrap()
            .len(),
        2
    );
    assert!(
        catalog
            .items_by_author(&account, &library, "Oscar")
            .unwrap()
            .is_empty()
    );
}

#[test]
fn retain_and_library_removal_delete_items() {
    let (db, account, library) = seeded();
    let catalog = db.store().catalog();
    let page = catalog
        .items_page(&account, &ItemQuery::new(&library), 0, 10)
        .unwrap();
    let deleted = catalog
        .retain_items(&account, &library, &[page[0].remote_id.as_str()])
        .unwrap();
    assert_eq!(deleted, 1);
    assert!(catalog.search(None, "marathon", 10).unwrap().is_empty());

    catalog.replace_libraries(&account, &[]).unwrap();
    assert_eq!(
        catalog
            .item_count(&account, &ItemQuery::new(&library))
            .unwrap(),
        0
    );
    assert!(catalog.libraries(&account).unwrap().is_empty());
}

#[test]
fn fts_query_quotes_terms() {
    assert_eq!(
        fts_query("  happy  prince "),
        Some("\"happy\"* \"prince\"*".into())
    );
    assert_eq!(fts_query("a\"b"), Some("\"ab\"*".into()));
    assert_eq!(fts_query("   "), None);
}
