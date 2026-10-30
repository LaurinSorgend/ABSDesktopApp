//! The loaded item: libmpv, the controller, OS media controls and reports to the engine.

pub mod view;

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use abs_core::engine::{Command, Engine};
use abs_core::playback::PlayStart;
use abs_player::controller::Controller;
use abs_player::media_controls::{ControlsState, MediaControls, MediaKey, NowPlaying};
use abs_player::timeline::{self, Chapter, Track};
use abs_player::{Media, MpvConfig, MpvPlayer, PlaybackState, Player, PlayerError, PlayerEvent};

use crate::settings::{Choices, SPEEDS, sleep_choice};

const REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// What is loaded in the player.
pub struct NowItem {
    pub account_id: String,
    pub item_id: String,
    pub title: String,
    pub author: String,
    pub duration: f64,
    pub chapters: Vec<Chapter>,
}

pub struct Playback {
    engine: Rc<Engine>,
    controller: Controller,
    controls: Option<Box<dyn MediaControls>>,
    now: Option<NowItem>,
    /// Item requested from the engine, with an optional start position.
    loading: Option<(String, Option<f64>)>,
    playing: bool,
    position: f64,
    last_report: Instant,
    chapter: Option<usize>,
    speed_index: usize,
    sleep_index: usize,
    volume: f64,
    muted: bool,
}

impl Playback {
    /// Starts libmpv. `on_event` runs on mpv's event thread.
    pub fn new(
        engine: Rc<Engine>,
        on_event: impl Fn(PlayerEvent) + Send + 'static,
    ) -> Result<Self, PlayerError> {
        let player: Arc<dyn Player> = Arc::new(MpvPlayer::new(&MpvConfig::default(), on_event)?);
        Ok(Self {
            engine,
            controller: Controller::new(player),
            controls: None,
            now: None,
            loading: None,
            playing: false,
            position: 0.0,
            last_report: Instant::now(),
            chapter: None,
            speed_index: 1,
            sleep_index: 0,
            volume: 100.0,
            muted: false,
        })
    }

    pub fn now(&self) -> Option<&NowItem> {
        self.now.as_ref()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    pub fn set_media_controls(&mut self, controls: Box<dyn MediaControls>) {
        self.controls = Some(controls);
        self.update_controls_metadata();
        self.update_controls_state();
    }

    /// Applies skip intervals and, for the next item, the default speed.
    pub fn apply_choices(&mut self, choices: &Choices) {
        self.controller.skip_back = choices.skip_back();
        self.controller.skip_forward = choices.skip_forward();
        if self.now.is_none() {
            self.speed_index = choices.default_speed;
        }
        self.set_volume(choices.volume);
    }

    pub fn volume(&self) -> f64 {
        self.volume
    }

    /// Plays an item, from `start` or where the server says it was left.
    pub fn request(&mut self, account_id: &str, item_id: &str, start: Option<f64>) {
        if self.now.as_ref().is_some_and(|n| n.item_id == item_id) {
            if let Some(start) = start {
                self.seek_to(start);
            }
            let _ = self.controller.player().play();
            return;
        }
        self.loading = Some((item_id.to_owned(), start));
        self.engine.send(Command::StartPlayback {
            account_id: account_id.to_owned(),
            item_id: item_id.to_owned(),
        });
    }

    /// Loads the tracks of a session the engine started.
    pub fn started(&mut self, start: PlayStart) -> Result<(), PlayerError> {
        let requested_start = match self.loading.take() {
            Some((id, at)) if id == start.item_id => at,
            _ => None,
        };
        let chapters: Vec<Chapter> = start
            .chapters
            .iter()
            .map(|c| Chapter {
                start: c.start,
                end: c.end,
                title: c.title.clone(),
            })
            .collect();
        let media = Media {
            tracks: start.tracks.iter().map(track).collect(),
            http_headers: start.http_headers.clone(),
        };
        let at = requested_start.unwrap_or(start.start_time);
        let player = self.controller.player().clone();
        player.load(media, at, true)?;
        player.set_speed(SPEEDS[self.speed_index])?;
        self.apply_volume();
        self.controller.set_item(chapters.clone(), start.duration);
        self.position = at;
        self.chapter = timeline::chapter_at(&chapters, at);
        self.now = Some(NowItem {
            account_id: start.account_id,
            item_id: start.item_id,
            title: start.title,
            author: start.author.unwrap_or_default(),
            duration: start.duration,
            chapters,
        });
        self.update_controls_metadata();
        Ok(())
    }

    pub fn start_failed(&mut self) {
        self.loading = None;
    }

    pub fn set_headers(&self, headers: &[String]) {
        let _ = self.controller.player().set_http_headers(headers);
    }

    /// Handles a player event. Returns an error message to show, if any.
    pub fn player_event(&mut self, event: PlayerEvent) -> Option<String> {
        match event {
            PlayerEvent::Position(position) => {
                self.position = position;
                let chapter = timeline::chapter_at(self.chapters(), position);
                let force = chapter != self.chapter;
                self.chapter = chapter;
                if force || self.last_report.elapsed() >= REPORT_INTERVAL {
                    self.report(force);
                }
            }
            PlayerEvent::State(state) => {
                let playing = state == PlaybackState::Playing || state == PlaybackState::Buffering;
                if playing != self.playing {
                    self.playing = playing;
                    self.report(false);
                    self.update_controls_state();
                }
            }
            PlayerEvent::TrackChanged(_) => {}
            PlayerEvent::Ended => {
                self.playing = false;
                self.report(true);
                self.update_controls_state();
            }
            PlayerEvent::Error(message) => return Some(message),
        }
        None
    }

    /// Runs the sleep timer; call about once a second.
    pub fn tick(&mut self) {
        if let Ok(true) = self.controller.tick(Instant::now()) {
            self.sleep_index = 0;
            self.report(true);
        }
    }

    /// Sends the position to the engine, which syncs the server session when due.
    pub fn report(&mut self, force_sync: bool) {
        let Some(now) = &self.now else {
            return;
        };
        self.last_report = Instant::now();
        self.engine.send(Command::ReportPlayback {
            item_id: now.item_id.clone(),
            position: self.position,
            playing: self.playing,
            force_sync,
        });
    }

    /// Last report and unload, before the app quits.
    pub fn shutdown(&mut self) {
        self.position = self.controller.player().position().max(self.position);
        self.playing = false;
        self.report(true);
        let _ = self.controller.player().stop();
    }

    fn chapters(&self) -> &[Chapter] {
        self.now.as_ref().map_or(&[], |n| n.chapters.as_slice())
    }

    fn update_controls_metadata(&mut self) {
        let (Some(controls), Some(now)) = (self.controls.as_mut(), self.now.as_ref()) else {
            return;
        };
        let metadata = NowPlaying {
            title: now.title.clone(),
            artist: Some(now.author.clone()).filter(|a| !a.is_empty()),
            album: None,
            cover_url: None,
            duration: Some(Duration::from_secs_f64(now.duration.max(0.0))),
        };
        let _ = controls.set_now_playing(&metadata);
    }

    fn update_controls_state(&mut self) {
        let Some(controls) = self.controls.as_mut() else {
            return;
        };
        let position = self.position;
        let state = match (&self.now, self.playing) {
            (None, _) => ControlsState::Stopped,
            (Some(_), true) => ControlsState::Playing { position },
            (Some(_), false) => ControlsState::Paused { position },
        };
        let _ = controls.set_state(state);
    }
}

/// Transport and settings actions from the UI, keyboard and media keys.
impl Playback {
    pub fn toggle(&self) {
        let _ = self.controller.toggle();
    }

