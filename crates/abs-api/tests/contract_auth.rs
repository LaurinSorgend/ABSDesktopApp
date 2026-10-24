//! Auth contract tests. Run with `cargo test -p abs-api --features integration`.
#![cfg(feature = "integration")]

mod common;

use abs_api::{Error, Feature, ServerVersion};
use futures::future::join_all;
use serde_json::{Value, json};

#[tokio::test]
async fn connect_reads_version() {
    for server in common::servers() {
        let (client, _) = common::client(&server).await;
        assert!(client.server_version() >= ServerVersion::MINIMUM);
        assert!(client.supports(Feature::RefreshTokens));
        assert!(!client.is_logged_in());
    }
}

#[tokio::test]
async fn wrong_password_is_invalid_credentials() {
    for server in common::servers() {
        let (client, stored) = common::client(&server).await;
        let err = client.login("listener", "wrong").await.unwrap_err();
        assert!(matches!(err, Error::InvalidCredentials), "{server}: {err}");
        assert!(stored.all().is_empty());
    }
}

#[tokio::test]
async fn login_stores_refresh_token_and_schedules_refresh() {
    for server in common::servers() {
        let (client, stored) = common::listener(&server).await;
        assert_eq!(stored.all().len(), 1);
        assert!(client.refresh_due_at().is_some());
        assert_eq!(client.me().await.unwrap().username, "listener");
    }
}

#[tokio::test]
async fn refresh_stores_each_new_token() {
    for server in common::servers() {
        let (client, stored) = common::listener(&server).await;
        client.refresh_now().await.unwrap();
        assert_eq!(stored.all().len(), 2, "{server}");
        let rotated = stored.all()[0] != stored.all()[1];
        println!("{server}: refresh token rotated: {rotated}");
        client.me().await.unwrap();
    }
}

#[tokio::test]
async fn restore_session_from_stored_token() {
    for server in common::servers() {
        let (first, stored) = common::listener(&server).await;
        drop(first);
        let (client, restored) = common::client(&server).await;
        let login = client.restore_session(&stored.last()).await.unwrap();
        assert_eq!(login.user.username, "listener");
        assert_eq!(restored.all().len(), 1);
    }
}

#[tokio::test]
async fn invalid_access_token_recovers_with_one_refresh() {
    for server in common::servers() {
        let (client, stored) = common::listener(&server).await;
        client.break_access_token();
        let calls = (0..5).map(|_| client.me());
        for result in join_all(calls).await {
            assert_eq!(result.unwrap().username, "listener");
        }
        assert_eq!(stored.all().len(), 2, "{server}: one refresh for five 401s");
    }
}

#[tokio::test]
async fn logout_invalidates_refresh_token() {
    for server in common::servers() {
        let (client, stored) = common::client(&server).await;
        client.login("leaver", "leaver-pass").await.unwrap();
        client.logout().await.unwrap();
        assert!(!client.is_logged_in());
        assert!(matches!(client.me().await, Err(Error::LoginRequired)));
        let (other, _) = common::client(&server).await;
        let err = other.restore_session(&stored.last()).await.unwrap_err();
        assert!(err.needs_login(), "{server}: {err}");
    }
}

#[tokio::test]
async fn api_key_login() {
    for server in common::servers() {
        let key = create_api_key(&server).await;
        let (client, stored) = common::client(&server).await;
        let login = client.login_api_key(&key).await.unwrap();
        assert_eq!(login.user.username, "admin");
        assert!(client.refresh_due_at().is_none());
        assert!(stored.all().is_empty());
        let (other, _) = common::client(&server).await;
        let err = other.login_api_key("not-a-key").await.unwrap_err();
        assert!(matches!(err, Error::InvalidCredentials), "{server}: {err}");
    }
}

async fn create_api_key(server: &str) -> String {
    let admin = common::admin(server).await;
    let user_id = admin.me().await.unwrap().id;
    let name = format!("contract-{}", std::process::id());
    let body = json!({ "name": name, "isActive": true, "userId": user_id });
    let response: Value = reqwest::Client::new()
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
}
