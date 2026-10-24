//! Socket.IO realtime events over a WebSocket, with reconnect and re-auth.
//!
//! Own Engine.IO client instead of a Socket.IO crate; see `docs/decisions/0003-realtime-client.md`.

mod connection;
mod events;
mod packet;

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::AbsClient;
use connection::Ended;
pub use events::RealtimeEvent;

const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(60);
const CHANNEL_SIZE: usize = 256;

/// A running realtime connection for one account. Dropping it disconnects.
pub struct Realtime {
    events: mpsc::Receiver<RealtimeEvent>,
    task: JoinHandle<()>,
}

impl Realtime {
    /// Connects in the background. Must be called inside a tokio runtime.
    ///
    /// Sends the current access token after connecting and again whenever it changes.
    /// Stops after logout, or after [`RealtimeEvent::AuthFailed`] when a token refresh does not help.
    pub fn spawn(client: Arc<AbsClient>) -> Self {
        let (sender, events) = mpsc::channel(CHANNEL_SIZE);
        let task = tokio::spawn(run(client, sender));
        Self { events, task }
    }

    /// The next event; `None` once the connection gave up.
    pub async fn next(&mut self) -> Option<RealtimeEvent> {
        self.events.recv().await
    }
}

impl Drop for Realtime {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn run(client: Arc<AbsClient>, events: mpsc::Sender<RealtimeEvent>) {
    let mut backoff = MIN_BACKOFF;
    while client.is_logged_in() {
        let _ = client.refresh_if_due().await;
        let mut ready = false;
        match connection::session(&client, &events, &mut ready).await {
            Ok(Ended::ReceiverGone) => return,
            Ok(Ended::AuthFailed) => {
                if client.refresh_now().await.is_err_and(|e| e.needs_login()) {
                    return;
                }
            }
            Err(reason) => {
                if events
                    .send(RealtimeEvent::Disconnected(reason))
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        if ready {
            backoff = MIN_BACKOFF;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}
