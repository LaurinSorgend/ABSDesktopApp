use abs_api::Raw;
use abs_api::models::{Libraries, LibraryItem, Page};

use super::*;
use crate::db::{Db, NewAccount};

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
    assert_eq!(catalog.item_count(&account, &library).unwrap(), 2);
    let page = catalog
        .items_page(&account, &library, ItemSort::Title, false, 0, 1)
        .unwrap();
    assert_eq!(page[0].title, "The Happy Prince and Other Tales");
    let page = catalog
        .items_page(&account, &library, ItemSort::Duration, true, 0, 10)
        .unwrap();
    assert_eq!(page[0].title, "The Happy Prince Marathon");
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
    assert_eq!(catalog.item_count(&account, &library).unwrap(), 2);
}

#[test]
fn retain_and_library_removal_delete_items() {
    let (db, account, library) = seeded();
    let catalog = db.store().catalog();
    let page = catalog
        .items_page(&account, &library, ItemSort::Title, false, 0, 10)
        .unwrap();
    let deleted = catalog
        .retain_items(&account, &library, &[page[0].remote_id.as_str()])
        .unwrap();
    assert_eq!(deleted, 1);
    assert!(catalog.search(None, "marathon", 10).unwrap().is_empty());

    catalog.replace_libraries(&account, &[]).unwrap();
    assert_eq!(catalog.item_count(&account, &library).unwrap(), 0);
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
