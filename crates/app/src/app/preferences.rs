//! The settings page.

use abs_core::engine::Command;

use slint::ComponentHandle;

use super::App;
use crate::paths::dir_size;
use crate::settings::{
    ACCENT, CACHE_MEGABYTES, CLOSE_TO_TRAY, COVER_CACHE, Choices, DARK_FLAVOR, DEFAULT_SPEED,
    LIGHT_FLAVOR, PROGRESS_BAR, SKIP_BACK, SKIP_BACK_SECONDS, SKIP_FORWARD, SKIP_FORWARD_SECONDS,
    THEME_MODE,
};
use crate::tray::{Labels, Tray};

impl App {
    /// Pushes the settings to the player and the settings page.
    pub(super) fn apply_choices(&mut self) {
        let choices = self.choices;
        if let Some(playback) = &mut self.playback {
            playback.apply_choices(&choices);
        }
        self.ui(|ui, s| {
            ui.set_skip_back_text(s.invoke_seconds(SKIP_BACK_SECONDS[choices.skip_back] as i32));
            ui.set_skip_forward_text(
                s.invoke_seconds(SKIP_FORWARD_SECONDS[choices.skip_forward] as i32),
            );
            ui.set_skip_back_index(choices.skip_back as i32);
            ui.set_skip_forward_index(choices.skip_forward as i32);
            ui.set_default_speed_index(choices.default_speed as i32);
            ui.set_close_to_tray(choices.close_to_tray);
            ui.set_cache_index(choices.cover_cache as i32);
            ui.set_progress_mode(choices.progress_bar as i32);
        });
        self.update_tray();
    }

    pub(super) fn refresh_settings(&self) {
        let used = dir_size(&self.cover_dir) / (1024 * 1024);
        let file_credentials = self.file_credentials;
        self.ui(|ui, s| {
            ui.set_cache_used_text(s.invoke_megabytes(used as i32));
            ui.set_credentials_note(if file_credentials {
                s.invoke_file_credentials()
            } else {
                "".into()
            });
        });
        self.refresh_accounts();
    }

    pub fn set_setting(&mut self, key: &str, index: usize) {
        let store = self.db.store();
        let settings = store.settings();
        let _ = match key {
            SKIP_BACK | SKIP_FORWARD | DEFAULT_SPEED | CLOSE_TO_TRAY | PROGRESS_BAR
            | THEME_MODE | LIGHT_FLAVOR | DARK_FLAVOR | ACCENT => settings.set(key, index),
            COVER_CACHE => {
                let megabytes = CACHE_MEGABYTES[index.min(CACHE_MEGABYTES.len() - 1)];
                self.engine.send(Command::SetCoverCacheSize { megabytes });
                Ok(())
            }
            _ => Ok(()),
        };
        if key == COVER_CACHE {
            self.choices.cover_cache = index;
        } else {
            self.choices = Choices {
                cover_cache: self.choices.cover_cache,
                ..Choices::load(&store)
            };
        }
        self.apply_choices();
        self.apply_theme();
    }

    /// Closing the window keeps the app running while it plays and the tray is shown.
    pub fn keeps_running_on_close(&self) -> bool {
        self.is_playing() && self.tray.is_some()
    }

    /// Shows the tray icon while "keep playing in the tray" is on.
    pub(super) fn update_tray(&mut self) {
        if !self.choices.close_to_tray {
            self.tray = None;
            return;
        }
        if self.tray.is_some() {
            return;
        }
        let labels = self.ui(|_, s| Labels {
            show: s.invoke_tray_show().into(),
            toggle: s.invoke_tray_toggle().into(),
            quit: s.invoke_tray_quit().into(),
        });
        let accent = self
            .window
            .upgrade()
            .map(|w| w.global::<crate::Theme>().get_accent())
            .map_or([0x1e, 0x66, 0xf5], |c| [c.red(), c.green(), c.blue()]);
        if let Some(labels) = labels {
            match Tray::new(&labels, "Audiobookshelf", accent) {
                Ok(tray) => self.tray = Some(tray),
                Err(e) => eprintln!("tray: {e}"),
            }
        }
    }

    /// Brings the window back, as from the tray.
    pub fn show_window(&self) {
        if let Some(window) = self.window.upgrade() {
            let _ = window.show();
        }
    }
}
