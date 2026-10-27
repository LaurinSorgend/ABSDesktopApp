//! The settings page.

use abs_core::engine::Command;
use abs_core::works::{MIRROR_PROGRESS_SETTING, PREFERRED_ACCOUNT_SETTING};

use slint::ComponentHandle;

use super::App;
use crate::paths::dir_size;
use crate::settings::{
    ACCENT, CACHE_MEGABYTES, CLOSE_TO_TRAY, CONFLICT, CONFLICT_MINUTES, COVER_CACHE, Choices,
    DARK_PALETTE, DEFAULT_SPEED, FONT, LIGHT_PALETTE, MONO_FONT, PROGRESS_BAR, REMOTE_CHECK,
    REMOTE_CHECK_SECONDS, SKIP_BACK, SKIP_BACK_SECONDS, SKIP_FORWARD, SKIP_FORWARD_SECONDS, THEME,
    THEME_INDEX_KEYS,
};
use crate::tray::{Labels, Tray};
use crate::updates::AUTO_CHECK;

impl App {
    /// Pushes the settings to the player and the settings page.
    pub(super) fn apply_choices(&mut self) {
        let choices = self.choices.clone();
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
            ui.set_conflict_index(choices.conflict as i32);
            ui.set_remote_check_index(choices.remote_check as i32);
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
            SKIP_BACK | SKIP_FORWARD | DEFAULT_SPEED | CLOSE_TO_TRAY | PROGRESS_BAR => {
                settings.set(key, index)
            }
            k if THEME_INDEX_KEYS.contains(&k) => settings.set(key, index),
            THEME | LIGHT_PALETTE | DARK_PALETTE | ACCENT => match self.theme_value(key, index) {
                Some((key, value)) => settings.set(&key, value),
                None => Ok(()),
            },
            FONT | MONO_FONT => match self.font_at(index) {
                name if name.is_empty() => settings.remove(key),
                name => settings.set(key, name),
            },
            COVER_CACHE => {
                let megabytes = CACHE_MEGABYTES[index.min(CACHE_MEGABYTES.len() - 1)];
                self.engine.send(Command::SetCoverCacheSize { megabytes });
                Ok(())
            }
            REMOTE_CHECK => {
                let seconds = REMOTE_CHECK_SECONDS[index.min(REMOTE_CHECK_SECONDS.len() - 1)];
                self.engine.send(Command::SetRemoteCheck { seconds });
                Ok(())
            }
            CONFLICT => settings.set(key, CONFLICT_MINUTES[index.min(CONFLICT_MINUTES.len() - 1)]),
            PREFERRED_ACCOUNT_SETTING => {
                match index.checked_sub(1).and_then(|i| self.accounts.get(i)) {
                    Some(account) => settings.set(key, &account.id),
                    None => settings.remove(key),
                }
            }
            MIRROR_PROGRESS_SETTING => settings.set(key, index == 1),
            AUTO_CHECK => {
                self.set_update_check(index == 1);
                Ok(())
            }
            _ => Ok(()),
        };
        if key == COVER_CACHE {
            self.choices.cover_cache = index;
        } else if key == REMOTE_CHECK {
            self.choices.remote_check = index.min(REMOTE_CHECK_SECONDS.len() - 1);
        } else {
            self.reload_choices();
        }
        if key == DEFAULT_SPEED
            && let Some(playback) = &mut self.playback
        {
            playback.set_speed(self.choices.default_speed);
        }
        self.apply_choices();
        self.apply_theme();
        self.refresh_source_settings();
    }

    /// Reads the settings again; the values the engine stores stay as the app has them.
    pub(super) fn reload_choices(&mut self) {
        self.choices = Choices {
            cover_cache: self.choices.cover_cache,
            remote_check: self.choices.remote_check,
            ..Choices::load(&self.db.store())
        };
    }

    /// The preferred account list and the mirror switch.
    pub(super) fn refresh_source_settings(&self) {
        let settings = self.db.store().settings();
        let preferred = settings.get(PREFERRED_ACCOUNT_SETTING).ok().flatten();
        let mirror = settings
            .get_or(MIRROR_PROGRESS_SETTING, false)
            .unwrap_or(false);
        let oldest = self
            .ui(|_, s| s.invoke_oldest_account())
            .unwrap_or_default();
        let mut options = vec![oldest];
        options.extend(self.accounts.iter().map(|a| self.account_place(a).into()));
        let index = preferred
            .and_then(|id| self.accounts.iter().position(|a| a.id == id))
            .map_or(0, |i| i as i32 + 1);
        self.ui(|ui, _| {
            ui.set_preferred_account_options(slint::ModelRc::new(slint::VecModel::from(options)));
            ui.set_preferred_account_index(index);
            ui.set_mirror_progress(mirror);
        });
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
