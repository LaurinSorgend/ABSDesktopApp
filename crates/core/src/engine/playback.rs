//! The engine's playback task: one server session at a time, commands in order.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use super::Event;
use super::worker::{Shared, sessions};
use crate::playback::{self, ActivePlayback};

/// Playback commands, forwarded from [`super::Command`].
#[derive(Debug)]
pub(super) enum PlaybackCommand {
    Start {
        account_id: String,
        item_id: String,
    },
    Report {
        position: f64,
        playing: bool,
        force_sync: bool,
    },
    Stop,
}

/// Runs until the channel closes, then closes the open session (app exit).
pub(super) async fn run(
    shared: Arc<Shared>,
    mut commands: mpsc::UnboundedReceiver<PlaybackCommand>,
) {
    let mut active: Option<ActivePlayback> = None;
    let mut sent_token: Option<String> = None;
    while let Some(command) = commands.recv().await {
        match command {
            PlaybackCommand::Start {
                account_id,
                item_id,
            } => {
                close(&shared, active.take()).await;
                active = start(&shared, &account_id, &item_id).await;
                sent_token = active
                    .as_ref()
                    .and_then(|a| a.session.client.access_token());
            }
            PlaybackCommand::Report {
                position,
                playing,
                force_sync,
            } => {
                if let Some(active) = active.as_mut() {
                    report(&shared, active, position, playing, force_sync).await;
                    send_new_headers(&shared, active, &mut sent_token);
                }
            }
            PlaybackCommand::Stop => close(&shared, active.take()).await,
        }
    }
    close(&shared, active.take()).await;
}

async fn start(shared: &Shared, account_id: &str, item_id: &str) -> Option<ActivePlayback> {
    let Some(session) = sessions(shared, Some(account_id)).into_iter().next() else {
        (shared.emit)(Event::PlaybackFailed {
            message: "account is not connected".into(),
        });
        return None;
    };
    match playback::start(session, item_id).await {
        Ok((active, start)) => {
            (shared.emit)(Event::PlaybackStarted(Box::new(start)));
            Some(active)
        }
        Err(e) => {
            (shared.emit)(Event::PlaybackFailed {
                message: e.to_string(),
            });
            None
        }
    }
}

async fn report(
    shared: &Shared,
    active: &mut ActivePlayback,
    position: f64,
    playing: bool,
    force: bool,
) {
    let due = active.tracker.report(Instant::now(), position, playing);
    if !(due || force) {
        return;
    }
    let result = active.sync(&shared.db).await;
    (shared.emit)(Event::ProgressChanged {
        account_id: active.session.account.id.clone(),
        item_ids: vec![active.item_id.clone()],
    });
    if let Err(e) = result {
        (shared.emit)(Event::SyncFailed {
            account_id: active.session.account.id.clone(),
            message: e.to_string(),
        });
    }
}

/// Tells the app when the access token changed, so later track requests stay authorized.
fn send_new_headers(shared: &Shared, active: &ActivePlayback, sent: &mut Option<String>) {
    let token = active.session.client.access_token();
    if token.is_some() && token != *sent {
        *sent = token.clone();
        let headers = vec![format!(
            "Authorization: Bearer {}",
            token.unwrap_or_default()
        )];
        (shared.emit)(Event::PlaybackHeaders { headers });
    }
}

async fn close(shared: &Shared, active: Option<ActivePlayback>) {
    let Some(active) = active else {
        return;
    };
    let account_id = active.session.account.id.clone();
    if let Err(e) = active.close(&shared.db).await {
        (shared.emit)(Event::SyncFailed {
            account_id,
            message: e.to_string(),
        });
    }
}
