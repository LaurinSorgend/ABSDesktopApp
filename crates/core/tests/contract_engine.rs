//! The engine against the Docker servers. Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::sync::mpsc::Receiver;

use common::{start, wait_for};

use abs_core::credentials::Credentials;
use abs_core::db::{Db, DownloadRow, DownloadState, ItemQuery};
use abs_core::downloads;
use abs_core::engine::{Command, Event, GroupKind};

#[test]
fn add_restart_and_remove_account() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let credentials = Credentials::memory();

    let (engine, events) = start(&db_path, &credentials);
    wait_for(&events, |e| {
        matches!(e, Event::AccountsLoaded(a) if a.is_empty()).then_some(())
    });
    engine.send(Command::AddAccount {
        url: server.clone(),
        username: "listener".into(),
        password: "listener-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    let report = wait_for(&events, |e| match e {
        Event::CatalogSynced { report, .. } => Some(report.clone()),
        _ => None,
    });
    assert!(report.items >= 502, "{report:?}");
    let item_id = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(listen_on_phone(&server));
    wait_for(&events, |e| match e {
        Event::ProgressChanged { item_ids, .. } => item_ids.contains(&item_id).then_some(()),
        _ => None,
    });
    engine.shutdown();

    let (engine, events) = start(&db_path, &credentials);
    let loaded = wait_for(&events, |e| match e {
        Event::AccountsLoaded(accounts) => Some(accounts.clone()),
        _ => None,
    });
    assert_eq!(loaded.len(), 1);
    wait_for(&events, |e| {
        matches!(e, Event::AccountConnected { .. }).then_some(())
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    engine.send(Command::RemoveAccount {
        account_id: account_id.clone(),
    });
    wait_for(&events, |e| {
        matches!(e, Event::AccountRemoved { .. }).then_some(())
    });
    assert_eq!(credentials.load(&account_id).unwrap(), None);
}

/// Plays and syncs an item from a second client, as a phone would. Returns the item id.
async fn listen_on_phone(server: &str) -> String {
    use abs_api::models::{DeviceInfo, PlayRequest, SessionSync};
    let phone = listener(server).await;
    let item_id = first_item(&phone).await;
    let request = PlayRequest {
        device_info: DeviceInfo {
            device_id: "engine-test-phone".into(),
            client_name: "contract-tests".into(),
            client_version: "0".into(),
            manufacturer: None,
            model: None,
        },
        media_player: "test".into(),
        supported_mime_types: vec!["audio/mpeg".into(), "audio/mp4".into()],
        force_direct_play: true,
        force_transcode: false,
    };
    let session = phone.play(&item_id, &request).await.unwrap();
    let sync = SessionSync {
        current_time: 99.0,
        time_listened: 2.0,
        duration: session.duration,
    };
    phone.sync_session(&session.id, &sync).await.unwrap();
    phone.close_session(&session.id, None).await.unwrap();
    item_id
}

#[test]
fn playback_session_through_the_engine() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let credentials = Credentials::memory();
    let (engine, events) = start(&dir.path().join("app.db"), &credentials);
    engine.send(Command::AddAccount {
        url: server.clone(),
        username: "leaver".into(),
        password: "leaver-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    let item_id = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { first_item(&listener(&server).await).await });
    engine.send(Command::StartPlayback {
        account_id,
        item_id: item_id.clone(),
    });
    let started = wait_for(&events, |e| match e {
        Event::PlaybackStarted(start) => Some(start.clone()),
        _ => None,
    });
    assert_eq!(started.item_id, item_id);
    assert!(!started.tracks.is_empty());
    for (position, force_sync) in [(700.0, false), (701.0, false), (702.0, true)] {
        engine.send(Command::ReportPlayback {
            item_id: item_id.clone(),
            position,
            playing: true,
            force_sync,
        });
    }
    wait_for(&events, |e| match e {
        Event::ProgressChanged { item_ids, .. } => item_ids.contains(&item_id).then_some(()),
        _ => None,
    });
    engine.send(Command::ReportPlayback {
        item_id: item_id.clone(),
        position: 703.0,
        playing: true,
        force_sync: false,
    });
    engine.shutdown();
    let progress = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(server_progress(&server, &item_id));
    assert_eq!(
        progress,
        Some(703.0),
        "closing on exit sends the last position"
    );
}

#[test]
fn ui_requests() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let (engine, events) = start(&db_path, &Credentials::memory());
    engine.send(Command::AddAccount {
        url: server,
        username: "admin".into(),
        password: "admin-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    let (library_id, item) = audiobooks(&db_path, &account_id);
    let request = |command: Command| engine.send(command);
    let account = || account_id.clone();

    request(Command::LoadCover {
        account_id: account(),
        item_id: item.clone(),
        etag: Some("e1".into()),
        width: 120,
    });
    let cover = wait_for(&events, |e| match e {
        Event::CoverLoaded { bytes, .. } => Some(bytes.clone()),
        _ => None,
    });
    assert!(
        cover.is_some_and(|b| b.starts_with(b"RIFF")),
        "a WebP cover"
    );

    request(Command::LoadItem {
        account_id: account(),
        item_id: item.clone(),
    });
    let json = wait_for(&events, |e| match e {
        Event::ItemLoaded { json, .. } => Some(json.clone()),
        _ => None,
    });
    assert!(json.contains("\"chapters\""));

    request(Command::LoadShelves {
        account_id: account(),
        library_id: library_id.clone(),
    });
    let shelves = wait_for(&events, |e| match e {
        Event::ShelvesLoaded { shelves, .. } => Some(shelves.clone()),
        _ => None,
    });
    assert!(!shelves.is_empty());

    request(Command::SearchServer {
        account_id: account(),
        library_id: library_id.clone(),
        text: "marathon".into(),
    });
    let hits = wait_for(&events, |e| match e {
        Event::SearchResults { item_ids, .. } => Some(item_ids.clone()),
        _ => None,
    });
    assert!(!hits.is_empty());

    for (kind, name) in [
        (GroupKind::Series, "Fixture series"),
        (GroupKind::Collections, "Fixture favourites"),
        (GroupKind::Authors, "Oscar Wilde"),
    ] {
        request(Command::LoadGroups {
            account_id: account(),
            library_id: library_id.clone(),
            kind,
        });
        let groups = wait_for(&events, |e| match e {
            Event::GroupsLoaded { groups, .. } => Some(groups.clone()),
            _ => None,
        });
        let group = groups.into_iter().find(|g| g.name == name).unwrap();
        assert!(group.count >= 1, "{name}");
        if kind == GroupKind::Authors {
            request(Command::LoadAuthorItems {
                account_id: account(),
                library_id: library_id.clone(),
                author_id: group.id.clone(),
            });
            let ids = wait_for(&events, |e| match e {
                Event::GroupItems { item_ids, .. } => Some(item_ids.clone()),
                _ => None,
            });
            assert!(!ids.is_empty());
        } else {
            assert!(!group.item_ids.is_empty(), "{name}");
        }
    }

    for finished in [true, false] {
        request(Command::SetFinished {
            account_id: account(),
            item_id: item.clone(),
            finished,
        });
        wait_for(&events, |e| {
            matches!(e, Event::ProgressChanged { .. }).then_some(())
        });
    }
    request(Command::AddBookmark {
        account_id: account(),
        item_id: item.clone(),
        time: 12.0,
        title: "Engine".into(),
    });
    wait_for(&events, |e| {
        matches!(e, Event::BookmarksChanged { .. }).then_some(())
    });
    request(Command::RemoveBookmark {
        account_id: account(),
        item_id: item.clone(),
        time: 12.0,
    });
    wait_for(&events, |e| {
        matches!(e, Event::BookmarksChanged { .. }).then_some(())
    });
    engine.shutdown();
}

/// The Audiobooks library and its first item, read through a second connection as the app does.
fn audiobooks(db_path: &std::path::Path, account_id: &str) -> (String, String) {
    let db = Db::open(db_path).unwrap();
    let store = db.store();
    let library = store
        .catalog()
        .libraries(account_id)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Audiobooks")
        .unwrap();
    let items = store
        .catalog()
        .items_page(account_id, &ItemQuery::new(&library.remote_id), 0, 1)
        .unwrap();
    (library.remote_id, items[0].remote_id.clone())
}

async fn server_progress(server: &str, item_id: &str) -> Option<f64> {
    let client = abs_api::AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    client.login("leaver", "leaver-pass").await.unwrap();
    client.item_progress(item_id).await.unwrap()?.current_time
}

async fn listener(server: &str) -> abs_api::AbsClient {
    let client = abs_api::AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    client.login("listener", "listener-pass").await.unwrap();
    client
}

async fn first_item(client: &abs_api::AbsClient) -> String {
    let libraries = client.libraries().await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Audiobooks").unwrap();
    let items = client
        .library_items(&library.id, &abs_api::ItemsQuery::default())
        .await
        .unwrap();
    items.results[0].id.clone()
}

#[test]
fn download_pause_resume_restart_and_delete() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let credentials = Credentials::memory();
    let (engine, events) = start(&db_path, &credentials);
    engine.send(Command::AddAccount {
        url: server,
        username: "listener".into(),
        password: "listener-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    let item_id = multi_file_book(&db_path, &account_id);
    let key = (account_id.clone(), item_id.clone());
    engine.send(Command::Download {
        account_id: account_id.clone(),
        item_id: item_id.clone(),
    });
    wait_for(&events, |e| {
        matches!(e, Event::DownloadProgress { .. }).then_some(())
    });
    engine.send(Command::PauseDownload {
        account_id: account_id.clone(),
        item_id: item_id.clone(),
    });
    let state = wait_download(&events, &db_path, &key, |s| {
        matches!(s, DownloadState::Paused | DownloadState::Done)
    });
    if state == DownloadState::Paused {
        engine.send(Command::ResumeDownload {
            account_id: account_id.clone(),
            item_id: item_id.clone(),
        });
    }
    wait_download(&events, &db_path, &key, |s| s == DownloadState::Done);
    let row = download(&db_path, &key).unwrap();
    assert_eq!(row.bytes_done, row.size);
    let manifest = downloads::read_manifest(&row.dir).unwrap();
    assert_eq!(manifest.tracks.len(), 5);
    assert!(row.dir.join(downloads::COVER_FILE).is_file());
    engine.send(Command::StartPlayback {
        account_id: account_id.clone(),
        item_id: item_id.clone(),
    });
    let started = wait_for(&events, |e| match e {
        Event::PlaybackStarted(start) => Some(start.clone()),
        _ => None,
    });
    assert!(started.local && started.http_headers.is_empty());
    assert!(std::path::Path::new(&started.tracks[4].url).is_file());
    assert_eq!(started.chapters.len(), manifest.chapters.len());
    engine.shutdown();

    let (engine, events) = start(&db_path, &credentials);
    wait_for(&events, |e| {
        matches!(e, Event::AccountsLoaded(_)).then_some(())
    });
    assert_eq!(download(&db_path, &key).unwrap().state, DownloadState::Done);
    engine.send(Command::DeleteDownload {
        account_id,
        item_id,
    });
    wait_for(&events, |e| {
        matches!(e, Event::DownloadsChanged { .. }).then_some(())
    });
    assert!(download(&db_path, &key).is_none());
    assert!(!row.dir.exists());
}

fn multi_file_book(db_path: &std::path::Path, account_id: &str) -> String {
    let db = Db::open(db_path).unwrap();
    let (library, _) = audiobooks(db_path, account_id);
    let items = db
        .store()
        .catalog()
        .items_page(account_id, &ItemQuery::new(&library), 0, 10)
        .unwrap();
    let book = items
        .iter()
        .find(|i| i.title.starts_with("The Happy Prince and"));
    book.unwrap().remote_id.clone()
}

fn download(db_path: &std::path::Path, key: &(String, String)) -> Option<DownloadRow> {
    let db = Db::open(db_path).unwrap();
    db.store().downloads().get(&key.0, &key.1).unwrap()
}

/// Waits for download events until the stored state passes `done`.
fn wait_download(
    events: &Receiver<Event>,
    db_path: &std::path::Path,
    key: &(String, String),
    done: impl Fn(DownloadState) -> bool,
) -> DownloadState {
    loop {
        if let Some(row) = download(db_path, key) {
            assert_ne!(row.state, DownloadState::Failed, "{:?}", row.error);
            if done(row.state) {
                return row.state;
            }
        }
        wait_for(events, |e| {
            matches!(e, Event::DownloadsChanged { .. }).then_some(())
        });
    }
}

#[test]
fn server_addresses_fall_back_and_reconnect() {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dead = "http://127.0.0.1:9/".to_owned();
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let credentials = Credentials::memory();
    let (engine, events) = start(&db_path, &credentials);
    engine.send(Command::AddAccount {
        url: server.clone(),
        username: "listener".into(),
        password: "listener-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    engine.send(Command::SetServerUrls {
        account_id: account_id.clone(),
        urls: vec![dead.clone()],
    });
    wait_for(&events, |e| {
        matches!(e, Event::Offline { .. }).then_some(())
    });
    engine.send(Command::SetServerUrls {
        account_id: account_id.clone(),
        urls: vec![dead, server],
    });
    wait_for(&events, |e| {
        matches!(e, Event::AccountConnected { .. }).then_some(())
    });
    engine.shutdown();

    let (_engine, events) = start(&db_path, &credentials);
    wait_for(&events, |e| {
        matches!(e, Event::AccountConnected { .. }).then_some(())
    });
}
