//! Account and sync flows against the Docker servers.
//! Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

use std::sync::Mutex;

use abs_api::http_client;
use abs_core::account::{add_account, restore_account};
use abs_core::credentials::Credentials;
use abs_core::db::{Db, ItemQuery};
use abs_core::sync::{sync_catalog, sync_progress};

fn servers() -> Vec<String> {
    std::env::var("ABS_TEST_SERVERS")
        .unwrap_or_else(|_| "http://localhost:13378,http://localhost:13379".into())
        .split(',')
        .map(|s| s.trim().to_owned())
        .collect()
}

#[tokio::test]
async fn add_sync_and_restore() {
    for server in servers() {
        let db = Mutex::new(Db::open_in_memory().unwrap());
        let credentials = Credentials::memory();
        let http = http_client().unwrap();
        let session = add_account(
            &db,
            &credentials,
            http.clone(),
            &server,
            "listener",
            "listener-pass",
        )
        .await
        .unwrap();
        assert!(credentials.load(&session.account.id).unwrap().is_some());

        let report = sync_catalog(&session, &db).await.unwrap();
        assert_eq!(report.libraries, 3, "{server}");
        assert!(report.items >= 502, "{server}: {report:?}");
        let grid = grid_count(&db, &session.account.id);
        assert_eq!(grid, 500, "{server}");
        let again = sync_catalog(&session, &db).await.unwrap();
        assert_eq!(again.removed, 0);

        sync_progress(&session, &db).await.unwrap();
        assert!(
            sync_progress(&session, &db).await.unwrap().is_empty(),
            "second pull changes nothing"
        );

        let readded = add_account(
            &db,
            &credentials,
            http.clone(),
            &server,
            "listener",
            "listener-pass",
        )
        .await
        .unwrap();
        assert_eq!(
            readded.account.id, session.account.id,
            "{server}: same account"
        );

        let restored = restore_account(&db, &credentials, http, session.account.clone())
            .await
            .unwrap();
        assert_eq!(restored.login.user.username, "listener");
    }
}

fn grid_count(db: &Mutex<Db>, account_id: &str) -> i64 {
    let db = db.lock().unwrap();
    let store = db.store();
    let catalog = store.catalog();
    let grid = catalog
        .libraries(account_id)
        .unwrap()
        .into_iter()
        .find(|l| l.name == "Grid")
        .unwrap();
    let first = catalog
        .items_page(account_id, &ItemQuery::new(&grid.remote_id), 0, 1)
        .unwrap();
    assert_eq!(first.len(), 1);
    catalog
        .item_count(account_id, &ItemQuery::new(&grid.remote_id))
        .unwrap()
}
