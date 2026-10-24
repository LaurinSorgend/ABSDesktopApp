//! The thread that reads mpv events and turns them into [`PlayerEvent`]s.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libmpv2::events::{Event, PropertyData};
use libmpv2::{Format, Mpv};

use super::{State, backend, lock};
use crate::player::{PlaybackState, PlayerError, PlayerEvent};
use crate::timeline::global_time;

const POSITION_INTERVAL: Duration = Duration::from_millis(250);
const END_FILE_EOF: u32 = 0;
const END_FILE_ERROR: u32 = 4;
const PROPERTIES: &[(&str, Format)] = &[
    ("time-pos", Format::Double),
    ("playlist-pos", Format::Int64),
    ("pause", Format::Flag),
    ("paused-for-cache", Format::Flag),
];

type Emit = Box<dyn Fn(PlayerEvent) + Send>;

pub(super) fn spawn(
    mpv: Arc<Mpv>,
    state: Arc<Mutex<State>>,
    emit: Emit,
) -> Result<JoinHandle<()>, PlayerError> {
    for (id, (name, format)) in PROPERTIES.iter().enumerate() {
        mpv.observe_property(name, *format, id as u64)
            .map_err(backend)?;
    }
    let thread = std::thread::Builder::new()
        .name("mpv-events".into())
        .spawn(move || Reader::new(mpv, state, emit).run())
        .map_err(|e| PlayerError::Backend(e.to_string()))?;
    Ok(thread)
}

struct Reader {
    mpv: Arc<Mpv>,
    state: Arc<Mutex<State>>,
    emit: Emit,
    last_position: Instant,
}

impl Reader {
    fn new(mpv: Arc<Mpv>, state: Arc<Mutex<State>>, emit: Emit) -> Self {
        Self {
            mpv,
            state,
            emit,
            last_position: Instant::now() - POSITION_INTERVAL,
        }
    }

    fn run(mut self) {
        loop {
            let mpv = self.mpv.clone();
            match mpv.wait_event(1.0) {
                Some(Ok(Event::Shutdown)) => return,
                Some(Ok(Event::FileLoaded)) => self.apply_pending_seek(),
                Some(Ok(Event::EndFile(reason))) => self.on_end_file(reason),
                Some(Ok(Event::PropertyChange { name, change, .. })) => {
                    self.on_property(name, change)
                }
                _ => {}
            }
        }
    }

    fn on_property(&mut self, name: &str, change: PropertyData<'_>) {
        match (name, change) {
            ("time-pos", PropertyData::Double(offset)) => self.on_time(offset),
            ("playlist-pos", PropertyData::Int64(pos)) => {
                let index = usize::try_from(pos).ok();
                lock(&self.state).current = index;
                if let Some(index) = index {
                    (self.emit)(PlayerEvent::TrackChanged(index));
                }
            }
            ("pause", PropertyData::Flag(paused)) => {
                lock(&self.state).paused = paused;
                let state = if paused {
                    PlaybackState::Paused
                } else {
                    PlaybackState::Playing
                };
                (self.emit)(PlayerEvent::State(state));
            }
            ("paused-for-cache", PropertyData::Flag(waiting)) => {
                let paused = lock(&self.state).paused;
                let state = match (waiting, paused) {
                    (true, _) => PlaybackState::Buffering,
                    (false, true) => PlaybackState::Paused,
                    (false, false) => PlaybackState::Playing,
                };
                (self.emit)(PlayerEvent::State(state));
            }
            _ => {}
        }
    }

    /// Updates the global position, unless a cross-track seek is still pending.
    fn on_time(&mut self, offset: f64) {
        let position = {
            let mut state = lock(&self.state);
            let Some(current) = state.current else {
                return;
            };
            if state.pending_seek.is_some() {
                return;
            }
            state.position = global_time(&state.tracks, current, offset);
            state.position
        };
        if self.last_position.elapsed() >= POSITION_INTERVAL {
            self.last_position = Instant::now();
            (self.emit)(PlayerEvent::Position(position));
        }
    }

    /// Runs on `FileLoaded`. Reads `playlist-pos` itself because its change
    /// notification may arrive after this event. Older libmpv versions start the
    /// first entry when leaving idle; then the wanted track is selected again.
    fn apply_pending_seek(&mut self) {
        let loaded = self.mpv.get_property::<i64>("playlist-pos").ok();
        let (index, offset, here) = {
            let mut state = lock(&self.state);
            if let Some(index) = loaded.and_then(|p| usize::try_from(p).ok()) {
                state.current = Some(index);
            }
            let Some((index, offset)) = state.pending_seek else {
                return;
            };
            let here = state.current == Some(index);
            if here {
                state.pending_seek = None;
            }
            (index, offset, here)
        };
        let result = if here {
            self.mpv.command("seek", &[&offset.to_string(), "absolute"])
        } else {
            self.mpv.set_property("playlist-pos", index as i64)
        };
        if let Err(e) = result {
            (self.emit)(PlayerEvent::Error(e.to_string()));
        }
    }

    fn on_end_file(&mut self, reason: u32) {
        let last_track = {
            let state = lock(&self.state);
            state.current.is_some() && state.current == state.tracks.len().checked_sub(1)
        };
        match reason {
            END_FILE_ERROR => (self.emit)(PlayerEvent::Error("track could not be played".into())),
            END_FILE_EOF if last_track => (self.emit)(PlayerEvent::Ended),
            _ => {}
        }
    }
}
