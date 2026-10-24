//! Audio playback, media controls and sleep timer.

pub mod controller;
mod mpv;
mod player;
pub mod sleep;
pub mod timeline;

pub use mpv::{MpvConfig, MpvPlayer};
pub use player::{Media, PlaybackState, Player, PlayerError, PlayerEvent};
