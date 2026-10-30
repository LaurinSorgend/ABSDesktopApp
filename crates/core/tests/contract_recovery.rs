//! What happens when things go wrong around a server: damaged downloads, a reverse proxy
//! answering 502, reports that arrive late, and a reconnect during a session.
//! Run with `cargo test -p abs-core --features integration`.
//!
//! `old_session_survives_a_reconnect` needs `ABS_TEST_SHORT_GRACE_SERVER`, a 2.37 server
//! started with `REFRESH_TOKEN_GRACE_PERIOD=2` and an `admin` / `admin-pass` user.
#![cfg(feature = "integration")]

mod common;

use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use abs_api::http_client;
use abs_api::models::EbookLocation;
use abs_core::account::{add_account, reconnect_account};
use abs_core::credentials::Credentials;
use abs_core::db::{Db, DownloadState, ItemQuery};
use abs_core::engine::{Command, Engine, Event};
use common::{FlakyProxy, start, wait_for};

const SERVER: &str = "http://localhost:13379";
/// Time for the sync after adding an account to end. Cutting it would take the account
/// offline, and the position would then never be sent to the broken server.
const SYNC_SETTLES: Duration = Duration::from_secs(3);

fn add(engine: &Engine, events: &Receiver<Event>, url: &str, username: &str) -> String {
    engine.send(Command::AddAccount {
        url: url.into(),
        username: username.into(),
        password: format!("{username}-pass"),
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

/// Items of a library, in the catalog's order.
fn items(db_path: &Path, account_id: &str, library: &str) -> Vec<(String, String)> {
    let db = Db::open(db_path).unwrap();
    let catalog = db.store().catalog();
    let library = catalog
        .libraries(account_id)
        .unwrap()
        .into_iter()
        .find(|l| l.name == library)
        .unwrap();
    catalog
        .items_page(account_id, &ItemQuery::new(&library.remote_id), 0, 10)
        .unwrap()
        .into_iter()
        .map(|i| (i.remote_id, i.title))
        .collect()
}

fn happy_prince(db_path: &Path, account_id: &str) -> String {
    let books = items(db_path, account_id, "Audiobooks");
    let book = books
        .into_iter()
        .find(|(_, title)| title.starts_with("The Happy Prince and"));
    book.unwrap().0
}

fn download_state(db_path: &Path, account_id: &str, item_id: &str) -> Option<DownloadState> {
    let db = Db::open(db_path).unwrap();
    let row = db.store().downloads().get(account_id, item_id);
    row.unwrap().map(|r| r.state)
}

/// Waits until the download is done or failed and returns which.
fn finished_download(
    events: &Receiver<Event>,
    db_path: &Path,
    account_id: &str,
    item_id: &str,
) -> DownloadState {
    let deadline = Instant::now() + Duration::from_secs(180);
    loop {
        let state = download_state(db_path, account_id, item_id);
        if matches!(state, Some(DownloadState::Done | DownloadState::Failed)) {
            return state.unwrap();
        }
        assert!(Instant::now() < deadline, "download still {state:?}");
        let _ = events.recv_timeout(Duration::from_millis(500));
    }
}

fn download(engine: &Engine, events: &Receiver<Event>, db: &Path, account: &str, item: &str) {
    engine.send(Command::Download {
        account_id: account.into(),
        item_id: item.into(),
    });
    let state = finished_download(events, db, account, item);
    assert_eq!(state, DownloadState::Done);
}

#[test]
fn damaged_download_is_repaired_at_start() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let credentials = Credentials::memory();
    let (engine, events) = start(&db_path, &credentials);
    let account_id = add(&engine, &events, SERVER, "traveller");
    let item_id = happy_prince(&db_path, &account_id);
    download(&engine, &events, &db_path, &account_id, &item_id);
    engine.shutdown();

    let (folder, files) = {
        let db = Db::open(&db_path).unwrap();
        let store = db.store();
        let row = store.downloads().get(&account_id, &item_id).unwrap();
        let files = store.downloads().files(&account_id, &item_id).unwrap();
        (row.unwrap().dir, files)
    };
    let name = |index: usize| folder.join(&files[index].file_name);
    let complete_part = folder.join(format!("{}.part", files[0].file_name));
    std::fs::rename(name(0), &complete_part).unwrap();
    std::fs::remove_file(name(1)).unwrap();

    let (engine, events) = start(&db_path, &credentials);
    wait_for(&events, |e| {
        matches!(e, Event::AccountsLoaded(_)).then_some(())
    });
    let state = finished_download(&events, &db_path, &account_id, &item_id);
    let error = Db::open(&db_path)
        .unwrap()
        .store()
        .downloads()
        .get(&account_id, &item_id)
        .unwrap()
        .unwrap()
        .error;
    engine.shutdown();
    assert_eq!(state, DownloadState::Done, "{error:?}");
    assert!(name(0).is_file() && name(1).is_file());
    assert!(!complete_part.exists());
}

#[test]
fn downloaded_book_plays_when_the_server_answers_502() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let proxy = FlakyProxy::new(SERVER);
    let (engine, events) = start(&db_path, &Credentials::memory());
    let account_id = add(&engine, &events, &proxy.url, "traveller");
    let item_id = happy_prince(&db_path, &account_id);
    download(&engine, &events, &db_path, &account_id, &item_id);

    proxy.set_broken(true);
    engine.send(Command::StartPlayback {
        account_id,
        item_id,
    });
    let started = wait_for(&events, |e| match e {
        Event::PlaybackStarted(start) => Some(Ok(start.local)),
        Event::PlaybackFailed { message } => Some(Err(message.clone())),
        _ => None,
    });
    engine.shutdown();
    assert_eq!(started, Ok(true));
}

#[test]
fn reading_position_survives_a_502() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let proxy = FlakyProxy::new(SERVER);
    let (engine, events) = start(&db_path, &Credentials::memory());
    let account_id = add(&engine, &events, &proxy.url, "reader");
    let books = items(&db_path, &account_id, "Ebooks");
    let item_id = books[0].0.clone();
    let location = format!("{}", 1000 + std::process::id() % 1000);

    std::thread::sleep(SYNC_SETTLES);
    proxy.set_broken(true);
    engine.send(Command::SaveEbookProgress {
        account_id: account_id.clone(),
        item_id: item_id.clone(),
        location: EbookLocation::Page(location.parse().unwrap()),
        progress: 0.4,
    });
    std::thread::sleep(Duration::from_secs(2));
    let queued = Db::open(&db_path)
        .unwrap()
        .store()
        .queue()
        .peek(&account_id, 10)
        .unwrap()
        .len();
    assert_eq!(queued, 1, "the position waits for the server");

    proxy.set_broken(false);
    engine.send(Command::SyncNow {
        account_id: Some(account_id.clone()),
    });
    let reached = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(wait_location(&item_id, &location));
    engine.shutdown();
    assert_eq!(reached, location);
}

