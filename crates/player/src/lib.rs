//! Audio playback, media controls and sleep timer.

pub mod controller;
pub mod media_controls;
mod mpv;
mod player;
pub mod sleep;
pub mod timeline;

pub use mpv::{MpvConfig, MpvPlayer};
pub use player::{Media, PlaybackState, Player, PlayerError, PlayerEvent};