    pub fn skip_back(&mut self) {
        let _ = self.controller.skip_back();
        self.after_seek();
    }

    pub fn skip_forward(&mut self) {
        let _ = self.controller.skip_forward();
        self.after_seek();
    }

    pub fn previous_chapter(&mut self) {
        let _ = self.controller.previous_chapter();
        self.after_seek();
    }

    pub fn next_chapter(&mut self) {
        let _ = self.controller.next_chapter();
        self.after_seek();
    }

    pub fn seek_to(&mut self, position: f64) {
        let _ = self.controller.seek(position);
        self.after_seek();
    }

    pub fn seek_fraction(&mut self, fraction: f64) {
        let duration = self.now.as_ref().map_or(0.0, |n| n.duration);
        self.seek_to(fraction * duration);
    }

    /// Seeks inside the current chapter.
    pub fn seek_chapter_fraction(&mut self, fraction: f64) {
        match self.chapter.and_then(|i| self.chapters().get(i)) {
            Some(c) => self.seek_to(c.start + fraction * (c.end - c.start)),
            None => self.seek_fraction(fraction),
        }
    }

    pub fn set_speed(&mut self, index: usize) {
        self.speed_index = index.min(SPEEDS.len() - 1);
        let _ = self.controller.set_speed(SPEEDS[self.speed_index]);
    }

    pub fn set_sleep(&mut self, index: usize) {
        self.sleep_index = index;
        let _ = self
            .controller
            .set_sleep(sleep_choice(index), Instant::now());
    }

    /// Volume in percent; unmutes.
    pub fn set_volume(&mut self, percent: f64) {
        self.volume = percent.clamp(0.0, 100.0);
        self.muted = false;
        self.apply_volume();
    }

    pub fn toggle_mute(&mut self) {
        self.muted = !self.muted;
        self.apply_volume();
    }

    fn apply_volume(&mut self) {
        let volume = if self.muted { 0.0 } else { self.volume };
        let _ = self.controller.set_volume(volume);
    }

    pub fn media_key(&mut self, key: MediaKey) {
        let player = self.controller.player().clone();
        match key {
            MediaKey::Play => drop(player.play()),
            MediaKey::Pause | MediaKey::Stop => drop(player.pause()),
            MediaKey::Toggle => self.toggle(),
            MediaKey::Next => self.next_chapter(),
            MediaKey::Previous => self.previous_chapter(),
            MediaKey::SkipForward => self.skip_forward(),
            MediaKey::SkipBack => self.skip_back(),
            MediaKey::SeekBy(by) => self.seek_to(self.position + by),
            MediaKey::SetPosition(at) => self.seek_to(at),
        }
    }

    fn after_seek(&mut self) {
        self.position = self.controller.player().position();
        self.chapter = timeline::chapter_at(self.chapters(), self.position);
        self.report(true);
        self.update_controls_state();
    }
}

fn track(source: &abs_core::playback::TrackSource) -> Track {
    Track {
        url: source.url.clone(),
        start_offset: source.start_offset,
        duration: source.duration,
    }
}
