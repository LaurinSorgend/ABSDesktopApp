//! The engine against the Docker servers. Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use abs_core::credentials::Credentials;
use abs_core::engine::{Command, Engine, EngineConfig, Event};

const WAIT: Duration = Duration::from_secs(30);

fn start(db_path: &std::path::Path, credentials: &Credentials) -> (Engine, Receiver<Event>) {
    let (sender, events) = channel();
    let config = EngineConfig {
        db_path: db_path.to_owned(),
        credentials: credentials.clone(),
    };
    let engine = Engine::start(config, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    (engine, events)
}

/// Waits for the first event `pick` accepts and returns what it extracted.
fn wait_for<T>(events: &Receiver<Event>, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    loop {
        let event = events.recv_timeout(WAIT).expect("engine event");
        if let Some(value) = pick(&event) {
            return value;
        }
        assert!(
            !matches!(
                event,
                Event::AddAccountFailed { .. } | Event::SyncFailed { .. }
            ),
            "{event:?}"
        );
    }
}

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
    let phone = abs_api::AbsClient::connect(abs_api::http_client().unwrap(), server)
        .await
        .unwrap();
    phone.login("listener", "listener-pass").await.unwrap();
    let libraries = phone.libraries().await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Audiobooks").unwrap();
    let items = phone
        .library_items(&library.id, &abs_api::ItemsQuery::default())
        .await
        .unwrap();
    let item_id = items.results[0].id.clone();
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
