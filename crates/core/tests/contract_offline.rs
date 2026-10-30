//! Offline listening against the Docker servers: download, lose the server, play,
//! get it back, and check what reaches it. The server goes away by switching the
//! account to an address nobody answers on, so other tests keep their server.
//! Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::path::Path;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use abs_api::AbsClient;
use abs_api::models::{DeviceInfo, PlayRequest, SessionSync};
use abs_core::credentials::Credentials;
use abs_core::db::{Db, DownloadState, ItemQuery};
use abs_core::engine::{Command, Engine, Event};
use common::{start, wait_for};

const DEAD: &str = "http://127.0.0.1:9/";

struct Setup {
    engine: Engine,
    events: Receiver<Event>,
    db_path: std::path::PathBuf,
    server: String,
    account_id: String,
    item_id: String,
    _dir: tempfile::TempDir,
}

#[test]
fn offline_listening_reaches_the_server_and_conflicts_ask_first() {
    let setup = downloaded_book();
    let runtime = tokio::runtime::Runtime::new().unwrap();

    go_offline(&setup);
    listen(&setup, 500.0);
    assert_eq!(local_sessions(&setup), 1);
    go_online(&setup);
    let position = runtime.block_on(wait_server_position(&setup, 502.0));
    assert!((position - 502.0).abs() < 3.0, "server has {position}");
    assert_eq!(local_sessions(&setup), 0);

    go_offline(&setup);
    listen(&setup, 900.0);
    runtime.block_on(listen_on_phone(&setup.server, &setup.item_id, 4000.0));
    go_online(&setup);
    let conflict = wait_for(&setup.events, |e| match e {
        Event::ProgressConflict(c) => Some(c.clone()),
        _ => None,
    });
    assert!(
        (conflict.server_position - 4000.0).abs() < 1.0,
        "{conflict:?}"
    );
    assert_eq!(local_sessions(&setup), 1, "waits for the answer");
    setup.engine.send(Command::ResolveConflict {
        account_id: setup.account_id.clone(),
        item_id: setup.item_id.clone(),
        keep_local: true,
    });
    let position = runtime.block_on(wait_server_position(&setup, 902.0));
    assert!(
        (position - 902.0).abs() < 3.0,
        "kept the local position, server has {position}"
    );

    go_offline(&setup);
    listen_with_phone_in_between(&setup, &runtime, 1300.0, 5000.0);
    go_online(&setup);
    wait_for(&setup.events, |e| {
        matches!(e, Event::ProgressConflict(_)).then_some(())
    });
    setup.engine.send(Command::ResolveConflict {
        account_id: setup.account_id.clone(),
        item_id: setup.item_id.clone(),
        keep_local: false,
    });
    let local = wait_local_position(&setup, 5000.0);
    assert!(
        (local - 5000.0).abs() < 1.0,
        "took the server position, local row has {local}"
    );
}

/// A local session during which a phone moves the server position, so the local row's
/// last write is newer than the phone's.
fn listen_with_phone_in_between(
    setup: &Setup,
    runtime: &tokio::runtime::Runtime,
    from: f64,
    phone: f64,
) {
    setup.engine.send(Command::StartPlayback {
        account_id: setup.account_id.clone(),
        item_id: setup.item_id.clone(),
    });
    wait_for(&setup.events, |e| {
        matches!(e, Event::PlaybackStarted(_)).then_some(())
    });
    report(setup, from, false);
    std::thread::sleep(Duration::from_secs(2));
    runtime.block_on(listen_on_phone(&setup.server, &setup.item_id, phone));
    std::thread::sleep(Duration::from_secs(2));
    report(setup, from + 4.0, true);
    wait_for(&setup.events, |e| {
        matches!(e, Event::ProgressChanged { .. }).then_some(())
    });
    setup.engine.send(Command::StopPlayback);
}

