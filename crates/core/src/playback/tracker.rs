//! Listened time and sync timing for one playback session. No IO.

use std::time::{Duration, Instant};

use abs_api::models::SessionSync;

/// Sync interval while playing.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(15);
/// Longest gap between two reports that still counts as listening, so a laptop
/// sleep between reports is not counted.
const MAX_STEP: Duration = Duration::from_secs(5);

/// Counts time spent playing and says when the server should hear about it.
#[derive(Debug, Clone)]
pub struct ListenTracker {
    playing_since: Option<Instant>,
    listened: Duration,
    last_sync: Instant,
    position: f64,
    duration: f64,
}

impl ListenTracker {
    pub fn new(now: Instant, position: f64, duration: f64) -> Self {
        Self {
            playing_since: None,
            listened: Duration::ZERO,
            last_sync: now,
            position,
            duration,
        }
    }

    /// Records the player state. Returns true when a sync is due: every
    /// [`SYNC_INTERVAL`] while playing, and right after pausing.
    pub fn report(&mut self, now: Instant, position: f64, playing: bool) -> bool {
        let was_playing = self.playing_since.is_some();
        if let Some(since) = self.playing_since {
            self.listened += now.saturating_duration_since(since).min(MAX_STEP);
        }
        self.playing_since = playing.then_some(now);
        self.position = position;
        let interval_due =
            playing && now.saturating_duration_since(self.last_sync) >= SYNC_INTERVAL;
        interval_due || (was_playing && !playing)
    }

    /// The sync body for now; resets the listened counter.
    pub fn take_sync(&mut self, now: Instant) -> SessionSync {
        if let Some(since) = self.playing_since {
            self.listened += now.saturating_duration_since(since).min(MAX_STEP);
            self.playing_since = Some(now);
        }
        let sync = SessionSync {
            current_time: self.position,
            time_listened: self.listened.as_secs_f64(),
            duration: self.duration,
        };
        self.listened = Duration::ZERO;
        self.last_sync = now;
        sync
    }

    /// Puts listened time back after a failed sync so it is sent next time.
    pub fn restore(&mut self, failed: &SessionSync) {
        self.listened += Duration::from_secs_f64(failed.time_listened.max(0.0));
    }

    pub fn position(&self) -> f64 {
        self.position
    }

    /// Takes a position the player was moved to from outside, as from another device.
    pub fn set_position(&mut self, position: f64) {
        self.position = position;
    }

    pub fn is_playing(&self) -> bool {
        self.playing_since.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncs_every_interval_and_on_pause() {
        let t0 = Instant::now();
        let mut tracker = ListenTracker::new(t0, 0.0, 100.0);
        assert!(!tracker.report(t0, 0.0, true));
        let mut now = t0;
        for second in 1..15 {
            now = t0 + Duration::from_secs(second);
            assert!(!tracker.report(now, second as f64, true));
        }
        now += Duration::from_secs(1);
        assert!(tracker.report(now, 15.0, true));
        let sync = tracker.take_sync(now);
        assert_eq!((sync.current_time, sync.time_listened), (15.0, 15.0));

        now += Duration::from_secs(2);
        assert!(tracker.report(now, 17.0, false), "pause syncs at once");
        assert_eq!(tracker.take_sync(now).time_listened, 2.0);
        assert!(!tracker.report(now + Duration::from_secs(60), 17.0, false));
    }

    #[test]
    fn sleep_gaps_and_failed_syncs() {
        let t0 = Instant::now();
        let mut tracker = ListenTracker::new(t0, 0.0, 100.0);
        tracker.report(t0, 0.0, true);
        tracker.report(t0 + Duration::from_secs(3600), 1.0, true);
        let failed = tracker.take_sync(t0 + Duration::from_secs(3600));
        assert_eq!(failed.time_listened, 5.0, "an hour asleep counts as 5 s");
        tracker.restore(&failed);
        let next = tracker.take_sync(t0 + Duration::from_secs(3600));
        assert_eq!(next.time_listened, 5.0);
    }
}
