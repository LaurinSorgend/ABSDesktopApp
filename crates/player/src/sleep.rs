//! Sleep timer: stop after a fixed time or at the end of the current chapter, with a fade-out.

use std::time::{Duration, Instant};

/// Length of the volume fade before the timer stops playback.
pub const FADE: Duration = Duration::from_secs(10);

/// When the timer fires.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SleepMode {
    /// At a wall-clock instant.
    At(Instant),
    /// When playback reaches this global position (the end of a chapter).
    Position(f64),
}

/// What the player should do now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SleepState {
    /// Play at this fraction of the user's volume (1.0 outside the fade).
    Volume(f64),
    /// Time is up: pause and restore the volume.
    Expired,
}

/// A running sleep timer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SleepTimer {
    mode: SleepMode,
}

impl SleepTimer {
    /// Stops `duration` from `now`.
    pub fn after(now: Instant, duration: Duration) -> Self {
        Self {
            mode: SleepMode::At(now + duration),
        }
    }

    /// Stops when playback reaches `chapter_end`.
    pub fn at_position(chapter_end: f64) -> Self {
        Self {
            mode: SleepMode::Position(chapter_end),
        }
    }

    pub fn mode(&self) -> SleepMode {
        self.mode
    }

    /// The state at wall time `now`, playback `position` and playback `speed`.
    pub fn state(&self, now: Instant, position: f64, speed: f64) -> SleepState {
        let remaining = match self.mode {
            SleepMode::At(deadline) => deadline.saturating_duration_since(now).as_secs_f64(),
            SleepMode::Position(end) => ((end - position) / speed.max(0.01)).max(0.0),
        };
        if remaining <= 0.0 {
            return SleepState::Expired;
        }
        SleepState::Volume((remaining / FADE.as_secs_f64()).min(1.0))
    }

    /// Wall time left, for the UI.
    pub fn remaining(&self, now: Instant, position: f64, speed: f64) -> Duration {
        match self.mode {
            SleepMode::At(deadline) => deadline.saturating_duration_since(now),
            SleepMode::Position(end) => {
                Duration::from_secs_f64(((end - position) / speed.max(0.01)).max(0.0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_duration_fades_then_expires() {
        let start = Instant::now();
        let timer = SleepTimer::after(start, Duration::from_secs(60));
        assert_eq!(timer.state(start, 0.0, 1.0), SleepState::Volume(1.0));
        let fading = timer.state(start + Duration::from_secs(55), 0.0, 1.0);
        assert_eq!(fading, SleepState::Volume(0.5));
        let done = timer.state(start + Duration::from_secs(61), 0.0, 1.0);
        assert_eq!(done, SleepState::Expired);
    }

    #[test]
    fn chapter_end_uses_playback_speed() {
        let now = Instant::now();
        let timer = SleepTimer::at_position(100.0);
        assert_eq!(timer.state(now, 90.0, 2.0), SleepState::Volume(0.5));
        assert_eq!(timer.state(now, 100.0, 1.0), SleepState::Expired);
        assert_eq!(timer.remaining(now, 40.0, 2.0), Duration::from_secs(30));
    }
}
