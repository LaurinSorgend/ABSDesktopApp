//! Server playback sessions: start, periodic sync, restart after a server restart, close.

mod local;
mod tracker;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use abs_api::models::{Chapter, DeviceInfo, PlayRequest, PlaybackSession, SessionSync};
use abs_api::reqwest::StatusCode;

use crate::account::{AccountError, Session, lock};
use crate::db::{Db, ProgressRow, now_ms};
pub use local::{LocalPlayback, local_start};
pub use tracker::{ListenTracker, SYNC_INTERVAL};

/// Audio types libmpv plays; sent so the server can choose direct play.
pub const SUPPORTED_MIME_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4b",
    "audio/aac",
    "audio/x-aac",
    "audio/flac",
    "audio/x-flac",
    "audio/ogg",
    "audio/opus",
    "audio/webm",
    "audio/wav",
    "audio/x-wav",
    "audio/x-ms-wma",
    "audio/aiff",
    "audio/x-aiff",
];

/// One track to hand to the player.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackSource {
    pub url: String,
    pub start_offset: f64,
    pub duration: f64,
}

/// What the app needs to start the player.
#[derive(Debug, Clone)]
pub struct PlayStart {
    pub account_id: String,
    pub item_id: String,
    pub library_id: String,
    pub title: String,
    pub author: Option<String>,
    pub tracks: Vec<TrackSource>,
    pub chapters: Vec<Chapter>,
    pub start_time: f64,
    pub duration: f64,
    /// HTTP headers for the track URLs.
    pub http_headers: Vec<String>,
    /// True when the tracks are downloaded files.
    pub local: bool,
}

/// A running server session for the item in the player.
pub struct ActivePlayback {
    pub session: Arc<Session>,
    pub item_id: String,
    pub server_session_id: String,
    pub tracker: ListenTracker,
}

/// Starts a session for `item_id` and returns it with the data for the player.
pub async fn start(
    session: Arc<Session>,
    item_id: &str,
) -> Result<(ActivePlayback, PlayStart), AccountError> {
    let server = session
        .client
        .play(item_id, &play_request(&session))
        .await?;
    let start = play_start(&session, item_id, &server)?;
    let active = ActivePlayback {
        session,
        item_id: item_id.to_owned(),
        server_session_id: server.id,
        tracker: ListenTracker::new(Instant::now(), server.current_time, server.duration),
    };
    Ok((active, start))
}

impl ActivePlayback {
    /// Sends listened time and position. A 404 means the server forgot the session
    /// (it keeps them in memory); a new one is started and the sync repeated.
    pub async fn sync(&mut self, db: &Mutex<Db>) -> Result<(), AccountError> {
        let sync = self.tracker.take_sync(Instant::now());
        let result = self.send(&sync).await;
        if result.is_err() {
            self.tracker.restore(&sync);
        }
        store_local(db, &self.session.account.id, &self.item_id, &sync)?;
        result
    }

    /// Final sync and close, on stop, item change and app exit. On failure the
    /// unsent listening time stays in the tracker.
    pub async fn close(&mut self, db: &Mutex<Db>) -> Result<(), AccountError> {
        let sync = self.tracker.take_sync(Instant::now());
        store_local(db, &self.session.account.id, &self.item_id, &sync)?;
        let result = self.send_close(&sync).await;
        if result.is_err() {
            self.tracker.restore(&sync);
        }
        result
    }

    async fn send_close(&mut self, sync: &SessionSync) -> Result<(), AccountError> {
        let session = self.session.clone();
        let client = &session.client;
        match client
            .close_session(&self.server_session_id, Some(sync))
            .await
        {
            Err(e) if e.status() == Some(StatusCode::NOT_FOUND) => {
                self.restart().await?;
                client
                    .close_session(&self.server_session_id, Some(sync))
                    .await?;
                Ok(())
            }
            result => result.map_err(Into::into),
        }
    }

    async fn send(&mut self, sync: &SessionSync) -> Result<(), AccountError> {
        let session = self.session.clone();
        let client = &session.client;
        match client.sync_session(&self.server_session_id, sync).await {
            Err(e) if e.status() == Some(StatusCode::NOT_FOUND) => {
                self.restart().await?;
                client.sync_session(&self.server_session_id, sync).await?;
                Ok(())
            }
            result => result.map_err(Into::into),
        }
    }

    async fn restart(&mut self) -> Result<(), AccountError> {
        let request = play_request(&self.session);
        let server = self.session.client.play(&self.item_id, &request).await?;
        self.server_session_id = server.id;
        Ok(())
    }
}

/// How this app names itself to the server.
pub(crate) fn device_info(session: &Session) -> DeviceInfo {
    DeviceInfo {
        device_id: session.account.device_id.clone(),
        client_name: "ABS Desktop".into(),
        client_version: env!("CARGO_PKG_VERSION").into(),
        manufacturer: None,
        model: Some(std::env::consts::OS.into()),
    }
}

fn play_request(session: &Session) -> PlayRequest {
    PlayRequest {
        device_info: device_info(session),
        media_player: "libmpv".into(),
        supported_mime_types: SUPPORTED_MIME_TYPES
            .iter()
            .map(|m| (*m).to_owned())
            .collect(),
        force_direct_play: false,
        force_transcode: false,
    }
}

fn play_start(
    session: &Session,
    item_id: &str,
    server: &PlaybackSession,
) -> Result<PlayStart, AccountError> {
    let client = &session.client;
    let tracks = server
        .audio_tracks
        .iter()
        .map(|t| {
            Ok(TrackSource {
                url: client.resolve(&t.content_url)?.to_string(),
                start_offset: t.start_offset,
                duration: t.duration,
            })
        })
        .collect::<Result<_, abs_api::Error>>()?;
    let token = client.access_token().ok_or(AccountError::LoginRequired)?;
    Ok(PlayStart {
        account_id: session.account.id.clone(),
        item_id: item_id.to_owned(),
        library_id: server.library_id.clone(),
        title: server.display_title.clone().unwrap_or_default(),
        author: server.display_author.clone(),
        tracks,
        chapters: server.chapters.clone(),
        start_time: server.current_time,
        duration: server.duration,
        http_headers: vec![format!("Authorization: Bearer {token}")],
        local: false,
    })
}

/// Keeps local progress current so the UI does not wait for the server.
fn store_local(
    db: &Mutex<Db>,
    account_id: &str,
    item_id: &str,
    sync: &SessionSync,
) -> Result<(), AccountError> {
    let row = ProgressRow {
        item_id: item_id.to_owned(),
        current_time: sync.current_time,
        progress: if sync.duration > 0.0 {
            sync.current_time / sync.duration
        } else {
            0.0
        },
        is_finished: false,
        ebook_location: None,
        ebook_progress: None,
        updated_at: now_ms(),
    };
    lock(db).store().progress().upsert(account_id, &row)?;
    Ok(())
}
