//! Player actions from the UI, keyboard and media keys, and player events.

use abs_core::engine::Command;
use abs_core::playback::PlayStart;
use abs_player::PlayerEvent;
use abs_player::media_controls::{MediaControls, MediaKey};
use slint::{ComponentHandle, Image, ModelRc, VecModel};

use super::App;
use crate::covers::LARGE_WIDTH;
use crate::format;
use crate::settings::{DEFAULT_SPEED, SPEEDS, VOLUME};
use crate::{BookmarkData, Strings, Ui};

/// Something the user did to the player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Toggle,
    SkipBack,
    SkipForward,
    PreviousChapter,
    NextChapter,
    SeekFraction(f64),
    SeekChapterFraction(f64),
    SeekTo(f64),
    Speed(usize),
    Sleep(usize),
}

impl App {
    /// Plays an item of the active account, from `start` or where it was left.
    pub fn play_item(&mut self, item_id: &str, start: Option<f64>) {
        let Some(account_id) = self.active.clone() else {
            return;
        };
        if let Some(playback) = &mut self.playback {
            playback.request(&account_id, item_id, start);
        }
        self.update_player_view();
    }

    pub(super) fn playback_started(&mut self, start: PlayStart) {
        let Some(playback) = &mut self.playback else {
            return;
        };
        let (account_id, item_id) = (start.account_id.clone(), start.item_id.clone());
        if let Err(e) = playback.started(start) {
            playback.start_failed();
            let text = self.ui(|_, s| s.invoke_player_error(e.to_string().into()));
            self.show_error(text.unwrap_or_default().into());
        }
        let etag = self
            .db
            .store()
            .catalog()
            .items_by_ids(&account_id, &[item_id.as_str()])
            .ok()
            .and_then(|items| items.into_iter().next())
            .and_then(|item| item.cover_etag);
        let cover = self
            .covers
            .want(&account_id, &item_id, etag.as_deref(), LARGE_WIDTH);
        self.now_cover(&item_id, cover.as_ref());
        if cover.is_none() {
            self.ui(|ui, _| ui.set_now_has_cover(false));
        }
        self.refresh_bookmarks();
        self.update_player_view();
    }

    pub(super) fn playback_failed(&mut self, message: &str) {
        if let Some(playback) = &mut self.playback {
            playback.start_failed();
        }
        self.update_player_view();
        let text = self.ui(|_, s| s.invoke_playback_failed(message.into()));
        self.show_error(text.unwrap_or_default().into());
    }

    pub fn player_event(&mut self, event: PlayerEvent) {
        let Some(playback) = &mut self.playback else {
            return;
        };
        if let Some(message) = playback.player_event(event) {
            let text = self.ui(|_, s| s.invoke_player_error(message.into()));
            self.show_error(text.unwrap_or_default().into());
        }
        self.update_player_view();
    }

    /// Once a second: the sleep timer and its countdown.
    pub fn tick(&mut self) {
        if let Some(playback) = &mut self.playback {
            playback.tick();
        }
        self.update_player_view();
    }

    pub fn player_action(&mut self, action: Action) {
        let Some(playback) = &mut self.playback else {
            return;
        };
        match action {
            Action::Toggle => playback.toggle(),
            Action::SkipBack => playback.skip_back(),
            Action::SkipForward => playback.skip_forward(),
            Action::PreviousChapter => playback.previous_chapter(),
            Action::NextChapter => playback.next_chapter(),
            Action::SeekFraction(f) => playback.seek_fraction(f),
            Action::SeekChapterFraction(f) => playback.seek_chapter_fraction(f),
            Action::SeekTo(at) => playback.seek_to(at),
            Action::Speed(index) => playback.set_speed(index),
            Action::Sleep(index) => playback.set_sleep(index),
        }
        if let Action::Speed(index) = action {
            self.remember_speed(index);
        }
        self.update_player_view();
    }

    /// The last speed becomes the default for the next start and the next item.
    fn remember_speed(&mut self, index: usize) {
        let index = index.min(SPEEDS.len() - 1);
        let _ = self.db.store().settings().set(DEFAULT_SPEED, index);
        self.choices.default_speed = index;
        self.ui(|ui, _| ui.set_default_speed_index(index as i32));
    }

    pub fn set_volume(&mut self, percent: f64) {
        let Some(playback) = &mut self.playback else {
            return;
        };
        playback.set_volume(percent);
        self.choices.volume = playback.volume();
        let _ = self.db.store().settings().set(VOLUME, self.choices.volume);
        self.update_player_view();
    }

    pub fn toggle_mute(&mut self) {
        if let Some(playback) = &mut self.playback {
            playback.toggle_mute();
        }
        self.update_player_view();
    }

    pub fn media_key(&mut self, key: MediaKey) {
        if let Some(playback) = &mut self.playback {
            playback.media_key(key);
        }
        self.update_player_view();
    }

    pub fn set_media_controls(&mut self, controls: Box<dyn MediaControls>) {
        if let Some(playback) = &mut self.playback {
            playback.set_media_controls(controls);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playback.as_ref().is_some_and(|p| p.is_playing())
    }

    pub(super) fn now_cover(&self, item_id: &str, image: Option<&Image>) {
        let is_now = self
            .playback
            .as_ref()
            .and_then(|p| p.now())
            .is_some_and(|n| n.item_id == item_id);
        if let (true, Some(image)) = (is_now, image) {
            self.ui(|ui, _| {
                ui.set_now_cover(image.clone());
                ui.set_now_has_cover(true);
            });
        }
    }

    pub fn add_bookmark(&mut self) {
        let Some(playback) = &self.playback else {
            return;
        };
        let Some(now) = playback.now() else {
            return;
        };
        let time = playback.position().floor();
        let title = self
            .ui(|_, s| s.invoke_bookmark_at(format::clock(time).into()))
            .unwrap_or_default();
        self.engine.send(Command::AddBookmark {
            account_id: now.account_id.clone(),
            item_id: now.item_id.clone(),
            time,
            title: title.into(),
        });
    }

    pub fn remove_bookmark(&mut self, time: f64) {
        let Some(now) = self.playback.as_ref().and_then(|p| p.now()) else {
            return;
        };
        self.engine.send(Command::RemoveBookmark {
            account_id: now.account_id.clone(),
            item_id: now.item_id.clone(),
            time,
        });
    }

    pub(super) fn bookmarks_changed(&mut self, _item_id: &str) {
        self.refresh_bookmarks();
    }

    fn refresh_bookmarks(&self) {
        let Some(now) = self.playback.as_ref().and_then(|p| p.now()) else {
            return;
        };
        let rows: Vec<BookmarkData> = self
            .db
            .store()
            .progress()
            .bookmarks(&now.account_id, &now.item_id)
            .unwrap_or_default()
            .into_iter()
            .map(|b| BookmarkData {
                title: b.title.into(),
                time: b.time as f32,
                time_text: format::clock(b.time).into(),
            })
            .collect();
        self.ui(|ui, _| ui.set_bookmarks(ModelRc::new(VecModel::from(rows))));
    }

    fn update_player_view(&mut self) {
        let (Some(window), Some(playback)) = (self.window.upgrade(), self.playback.as_ref()) else {
            return;
        };
        let ui = window.global::<Ui>();
        let strings = window.global::<Strings>();
        self.player_view.update(playback, &ui, &strings);
    }
}
