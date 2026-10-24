//! Shared setup for the contract tests against the Docker servers in `docker/`.
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::sync::{Arc, Mutex};

use abs_api::{AbsClient, SinkError, http_client};

/// Servers from `ABS_TEST_SERVERS` (comma separated), else both Docker servers.
pub fn servers() -> Vec<String> {
    std::env::var("ABS_TEST_SERVERS")
        .unwrap_or_else(|_| "http://localhost:13378,http://localhost:13379".into())
        .split(',')
        .map(|s| s.trim().to_owned())
        .collect()
}

/// Every refresh token the client handed to its sink, oldest first.
#[derive(Clone, Default)]
pub struct Stored(Arc<Mutex<Vec<String>>>);

impl Stored {
    pub fn all(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    pub fn last(&self) -> String {
        self.all().last().cloned().expect("no refresh token stored")
    }
}

/// A logged-out client with a recording token sink.
pub async fn client(server: &str) -> (AbsClient, Stored) {
    let stored = Stored::default();
    let sink = stored.clone();
    let client = AbsClient::connect(http_client().unwrap(), server)
        .await
        .unwrap_or_else(|e| panic!("{server}: {e}"))
        .with_token_sink(move |token: &str| -> Result<(), SinkError> {
            sink.0.lock().unwrap().push(token.to_owned());
            Ok(())
        });
    (client, stored)
}

/// A client logged in as the non-admin test user.
pub async fn listener(server: &str) -> (AbsClient, Stored) {
    let (client, stored) = client(server).await;
    client.login("listener", "listener-pass").await.unwrap();
    (client, stored)
}

/// A client logged in as the admin.
pub async fn admin(server: &str) -> AbsClient {
    let (client, _) = client(server).await;
    client.login("admin", "admin-pass").await.unwrap();
    client
}