/// Polls the server until the reader's location of the item is `expected`, or 90 s passed.
/// An account that went offline with the proxy only reconnects on the engine's minute tick.
async fn wait_location(item_id: &str, expected: &str) -> String {
    let client = abs_api::AbsClient::connect(http_client().unwrap(), SERVER)
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
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[test]
fn late_report_of_the_previous_book_does_not_move_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let (engine, events) = start(&db_path, &Credentials::memory());
    let account_id = add(&engine, &events, SERVER, "listener");
    let books = items(&db_path, &account_id, "Audiobooks");
    let (first, second) = (books[0].0.clone(), books[1].0.clone());
    for item_id in [&first, &second] {
        engine.send(Command::StartPlayback {
            account_id: account_id.clone(),
            item_id: item_id.clone(),
        });
        wait_for(&events, |e| {
            matches!(e, Event::PlaybackStarted(_)).then_some(())
        });
    }
    let stale = 300.0 + f64::from(std::process::id() % 1000) / 10.0;
    engine.send(Command::ReportPlayback {
        item_id: first,
        position: stale,
        playing: false,
        force_sync: true,
    });
    std::thread::sleep(Duration::from_secs(3));
    let local = Db::open(&db_path)
        .unwrap()
        .store()
        .progress()
        .get(&account_id, &second)
        .unwrap();
    engine.shutdown();
    assert_ne!(local.map(|p| p.current_time), Some(stale));
}

#[tokio::test]
async fn old_session_survives_a_reconnect() {
    let Ok(server) = std::env::var("ABS_TEST_SHORT_GRACE_SERVER") else {
        return;
    };
    let db = Mutex::new(Db::open_in_memory().unwrap());
    let credentials = Credentials::memory();
    let http = http_client().unwrap();
    let old = add_account(
        &db,
        &credentials,
        http.clone(),
        &server,
        "admin",
        "admin-pass",
    )
    .await
    .unwrap();
    let new = reconnect_account(&db, http, &old.client, old.account.clone())
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    old.client
        .refresh_now()
        .await
        .expect("the old session can still refresh");
    new.client
        .refresh_now()
        .await
        .expect("the new session can still refresh");
    assert_eq!(old.client.refresh_token(), new.client.refresh_token());
}