/// The local position once it is near `target`, or the last one after 15 s.
fn wait_local_position(setup: &Setup, target: f64) -> f64 {
    let mut position = 0.0;
    for _ in 0..60 {
        let db = Db::open(&setup.db_path).unwrap();
        let row = db.store().progress().get(&setup.account_id, &setup.item_id);
        position = row.unwrap().map_or(0.0, |p| p.current_time);
        if (position - target).abs() < 1.0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    position
}

/// A fresh engine with the traveller account and the multi-file book downloaded.
fn downloaded_book() -> Setup {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let (engine, events) = start(&db_path, &Credentials::memory());
    engine.send(Command::AddAccount {
        url: server.clone(),
        username: "traveller".into(),
        password: "traveller-pass".into(),
    });
    let account_id = wait_for(&events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(&events, |e| {
        matches!(e, Event::CatalogSynced { .. }).then_some(())
    });
    let item_id = multi_file_book(&db_path, &account_id);
    engine.send(Command::Download {
        account_id: account_id.clone(),
        item_id: item_id.clone(),
    });
    let setup = Setup {
        engine,
        events,
        db_path,
        server,
        account_id,
        item_id,
        _dir: dir,
    };
    while download_state(&setup) != Some(DownloadState::Done) {
        wait_for(&setup.events, |e| {
            matches!(e, Event::DownloadsChanged { .. }).then_some(())
        });
    }
    setup
}

fn go_offline(setup: &Setup) {
    set_urls(setup, vec![DEAD.into()]);
    wait_for(&setup.events, |e| {
        matches!(e, Event::Offline { .. }).then_some(())
    });
}

fn go_online(setup: &Setup) {
    set_urls(setup, vec![setup.server.clone()]);
    wait_for(&setup.events, |e| {
        matches!(e, Event::AccountConnected { .. }).then_some(())
    });
}

fn set_urls(setup: &Setup, urls: Vec<String>) {
    setup.engine.send(Command::SetServerUrls {
        account_id: setup.account_id.clone(),
        urls,
    });
}

/// Plays the download from `from` for two seconds, then stops.
fn listen(setup: &Setup, from: f64) {
    setup.engine.send(Command::StartPlayback {
        account_id: setup.account_id.clone(),
        item_id: setup.item_id.clone(),
    });
    let started = wait_for(&setup.events, |e| match e {
        Event::PlaybackStarted(start) => Some(start.clone()),
        _ => None,
    });
    assert!(started.local);
    report(setup, from, false);
    std::thread::sleep(Duration::from_secs(2));
    report(setup, from + 2.0, true);
    wait_for(&setup.events, |e| {
        matches!(e, Event::ProgressChanged { .. }).then_some(())
    });
    setup.engine.send(Command::StopPlayback);
}

fn report(setup: &Setup, position: f64, force_sync: bool) {
    setup.engine.send(Command::ReportPlayback {
        position,
        playing: true,
        force_sync,
    });
}

/// The server position once it is near `target`, or the last one after 15 s.
async fn wait_server_position(setup: &Setup, target: f64) -> f64 {
    let mut position = 0.0;
    for _ in 0..60 {
        position = server_position(&setup.server, &setup.item_id).await;
        if (position - target).abs() < 3.0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    position
}

fn local_sessions(setup: &Setup) -> usize {
    let db = Db::open(&setup.db_path).unwrap();
    let sessions = db.store().local_sessions().list(&setup.account_id);
    sessions.unwrap().len()
}

fn download_state(setup: &Setup) -> Option<DownloadState> {
    let db = Db::open(&setup.db_path).unwrap();
    let row = db
        .store()
        .downloads()
        .get(&setup.account_id, &setup.item_id);
    row.unwrap().map(|r| r.state)
}

fn multi_file_book(db_path: &Path, account_id: &str) -> String {
    let db = Db::open(db_path).unwrap();
    let catalog = db.store().catalog();
    let library = catalog
        .libraries(account_id)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Audiobooks")
        .unwrap();
    let items = catalog
        .items_page(account_id, &ItemQuery::new(&library.remote_id), 0, 10)
        .unwrap();
    let book = items
        .iter()
        .find(|i| i.title.starts_with("The Happy Prince and"));
    book.unwrap().remote_id.clone()
}

async fn traveller(server: &str) -> AbsClient {
    let client = AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    client.login("traveller", "traveller-pass").await.unwrap();
    client
}

async fn server_position(server: &str, item_id: &str) -> f64 {
    let progress = traveller(server)
        .await
        .item_progress(item_id)
        .await
        .unwrap();
    progress.and_then(|p| p.current_time).unwrap_or_default()
}

/// Moves the server position from another device while the desktop is offline.
async fn listen_on_phone(server: &str, item_id: &str, position: f64) {
    let phone = traveller(server).await;
    let request = PlayRequest {
        device_info: DeviceInfo {
            device_id: "offline-test-phone".into(),
            client_name: "contract-tests".into(),
            client_version: "0".into(),
            manufacturer: None,
            model: None,
        },
        media_player: "test".into(),
        supported_mime_types: vec!["audio/mpeg".into()],
        force_direct_play: true,
        force_transcode: false,
    };
    let session = phone.play(item_id, &request).await.unwrap();
    let sync = SessionSync {
        current_time: position,
        time_listened: 5.0,
        duration: session.duration,
    };
    phone.sync_session(&session.id, &sync).await.unwrap();
    phone.close_session(&session.id, None).await.unwrap();
}
