//! The engine's playback task: one item at a time, commands in order.
//!
//! A downloaded item plays from its files. With a connection it still gets a server
//! session, so listening time counts. Without one, or when the server goes away
//! while playing, listening is recorded as a local session for later.

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use super::worker::{Shared, lock_map, sessions};
use super::{Event, offline};
use crate::account::{AccountError, Session};
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

/// The item in the player and how its listening is recorded.
struct Active {
    start: PlayStart,
    mode: Mode,
}

enum Mode {
    Server(ActivePlayback),
    Local(LocalPlayback),
}

impl Active {
    fn tracker(&mut self) -> &mut ListenTracker {
        match &mut self.mode {
            Mode::Server(playback) => &mut playback.tracker,
            Mode::Local(local) => &mut local.tracker,
        }
    }

    /// The access token the tracks need while they stream from the server.
    fn stream_token(&self) -> Option<String> {
        match &self.mode {
            Mode::Server(playback) if !self.start.local => playback.session.client.access_token(),
            _ => None,
        }
    }

    /// Syncs; when the server is gone the listening moves to a local session.
    async fn sync(&mut self, shared: &Shared) -> Result<(), AccountError> {
        let result = match &mut self.mode {
            Mode::Server(playback) => playback.sync(&shared.db).await,
            Mode::Local(local) => return local.sync(&shared.db),
        };
        self.fall_back(shared, result)
    }

    async fn close(mut self, shared: &Shared) -> Result<(), AccountError> {
        let result = match &mut self.mode {
            Mode::Server(playback) => playback.close(&shared.db).await,
            Mode::Local(local) => local.sync(&shared.db),
        };
        let result = self.fall_back(shared, result);
        set_running_local(shared, None);
        result
    }

    fn fall_back(
        &mut self,
        shared: &Shared,
        result: Result<(), AccountError>,
    ) -> Result<(), AccountError> {
        let Err(e) = &result else {
            return result;
        };
        let Mode::Server(playback) = &self.mode else {
            return result;
        };
        if !e.is_network() {
            return result;
        }
        super::worker::went_offline(shared, &self.start.account_id, e);
        let mut local = LocalPlayback::resume(&self.start, playback.tracker.clone());
        set_running_local(shared, Some(local.session_id()));
        let stored = local.sync(&shared.db);
        self.mode = Mode::Local(local);
        stored
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
        (None, Some(local)) => Ok(start_local(shared, local)),
        (None, None) => {
            (shared.emit)(Event::PlaybackFailed {
                message: "account is not connected".into(),
            });
            return None;
        }
    };
    match result {
        Ok(active) => {
            (shared.emit)(Event::PlaybackStarted(Box::new(active.start.clone())));
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

fn start_local(shared: &Shared, start: PlayStart) -> Active {
    let local = LocalPlayback::new(&start);
    set_running_local(shared, Some(local.session_id()));
    Active {
        start,
        mode: Mode::Local(local),
    }
}

/// A server session; with a download its files replace the server's tracks.
async fn start_server(
    session: Arc<Session>,
    item_id: &str,
    local: Option<PlayStart>,
) -> Result<Active, AccountError> {
    let (playback, mut start) = playback::start(session, item_id).await?;
    if let Some(local) = local {
        start.tracks = local.tracks;
        start.http_headers = Vec::new();
        start.local = true;
    }
    Ok(Active {
        start,
        mode: Mode::Server(playback),
    })
}

async fn report(shared: &Shared, active: &mut Active, position: f64, playing: bool, force: bool) {
    let due = active.tracker().report(Instant::now(), position, playing);
    if !(due || force) {
        return;
    }
    let result = active.sync(shared).await;
    let account_id = active.start.account_id.clone();
    (shared.emit)(Event::ProgressChanged {
        account_id: account_id.clone(),
        item_ids: vec![active.start.item_id.clone()],
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

/// Closes the item; a local session it leaves behind is sent if the server is there.
async fn close(shared: &Arc<Shared>, active: Option<Active>) {
    let Some(active) = active else {
        return;
    };
    let account_id = active.start.account_id.clone();
    if let Err(e) = active.close(shared).await {
        (shared.emit)(Event::SyncFailed {
            account_id: account_id.clone(),
            message: e.to_string(),
        });
    }
    offline::spawn_flush(shared, &account_id);
}

fn set_running_local(shared: &Shared, id: Option<&str>) {
    *lock_map(&shared.running_local) = id.map(str::to_owned);
}
