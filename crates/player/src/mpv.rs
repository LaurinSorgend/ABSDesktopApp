//! [`Player`] on libmpv: one mpv playlist entry per track, global time mapped per track.

mod events;

use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use libmpv2::Mpv;

use crate::player::{Media, Player, PlayerError, PlayerEvent};
use crate::timeline::{Track, global_time, locate};

/// Options that turn off what an audio-only embedded player does not need.
const LEAN_OPTIONS: &[&str] = &[
    "load-scripts",
    "ytdl",
    "osc",
    "input-default-bindings",
    "load-stats-overlay",
    "load-console",
    "load-auto-profiles",
    "load-select",
    "load-positioning",
    "load-commands",
    "media-controls",
];

/// Settings for [`MpvPlayer::new`].
#[derive(Debug, Clone)]
pub struct MpvConfig {
    /// mpv `ao`; `None` keeps mpv's default, `Some("null")` plays silently.
    pub audio_output: Option<String>,
    /// mpv `demuxer-max-bytes`; caps memory for long single-file books.
    pub demuxer_max_bytes: String,
    pub demuxer_max_back_bytes: String,
}

impl Default for MpvConfig {
    fn default() -> Self {
        Self {
            audio_output: None,
            demuxer_max_bytes: "8MiB".into(),
            demuxer_max_back_bytes: "2MiB".into(),
        }
    }
}

/// Playback state shared with the event thread.
#[derive(Debug, Default)]
pub(crate) struct State {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    /// A seek into another track, applied once that track has loaded.
    pub pending_seek: Option<(usize, f64)>,
    pub position: f64,
    pub paused: bool,
}

/// libmpv backend. Dropping it shuts mpv down.
pub struct MpvPlayer {
    mpv: Arc<Mpv>,
    state: Arc<Mutex<State>>,
    events: Option<JoinHandle<()>>,
}

impl MpvPlayer {
    /// Creates mpv and its event thread. `on_event` runs on that thread.
    pub fn new(
        config: &MpvConfig,
        on_event: impl Fn(PlayerEvent) + Send + 'static,
    ) -> Result<Self, PlayerError> {
        let mpv = Arc::new(create(config).map_err(backend)?);
        let state = Arc::new(Mutex::new(State {
            paused: true,
            ..State::default()
        }));
        let events = events::spawn(mpv.clone(), state.clone(), Box::new(on_event))?;
        Ok(Self {
            mpv,
            state,
            events: Some(events),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        lock(&self.state)
    }

    fn command(&self, name: &str, args: &[&str]) -> Result<(), PlayerError> {
        self.mpv.command(name, args).map_err(backend)
    }
}

impl Player for MpvPlayer {
    fn load(&self, media: Media, start: f64, play: bool) -> Result<(), PlayerError> {
        let (index, offset) = locate(&media.tracks, start);
        self.set_http_headers(&media.http_headers)?;
        self.mpv.set_property("pause", !play).map_err(backend)?;
        {
            let mut state = self.state();
            state.position = global_time(&media.tracks, index, offset);
            state.pending_seek = (offset > 0.0).then_some((index, offset));
            state.current = None;
            state.tracks = media.tracks.clone();
        }
        self.command("playlist-clear", &[])?;
        for (i, track) in media.tracks.iter().enumerate() {
            let mode = if i == 0 { "replace" } else { "append" };
            self.command("loadfile", &[&track.url, mode])?;
        }
        if index > 0 {
            self.mpv
                .set_property("playlist-pos", index as i64)
                .map_err(backend)?;
        }
        Ok(())
    }

    fn play(&self) -> Result<(), PlayerError> {
        self.mpv.set_property("pause", false).map_err(backend)
    }

    fn pause(&self) -> Result<(), PlayerError> {
        self.mpv.set_property("pause", true).map_err(backend)
    }

    fn seek(&self, position: f64) -> Result<(), PlayerError> {
        let (index, offset, current) = {
            let mut state = self.state();
            if state.tracks.is_empty() {
                return Err(PlayerError::NothingLoaded);
            }
            let (index, offset) = locate(&state.tracks, position);
            state.position = global_time(&state.tracks, index, offset);
            if state.current != Some(index) {
                state.pending_seek = Some((index, offset));
            }
            (index, offset, state.current)
        };
        if current == Some(index) {
            return self.command("seek", &[&offset.to_string(), "absolute"]);
        }
        self.mpv
            .set_property("playlist-pos", index as i64)
            .map_err(backend)
    }

    fn set_speed(&self, speed: f64) -> Result<(), PlayerError> {
        self.mpv
            .set_property("speed", speed.clamp(0.25, 4.0))
            .map_err(backend)
    }

    fn set_volume(&self, percent: f64) -> Result<(), PlayerError> {
        self.mpv
            .set_property("volume", percent.clamp(0.0, 100.0))
            .map_err(backend)
    }

    fn set_http_headers(&self, headers: &[String]) -> Result<(), PlayerError> {
        self.mpv
            .set_property("http-header-fields", headers.join(",").as_str())
            .map_err(backend)
    }

    fn stop(&self) -> Result<(), PlayerError> {
        *self.state() = State {
            paused: true,
            ..State::default()
        };
        self.command("stop", &[])
    }

    fn position(&self) -> f64 {
        self.state().position
    }

    fn is_paused(&self) -> bool {
        self.state().paused
    }
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        let _ = self.mpv.command("quit", &[]);
        if let Some(events) = self.events.take() {
            let _ = events.join();
        }
    }
}

fn create(config: &MpvConfig) -> libmpv2::Result<Mpv> {
    Mpv::with_initializer(|init| {
        init.set_option("video", "no")?;
        init.set_option("idle", "yes")?;
        init.set_option("cache", "yes")?;
        init.set_option("demuxer-max-bytes", config.demuxer_max_bytes.as_str())?;
        init.set_option(
            "demuxer-max-back-bytes",
            config.demuxer_max_back_bytes.as_str(),
        )?;
        init.set_option("audio-pitch-correction", "yes")?;
        init.set_option("keep-open", "no")?;
        if let Some(ao) = &config.audio_output {
            init.set_option("ao", ao.as_str())?;
        }
        for option in LEAN_OPTIONS {
            init.set_option(option, "no")?;
        }
        Ok(())
    })
}

pub(crate) fn lock(state: &Mutex<State>) -> MutexGuard<'_, State> {
    state.lock().unwrap_or_else(|p| p.into_inner())
}

pub(crate) fn backend(err: libmpv2::Error) -> PlayerError {
    PlayerError::Backend(err.to_string())
}
