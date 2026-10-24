//! The engine's playback task: one item at a time, commands in order.
//!
//! A downloaded item plays from its files. With a connection it still gets a server
//! session, so listening time counts; without one only the local progress moves.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use super::Event;
use super::worker::{Shared, sessions};
use crate::account::AccountError;
use crate::playback::{self, ActivePlayback, ListenTracker, LocalPlayback, PlayStart};

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

/// The item in the player.
enum Active {
    Server {
        playback: ActivePlayback,
        local_files: bool,
    },
    Local(LocalPlayback),
}

impl Active {
    fn tracker(&mut self) -> &mut ListenTracker {
        match self {
            Self::Server { playback, .. } => &mut playback.tracker,
            Self::Local(local) => &mut local.tracker,
        }
    }

    fn ids(&self) -> (String, String) {
        match self {
            Self::Server { playback, .. } => (
                playback.session.account.id.clone(),
                playback.item_id.clone(),
            ),
            Self::Local(local) => (local.account_id.clone(), local.item_id.clone()),
        }
    }

    async fn sync(&mut self, shared: &Shared) -> Result<(), AccountError> {
        match self {
            Self::Server { playback, .. } => playback.sync(&shared.db).await,
            Self::Local(local) => local.sync(&shared.db),
        }
    }

    async fn close(self, shared: &Shared) -> Result<(), AccountError> {
        match self {
            Self::Server { playback, .. } => playback.close(&shared.db).await,
            Self::Local(mut local) => local.sync(&shared.db),
        }
    }

    /// The access token the server tracks need, if they stream.
    fn stream_token(&self) -> Option<String> {
        match self {
            Self::Server {
                playback,
                local_files: false,
            } => playback.session.client.access_token(),
            _ => None,
        }
    }
}

/// Runs until the channel closes, then closes the open session (app exit).
pub(super) async fn run(
    shared: Arc<Shared>,
    mut commands: mpsc::UnboundedReceiver<PlaybackCommand>,
) {
    let mut active: Option<Active> = None;
    let mut sent_token: Option<String> = None;
    while let Some(command) = commands.recv().await {
        match command {
            PlaybackCommand::Start {
                account_id,
                item_id,
            } => {
                close(&shared, active.take()).await;
                active = start(&shared, &account_id, &item_id).await;
                sent_token = active.as_ref().and_then(Active::stream_token);
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

async fn start(shared: &Shared, account_id: &str, item_id: &str) -> Option<Active> {
    let local = playback::local_start(&shared.db, account_id, item_id);
    let session = sessions(shared, Some(account_id)).into_iter().next();
    let result = match (session, local) {
        (Some(session), local) => start_server(session, item_id, local).await,
        (None, Some(local)) => Ok((Active::Local(LocalPlayback::new(&local)), local)),
        (None, None) => {
            (shared.emit)(Event::PlaybackFailed {
                message: "account is not connected".into(),
            });
            return None;
        }
    };
    match result {
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

/// A server session; with a download its files replace the server's tracks.
async fn start_server(
    session: Arc<crate::account::Session>,
    item_id: &str,
    local: Option<PlayStart>,
) -> Result<(Active, PlayStart), AccountError> {
    let (playback, mut start) = playback::start(session, item_id).await?;
    let local_files = local.is_some();
    if let Some(local) = local {
        start.tracks = local.tracks;
        start.http_headers = Vec::new();
        start.local = true;
    }
    Ok((
        Active::Server {
            playback,
            local_files,
        },
        start,
    ))
}

async fn report(shared: &Shared, active: &mut Active, position: f64, playing: bool, force: bool) {
    let due = active.tracker().report(Instant::now(), position, playing);
    if !(due || force) {
        return;
    }
    let result = active.sync(shared).await;
    let (account_id, item_id) = active.ids();
    (shared.emit)(Event::ProgressChanged {
        account_id: account_id.clone(),
        item_ids: vec![item_id],
    });
    if let Err(e) = result {
        (shared.emit)(Event::SyncFailed {
            account_id,
            message: e.to_string(),
        });
    }
}

/// Tells the app when the access token changed, so later track requests stay authorized.
fn send_new_headers(shared: &Shared, active: &Active, sent: &mut Option<String>) {
    let token = active.stream_token();
    if token.is_some() && token != *sent {
        *sent = token.clone();
        let headers = vec![format!(
            "Authorization: Bearer {}",
            token.unwrap_or_default()
        )];
        (shared.emit)(Event::PlaybackHeaders { headers });
    }
}

async fn close(shared: &Shared, active: Option<Active>) {
    let Some(active) = active else {
        return;
    };
    let (account_id, _) = active.ids();
    if let Err(e) = active.close(shared).await {
        (shared.emit)(Event::SyncFailed {
            account_id,
            message: e.to_string(),
        });
    }
}
