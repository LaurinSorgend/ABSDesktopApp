//! Several accounts at once. Run with `cargo test -p abs-core --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::collections::HashSet;
use std::sync::mpsc::Receiver;

use common::{start, wait_for};

use abs_api::{AbsClient, http_client};
use abs_core::credentials::Credentials;
use abs_core::engine::{Command, Engine, Event};
use serde_json::{Value, json};

const OLDEST: &str = "http://localhost:13378";
const LATEST: &str = "http://localhost:13379";

#[test]
fn two_servers_at_once_and_disable() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("app.db");
    let credentials = Credentials::memory();

    let (engine, events) = start(&db_path, &credentials);
    let oldest = add(&engine, &events, OLDEST);
    let latest = add(&engine, &events, LATEST);
    assert_ne!(oldest, latest);
    engine.send(Command::SetAccountEnabled {
        account_id: oldest.clone(),
        enabled: false,
    });
    let updated = wait_for(&events, |e| match e {
        Event::AccountUpdated(account) => Some(account.clone()),
        _ => None,
    });
    assert_eq!(updated.id, oldest);
    assert!(!updated.enabled);
    engine.shutdown();

    let (engine, events) = start(&db_path, &credentials);
    let loaded = wait_for(&events, |e| match e {
        Event::AccountsLoaded(accounts) => Some(accounts.clone()),
        _ => None,
    });
    assert_eq!(loaded.len(), 2);
    let connected = connected_until_synced(&events, &latest);
    assert!(!connected.contains(&oldest), "a disabled account stays off");

    engine.send(Command::SetAccountEnabled {
        account_id: oldest.clone(),
        enabled: true,
    });
    wait_for(&events, |e| match e {
        Event::AccountConnected { account_id } => (account_id == &oldest).then_some(()),
        _ => None,
    });
    engine.shutdown();
}

#[test]
fn api_key_account_survives_restart() {
    for url in [OLDEST, LATEST] {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("app.db");
        let credentials = Credentials::memory();
        let api_key = listener_api_key(url);

        let (engine, events) = start(&db_path, &credentials);
        engine.send(Command::AddAccountWithKey {
            url: url.into(),
            api_key,
        });
        let account = wait_for(&events, |e| match e {
            Event::AccountAdded(account) => Some(account.clone()),
            _ => None,
        });
        assert!(account.api_key, "{url}");
        assert_eq!(account.username, "listener");
        engine.shutdown();

        let (engine, events) = start(&db_path, &credentials);
        let connected = connected_until_synced(&events, &account.id);
        assert!(connected.contains(&account.id), "{url}");
        engine.shutdown();
    }
}

/// A new API key for `listener`, created by the admin.
fn listener_api_key(url: &str) -> String {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let listener = AbsClient::connect(http_client().unwrap(), url)
            .await
            .unwrap();
        let user_id = listener
            .login("listener", "listener-pass")
            .await
            .unwrap()
            .user
            .id;
        let admin = AbsClient::connect(http_client().unwrap(), url)
            .await
            .unwrap();
        admin.login("admin", "admin-pass").await.unwrap();
        let name = format!("core-contract-{}", std::process::id());
        let body = json!({ "name": name, "isActive": true, "userId": user_id });
        let response: Value = http_client()
            .unwrap()
            .post(admin.url("api/api-keys").unwrap())
            .bearer_auth(admin.access_token().unwrap())
            .json(&body)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        response["apiKey"]["apiKey"].as_str().unwrap().to_owned()
    })
}

fn add(engine: &Engine, events: &Receiver<Event>, url: &str) -> String {
    engine.send(Command::AddAccount {
        url: url.into(),
        username: "listener".into(),
        password: "listener-pass".into(),
    });
    let account_id = wait_for(events, |e| match e {
        Event::AccountAdded(account) => Some(account.id.clone()),
        _ => None,
    });
    wait_for(events, |e| match e {
        Event::CatalogSynced { account_id: id, .. } => (id == &account_id).then_some(()),
        _ => None,
    });
    account_id
}

/// Accounts that connected before `account_id` finished its catalog sync.
fn connected_until_synced(events: &Receiver<Event>, account_id: &str) -> HashSet<String> {
    let mut connected = HashSet::new();
    wait_for(events, |e| match e {
        Event::AccountConnected { account_id } => {
            connected.insert(account_id.clone());
            None
        }
        Event::CatalogSynced { account_id: id, .. } => (id == account_id).then_some(()),
        _ => None,
    });
    connected
}
