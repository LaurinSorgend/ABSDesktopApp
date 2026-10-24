//! OS media controls (SMTC on Windows, MPRIS on Linux) behind a small trait.

use std::ffi::c_void;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig, SeekDirection,
};

use crate::player::PlayerError;

/// What the OS shows for the current item.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NowPlaying {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// `https://` or `file://` URL of the cover.
    pub cover_url: Option<String>,
    pub duration: Option<Duration>,
}

/// Playback state shown by the OS.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControlsState {
    Stopped,
    Paused { position: f64 },
    Playing { position: f64 },
}

/// A request from a media key, the taskbar or the desktop shell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaKey {
    Play,
    Pause,
    Toggle,
    Stop,
    Next,
    Previous,
    SkipForward,
    SkipBack,
    /// Relative seek in seconds; negative goes back.
    SeekBy(f64),
    /// Absolute global position in seconds.
    SetPosition(f64),
}

/// OS media controls. Not `Send`: keep it on the thread that created it.
pub trait MediaControls {
    fn set_now_playing(&mut self, now_playing: &NowPlaying) -> Result<(), PlayerError>;
    fn set_state(&mut self, state: ControlsState) -> Result<(), PlayerError>;
}

/// The platform controls via souvlaki. On Windows `hwnd` must be the main window handle.
pub fn system_media_controls(
    hwnd: Option<*mut c_void>,
    on_key: impl Fn(MediaKey) + Send + 'static,
) -> Result<Box<dyn MediaControls>, PlayerError> {
    let config = PlatformConfig {
        display_name: "ABS Desktop",
        dbus_name: "abs_desktop",
        hwnd,
    };
    let mut controls = souvlaki::MediaControls::new(config).map_err(platform)?;
    controls
        .attach(move |event| {
            if let Some(key) = key_for(event) {
                on_key(key);
            }
        })
        .map_err(platform)?;
    Ok(Box::new(Souvlaki(controls)))
}

struct Souvlaki(souvlaki::MediaControls);

impl MediaControls for Souvlaki {
    fn set_now_playing(&mut self, now: &NowPlaying) -> Result<(), PlayerError> {
        let metadata = MediaMetadata {
            title: Some(&now.title),
            artist: now.artist.as_deref(),
            album: now.album.as_deref(),
            cover_url: now.cover_url.as_deref(),
            duration: now.duration,
        };
        self.0.set_metadata(metadata).map_err(platform)
    }

    fn set_state(&mut self, state: ControlsState) -> Result<(), PlayerError> {
        let progress = |secs: f64| Some(MediaPosition(Duration::from_secs_f64(secs.max(0.0))));
        let playback = match state {
            ControlsState::Stopped => MediaPlayback::Stopped,
            ControlsState::Paused { position } => MediaPlayback::Paused {
                progress: progress(position),
            },
            ControlsState::Playing { position } => MediaPlayback::Playing {
                progress: progress(position),
            },
        };
        self.0.set_playback(playback).map_err(platform)
    }
}

fn key_for(event: MediaControlEvent) -> Option<MediaKey> {
    let signed = |direction: SeekDirection, secs: f64| match direction {
        SeekDirection::Forward => secs,
        SeekDirection::Backward => -secs,
    };
    Some(match event {
        MediaControlEvent::Play => MediaKey::Play,
        MediaControlEvent::Pause => MediaKey::Pause,
        MediaControlEvent::Toggle => MediaKey::Toggle,
        MediaControlEvent::Stop => MediaKey::Stop,
        MediaControlEvent::Next => MediaKey::Next,
        MediaControlEvent::Previous => MediaKey::Previous,
        MediaControlEvent::Seek(SeekDirection::Forward) => MediaKey::SkipForward,
        MediaControlEvent::Seek(SeekDirection::Backward) => MediaKey::SkipBack,
        MediaControlEvent::SeekBy(direction, by) => {
            MediaKey::SeekBy(signed(direction, by.as_secs_f64()))
        }
        MediaControlEvent::SetPosition(MediaPosition(at)) => {
            MediaKey::SetPosition(at.as_secs_f64())
        }
        _ => return None,
    })
}

fn platform(err: souvlaki::Error) -> PlayerError {
    PlayerError::Backend(format!("media controls: {err:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_os_events() {
        assert_eq!(key_for(MediaControlEvent::Toggle), Some(MediaKey::Toggle));
        let back = MediaControlEvent::SeekBy(SeekDirection::Backward, Duration::from_secs(15));
        assert_eq!(key_for(back), Some(MediaKey::SeekBy(-15.0)));
        assert_eq!(key_for(MediaControlEvent::Raise), None);
    }
}
