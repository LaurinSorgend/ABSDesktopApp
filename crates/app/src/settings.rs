//! User settings and the choices the settings page offers.

use std::time::Duration;

use abs_core::db::Store;
use abs_core::engine::{COVER_CACHE_SETTING, DEFAULT_COVER_CACHE_MB};
use abs_player::controller::SleepChoice;

pub const SKIP_BACK: &str = "skip_back";
pub const SKIP_FORWARD: &str = "skip_forward";
pub const DEFAULT_SPEED: &str = "default_speed";
pub const CLOSE_TO_TRAY: &str = "close_to_tray";
pub const COVER_CACHE: &str = "cover_cache";
pub const ACTIVE_ACCOUNT: &str = "active_account";
pub const PROGRESS_BAR: &str = "progress_bar";
pub const VOLUME: &str = "volume";

pub const SKIP_BACK_SECONDS: [u64; 4] = [5, 10, 15, 30];
pub const SKIP_FORWARD_SECONDS: [u64; 4] = [10, 15, 30, 60];
pub const SPEEDS: [f64; 8] = [0.75, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5];
pub const CACHE_MEGABYTES: [u64; 4] = [100, 250, 500, 1000];
/// Sleep timer minutes; the index after the last is "end of chapter", index 0 is off.
pub const SLEEP_MINUTES: [u64; 5] = [10, 15, 30, 45, 60];

/// Playback settings as indices into the option lists.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Choices {
    pub skip_back: usize,
    pub skip_forward: usize,
    pub default_speed: usize,
    pub close_to_tray: bool,
    pub cover_cache: usize,
    /// 0 book, 1 chapter, 2 both.
    pub progress_bar: usize,
    /// Percent.
    pub volume: f64,
}

impl Choices {
    pub fn load(store: &Store<'_>) -> Self {
        let settings = store.settings();
        let get = |key: &str, default: usize, len: usize| {
            settings
                .get_or(key, default)
                .unwrap_or(default)
                .min(len - 1)
        };
        let cache_mb = settings
            .get_or(COVER_CACHE_SETTING, DEFAULT_COVER_CACHE_MB)
            .unwrap_or(DEFAULT_COVER_CACHE_MB);
        Self {
            skip_back: get(SKIP_BACK, 1, SKIP_BACK_SECONDS.len()),
            skip_forward: get(SKIP_FORWARD, 2, SKIP_FORWARD_SECONDS.len()),
            default_speed: get(DEFAULT_SPEED, 1, SPEEDS.len()),
            close_to_tray: settings.get_or(CLOSE_TO_TRAY, 0u8).unwrap_or(0) == 1,
            progress_bar: get(PROGRESS_BAR, 0, 3),
            volume: settings
                .get_or(VOLUME, 100.0_f64)
                .unwrap_or(100.0)
                .clamp(0.0, 100.0),
            cover_cache: CACHE_MEGABYTES
                .iter()
                .position(|mb| *mb >= cache_mb)
                .unwrap_or(CACHE_MEGABYTES.len() - 1),
        }
    }

    pub fn skip_back(&self) -> Duration {
        Duration::from_secs(SKIP_BACK_SECONDS[self.skip_back])
    }

    pub fn skip_forward(&self) -> Duration {
        Duration::from_secs(SKIP_FORWARD_SECONDS[self.skip_forward])
    }
}

/// The sleep timer for an option index.
pub fn sleep_choice(index: usize) -> SleepChoice {
    match index {
        0 => SleepChoice::Off,
        i if i <= SLEEP_MINUTES.len() => {
            SleepChoice::After(Duration::from_secs(SLEEP_MINUTES[i - 1] * 60))
        }
        _ => SleepChoice::EndOfChapter,
    }
}
