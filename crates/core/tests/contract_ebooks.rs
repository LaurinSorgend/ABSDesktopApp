//! Reading through the engine against the Docker servers: open an ebook, save
//! positions, lose the server and get it back. Run with
//! `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use abs_api::AbsClient;
use abs_api::models::EbookLocation;
use abs_core::credentials::Credentials;
use abs_core::db::{Db, ItemQuery};
use abs_core::ebooks::EbookOpen;
use abs_core::engine::{Command, Engine, Event};
use common::{start, wait_for};

const DEAD: &str = "http://127.0.0.1:9/";

#[test]
fn open_save_and_queue_reading_positions() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let (engine, events) = start(&db_path, &Credentials::memory());
    let account_id = add_reader(&engine, &events, &server);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let epub = ebook_item(&db_path, &account_id, "epub");
    let pdf = ebook_item(&db_path, &account_id, "pdf");

    let fetched = open(&engine, &events, &account_id, &epub);
    assert_eq!(fetched.format, "epub");
    assert!(std::fs::read(&fetched.path).unwrap().starts_with(b"PK"));

    let first = "epubcfi(/6/8!/4/2/1:0)";
    save(
        &engine,
        &account_id,
        &epub,
        EbookLocation::Cfi(first.into()),
    );
    assert_eq!(
        runtime.block_on(server_location(&server, &epub, first)),
        first
    );

    let second = "epubcfi(/6/10!/4/2/1:0)";
    set_urls(&engine, &account_id, vec![DEAD.into()]);
    wait_for(&events, |e| {
        matches!(e, Event::Offline { .. }).then_some(())
    });
    save(
        &engine,
        &account_id,
        &epub,
        EbookLocation::Cfi(second.into()),
    );
    save(&engine, &account_id, &pdf, EbookLocation::Page(7));
    let cached = open(&engine, &events, &account_id, &epub);
    assert_eq!(cached.path, fetched.path);
    assert_eq!(cached.location.as_deref(), Some(second));

    set_urls(&engine, &account_id, vec![server.clone()]);
    assert_eq!(
        runtime.block_on(server_location(&server, &epub, second)),
        second
    );
    assert_eq!(runtime.block_on(server_location(&server, &pdf, "7")), "7");
    let db = Db::open(&db_path).unwrap();
    assert!(db.store().queue().peek(&account_id, 10).unwrap().is_empty());
}

fn add_reader(engine: &Engine, events: &Receiver<Event>, server: &str) -> String {
    engine.send(Command::AddAccount {
        url: server.into(),
        username: "reader".into(),
        password: "reader-pass".into(),
    });
    let account_id = wait_for(events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    account_id
}

/// The item of the Ebooks library whose ebook has `format`.
fn ebook_item(db_path: &Path, account_id: &str, format: &str) -> String {
    let db = Db::open(db_path).unwrap();
    let catalog = db.store().catalog();
    let library = catalog
        .libraries(account_id)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Ebooks")
        .unwrap();
    let items = catalog
        .items_page(account_id, &ItemQuery::new(&library.remote_id), 0, 10)
        .unwrap();
    let found = items.iter().find(|item| {
        let json = catalog
            .item_json(account_id, &item.remote_id)
            .unwrap()
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        value["media"]["ebookFormat"] == format
    });
    found.unwrap().remote_id.clone()
}

fn open(engine: &Engine, events: &Receiver<Event>, account_id: &str, item_id: &str) -> EbookOpen {
    engine.send(Command::OpenEbook {
        account_id: account_id.into(),
        item_id: item_id.into(),
    });
    wait_for(events, |e| match e {
        Event::EbookReady(open) => Some((**open).clone()),
        Event::EbookFailed { message, .. } => panic!("{message}"),
        _ => None,
    })
}

fn save(engine: &Engine, account_id: &str, item_id: &str, location: EbookLocation) {
    engine.send(Command::SaveEbookProgress {
        account_id: account_id.into(),
        item_id: item_id.into(),
        location,
        progress: 0.3,
    });
}

fn set_urls(engine: &Engine, account_id: &str, urls: Vec<String>) {
    engine.send(Command::SetServerUrls {
        account_id: account_id.into(),
        urls,
    });
}

/// Polls the server until the item's `ebookLocation` is `expected` or time runs out.
async fn server_location(server: &str, item_id: &str, expected: &str) -> String {
    let client = AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    client.login("reader", "reader-pass").await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let progress = client.item_progress(item_id).await.unwrap();
        let location = progress.and_then(|p| p.ebook_location).unwrap_or_default();
        if location == expected || Instant::now() > deadline {
            return location;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
