use serde::{Deserialize, Serialize};

use super::item::{AudioTrack, Chapter, LibraryItem};

/// Body of `POST /api/items/:id/play`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub device_info: DeviceInfo,
    pub media_player: String,
    pub supported_mime_types: Vec<String>,
    pub force_direct_play: bool,
    pub force_transcode: bool,
}

/// Identifies this client to the server.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub device_id: String,
    pub client_name: String,
    pub client_version: String,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
}

/// Body of `POST /api/session/:id/sync` and `/close`.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSync {
    pub current_time: f64,
    pub time_listened: f64,
    pub duration: f64,
}

/// A playback session started by `/play`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlaybackSession {
    pub id: String,
    pub user_id: String,
    pub library_id: String,
    pub library_item_id: String,
    pub episode_id: Option<String>,
    pub media_type: String,
    pub display_title: Option<String>,
    pub display_author: Option<String>,
    pub cover_path: Option<String>,
    pub duration: f64,
    /// 0 direct play, 1 direct stream, 2 transcode (HLS), 3 local.
    pub play_method: i64,
    pub media_player: Option<String>,
    pub start_time: f64,
    pub current_time: f64,
    pub time_listening: f64,
    pub chapters: Vec<Chapter>,
    pub audio_tracks: Vec<AudioTrack>,
    pub library_item: Option<LibraryItem>,
}

/// A session listened to without a server connection, for `POST /api/session/local-all`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSession {
    /// `play_local_<uuid>`; the server swaps it for its own id.
    pub id: String,
    pub library_id: String,
    pub library_item_id: String,
    pub media_type: String,
    pub display_title: String,
    pub display_author: String,
    pub duration: f64,
    /// Always 3 (local).
    pub play_method: i64,
    pub media_player: String,
    pub start_time: f64,
    pub current_time: f64,
    pub time_listening: f64,
    /// Milliseconds since the epoch on the server's clock.
    pub started_at: i64,
    pub updated_at: i64,
}

/// Body of `POST /api/session/local-all`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalSessions {
    pub device_info: DeviceInfo,
    pub sessions: Vec<LocalSession>,
}

/// The server's answer for one local session, in the order they were sent.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LocalSyncResult {
    /// The server's id for the session, not the `play_local_` one.
    pub id: String,
    pub success: bool,
    /// False when the server kept a newer position from another device.
    pub progress_synced: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub(crate) struct LocalSyncResults {
    pub results: Vec<LocalSyncResult>,
}
