//! Playback sessions against the Docker servers.
//! Run with `cargo test -p abs-core --features integration`.
//! Set `ABS_TEST_RESTART_CONTAINER=abs-test-abs-latest-1` to also test a server restart;
//! run that test alone (`-- sync_after_server_restart`), it takes the server down.
#![cfg(feature = "integration")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use abs_api::http_client;
use abs_core::account::{Session, add_account};
use abs_core::credentials::Credentials;
use abs_core::db::{Db, ItemQuery};
use abs_core::playback::{self, ActivePlayback};
use abs_core::sync::sync_catalog;

const LATEST: &str = "http://localhost:13379";

async fn setup(server: &str) -> (Mutex<Db>, Arc<Session>, String) {
    let db = Mutex::new(Db::open_in_memory().unwrap());
    let credentials = Credentials::memory();
    let session = add_account(
        &db,
        &credentials,
        http_client().unwrap(),
        server,
        "listener",
        "listener-pass",
    )
    .await
    .unwrap();
    sync_catalog(&session, &db).await.unwrap();
    let item_id = {
        let db = db.lock().unwrap();
        let store = db.store();
        let library = store
            .catalog()
            .libraries(&session.account.id)
            .unwrap()
            .into_iter()
            .find(|l| l.name == "Audiobooks")
            .unwrap();
        store
            .catalog()
            .items_page(
                &session.account.id,
                &ItemQuery::new(&library.remote_id),
                0,
                1,
            )
            .unwrap()[0]
            .remote_id
            .clone()
    };
    (db, Arc::new(session), item_id)
}

fn listen(active: &mut ActivePlayback, position: f64) {
    let now = Instant::now();
    active.tracker.report(now, position - 2.0, true);
    active
        .tracker
        .report(now + Duration::from_secs(2), position, true);
}

#[tokio::test]
async fn start_sync_and_close() {
    for server in ["http://localhost:13378", LATEST] {
        let (db, session, item_id) = setup(server).await;
        let (mut active, start) = playback::start(session.clone(), &item_id).await.unwrap();
        assert!(!start.tracks.is_empty(), "{server}");
        assert!(start.http_headers[0].starts_with("Authorization: Bearer "));
        listen(&mut active, 444.0);
        active.sync(&db).await.unwrap();
        let progress = session
            .client
            .item_progress(&item_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(progress.current_time, Some(444.0), "{server}");
        let local = db
            .lock()
            .unwrap()
            .store()
            .progress()
            .get(&session.account.id, &item_id)
            .unwrap();
        assert_eq!(local.unwrap().current_time, 444.0);
        active.close(&db).await.unwrap();
    }
}

#[tokio::test]
async fn sync_after_server_restart_starts_a_new_session() {
    let Ok(container) = std::env::var("ABS_TEST_RESTART_CONTAINER") else {
        return;
    };
    let (db, session, item_id) = setup(LATEST).await;
    let (mut active, _) = playback::start(session.clone(), &item_id).await.unwrap();
    let first_id = active.server_session_id.clone();
    let status = std::process::Command::new("docker")
        .args(["restart", &container])
        .status()
        .unwrap();
    assert!(status.success());
    wait_until_up(&session).await;
    listen(&mut active, 555.0);
    active.sync(&db).await.unwrap();
    assert_ne!(
        active.server_session_id, first_id,
        "a new session was started"
    );
    let progress = session
        .client
        .item_progress(&item_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(progress.current_time, Some(555.0));
    active.close(&db).await.unwrap();
}

async fn wait_until_up(session: &Session) {
    for _ in 0..60 {
        if session.client.me().await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    panic!("server did not come back");
}
