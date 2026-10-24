use crate::timeline::Track;

/// Player errors.
#[derive(Debug, thiserror::Error)]
pub enum PlayerError {
    #[error("audio backend: {0}")]
    Backend(String),
    #[error("nothing loaded")]
    NothingLoaded,
}

/// What to play: an item's tracks, plus HTTP headers such as `Authorization`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Media {
    pub tracks: Vec<Track>,
    pub http_headers: Vec<String>,
}

/// Coarse playback state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Playing,
    Paused,
    /// Waiting for network data.
    Buffering,
}

/// Notifications from a player.
#[derive(Debug, Clone, PartialEq)]
pub enum PlayerEvent {
    /// Global item position in seconds, at most a few times per second.
    Position(f64),
    State(PlaybackState),
    /// A track started; `index` into [`Media::tracks`].
    TrackChanged(usize),
    /// The last track played to its end.
    Ended,
    /// A track could not be played.
    Error(String),
}

/// An audio backend. Positions are global item time in seconds.
pub trait Player: Send + Sync {
    /// Replaces the current media and starts at `start`, paused unless `play`.
    fn load(&self, media: Media, start: f64, play: bool) -> Result<(), PlayerError>;
    fn play(&self) -> Result<(), PlayerError>;
    fn pause(&self) -> Result<(), PlayerError>;
    fn seek(&self, position: f64) -> Result<(), PlayerError>;
    fn set_speed(&self, speed: f64) -> Result<(), PlayerError>;
    /// Volume in percent, 0 to 100.
    fn set_volume(&self, percent: f64) -> Result<(), PlayerError>;
    /// Headers for the next requests, such as a refreshed `Authorization` header.
    fn set_http_headers(&self, headers: &[String]) -> Result<(), PlayerError>;
    /// Unloads the media.
    fn stop(&self) -> Result<(), PlayerError>;
    /// Last known global position.
    fn position(&self) -> f64;
    fn is_paused(&self) -> bool;
}
