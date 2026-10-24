//! Chapter jumps, skip intervals, speed, volume and the sleep timer on top of a [`Player`].

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::player::{Player, PlayerError};
use crate::sleep::{SleepState, SleepTimer};
use crate::timeline::{self, Chapter};

/// Default skip back interval.
pub const DEFAULT_SKIP_BACK: Duration = Duration::from_secs(10);
/// Default skip forward interval.
pub const DEFAULT_SKIP_FORWARD: Duration = Duration::from_secs(30);

/// Sleep timer choices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SleepChoice {
    Off,
    After(Duration),
    EndOfChapter,
}

/// Player controls the UI calls. Call [`Controller::tick`] about once a second.
pub struct Controller {
    player: Arc<dyn Player>,
    chapters: Vec<Chapter>,
    duration: f64,
    pub skip_back: Duration,
    pub skip_forward: Duration,
    speed: f64,
    volume: f64,
    sleep: Option<SleepTimer>,
}

impl Controller {
    pub fn new(player: Arc<dyn Player>) -> Self {
        Self {
            player,
            chapters: Vec::new(),
            duration: 0.0,
            skip_back: DEFAULT_SKIP_BACK,
            skip_forward: DEFAULT_SKIP_FORWARD,
            speed: 1.0,
            volume: 100.0,
            sleep: None,
        }
    }

    /// Sets the chapters and length of the loaded item.
    pub fn set_item(&mut self, chapters: Vec<Chapter>, duration: f64) {
        self.chapters = chapters;
        self.duration = duration;
    }

    pub fn player(&self) -> &Arc<dyn Player> {
        &self.player
    }

    pub fn chapters(&self) -> &[Chapter] {
        &self.chapters
    }

    /// Index of the chapter at the current position.
    pub fn current_chapter(&self) -> Option<usize> {
        timeline::chapter_at(&self.chapters, self.player.position())
    }

    pub fn toggle(&self) -> Result<(), PlayerError> {
        if self.player.is_paused() {
            self.player.play()
        } else {
            self.player.pause()
        }
    }

    pub fn skip_back(&self) -> Result<(), PlayerError> {
        self.seek(self.player.position() - self.skip_back.as_secs_f64())
    }

    pub fn skip_forward(&self) -> Result<(), PlayerError> {
        self.seek(self.player.position() + self.skip_forward.as_secs_f64())
    }

    /// Next chapter; does nothing in the last one.
    pub fn next_chapter(&self) -> Result<(), PlayerError> {
        match timeline::next_chapter_start(&self.chapters, self.player.position()) {
            Some(start) => self.seek(start),
            None => Ok(()),
        }
    }

    /// Restarts the chapter, or goes to the previous one near its start.
    pub fn previous_chapter(&self) -> Result<(), PlayerError> {
        let position = self.player.position();
        let target = timeline::previous_chapter_start(&self.chapters, position).unwrap_or(0.0);
        self.seek(target)
    }

    /// Seeks, clamped to the item.
    pub fn seek(&self, position: f64) -> Result<(), PlayerError> {
        let end = if self.duration > 0.0 {
            self.duration
        } else {
            f64::MAX
        };
        self.player.seek(position.clamp(0.0, end))
    }

    pub fn set_speed(&mut self, speed: f64) -> Result<(), PlayerError> {
        self.speed = speed;
        self.player.set_speed(speed)
    }

    pub fn set_volume(&mut self, percent: f64) -> Result<(), PlayerError> {
        self.volume = percent;
        self.player.set_volume(percent)
    }

    /// Starts, replaces or cancels the sleep timer.
    pub fn set_sleep(&mut self, choice: SleepChoice, now: Instant) -> Result<(), PlayerError> {
        self.sleep = match choice {
            SleepChoice::Off => None,
            SleepChoice::After(duration) => Some(SleepTimer::after(now, duration)),
            SleepChoice::EndOfChapter => self
                .current_chapter()
                .map(|i| SleepTimer::at_position(self.chapters[i].end)),
        };
        self.player.set_volume(self.volume)
    }

    /// Time left on the sleep timer.
    pub fn sleep_remaining(&self, now: Instant) -> Option<Duration> {
        let timer = self.sleep?;
        Some(timer.remaining(now, self.player.position(), self.speed))
    }

    /// Applies the sleep fade. Returns true when the timer just paused playback.
    pub fn tick(&mut self, now: Instant) -> Result<bool, PlayerError> {
        let Some(timer) = self.sleep else {
            return Ok(false);
        };
        match timer.state(now, self.player.position(), self.speed) {
            SleepState::Volume(factor) => {
                self.player.set_volume(self.volume * factor)?;
                Ok(false)
            }
            SleepState::Expired => {
                self.sleep = None;
                self.player.pause()?;
                self.player.set_volume(self.volume)?;
                Ok(true)
            }
        }
    }
}

#[cfg(test)]
mod tests;
