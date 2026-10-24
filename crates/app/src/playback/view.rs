//! Player state on the `Ui` global.

use std::rc::Rc;
use std::time::Instant;

use abs_player::controller::SleepChoice;
use slint::{Model, ModelRc, VecModel};

use super::Playback;
use crate::format;
use crate::settings::{SKIP_BACK_SECONDS, SKIP_FORWARD_SECONDS, SLEEP_MINUTES, sleep_choice};
use crate::{ChapterData, Strings, Ui};

/// The chapter list model and what it currently shows.
pub struct PlayerView {
    chapters: Rc<VecModel<ChapterData>>,
    item: Option<String>,
    current: Option<usize>,
}

impl PlayerView {
    pub fn new() -> Self {
        Self {
            chapters: Rc::new(VecModel::default()),
            item: None,
            current: None,
        }
    }

    pub fn model(&self) -> ModelRc<ChapterData> {
        self.chapters.clone().into()
    }

    /// Writes the player properties; cheap enough for every position event.
    pub fn update(&mut self, playback: &Playback, ui: &Ui<'_>, strings: &Strings<'_>) {
        ui.set_loading_media(playback.is_loading());
        ui.set_playing(playback.playing);
        let Some(now) = playback.now() else {
            ui.set_has_media(false);
            return;
        };
        ui.set_has_media(true);
        ui.set_now_id(now.item_id.as_str().into());
        ui.set_now_title(now.title.as_str().into());
        ui.set_now_author(now.author.as_str().into());
        let (position, duration) = (playback.position, now.duration);
        ui.set_position_fraction(fraction(position, 0.0, duration));
        ui.set_position_text(format::clock(position).into());
        ui.set_remaining_text(format::remaining(duration - position).into());
        ui.set_speed_index(playback.speed_index as i32);
        ui.set_volume(playback.volume as f32);
        ui.set_muted(playback.muted);
        ui.set_sleep_index(playback.sleep_index as i32);
        ui.set_sleep_text(sleep_text(playback, strings).into());
        self.update_chapter(playback, ui);
    }

    fn update_chapter(&mut self, playback: &Playback, ui: &Ui<'_>) {
        let Some(now) = playback.now() else {
            return;
        };
        if self.item.as_deref() != Some(now.item_id.as_str()) {
            self.item = Some(now.item_id.clone());
            self.current = None;
            let rows: Vec<ChapterData> = now
                .chapters
                .iter()
                .map(|c| chapter_row(&c.title, c.start, c.end, false))
                .collect();
            self.chapters.set_vec(rows);
            ui.set_chapter_markers(markers(&now.chapters, now.duration));
        }
        if self.current != playback.chapter {
            self.set_current(self.current, false);
            self.set_current(playback.chapter, true);
            self.current = playback.chapter;
        }
        match playback.chapter.and_then(|i| now.chapters.get(i)) {
            Some(c) => {
                ui.set_chapter_title(c.title.as_str().into());
                ui.set_chapter_fraction(fraction(playback.position, c.start, c.end));
                let into = format::clock(playback.position - c.start);
                let length = format::clock(c.end - c.start);
                ui.set_chapter_position_text(format!("{into} / {length}").into());
                ui.set_chapter_elapsed_text(into.into());
                ui.set_chapter_remaining_text(format::remaining(c.end - playback.position).into());
            }
            None => {
                ui.set_chapter_title("".into());
                ui.set_chapter_fraction(fraction(playback.position, 0.0, now.duration));
                ui.set_chapter_position_text(format::clock(playback.position).into());
                ui.set_chapter_elapsed_text(format::clock(playback.position).into());
                ui.set_chapter_remaining_text(
                    format::remaining(now.duration - playback.position).into(),
                );
            }
        }
    }

    fn set_current(&self, index: Option<usize>, current: bool) {
        let Some(index) = index else {
            return;
        };
        if let Some(mut row) = self.chapters.row_data(index) {
            row.current = current;
            self.chapters.set_row_data(index, row);
        }
    }
}

/// A chapter list row.
pub fn chapter_row(title: &str, start: f64, end: f64, current: bool) -> ChapterData {
    ChapterData {
        title: title.into(),
        start: start as f32,
        start_text: format::clock(start).into(),
        length_text: format::clock(end - start).into(),
        current,
    }
}

/// The option labels that depend on settings.
pub fn fill_options(ui: &Ui<'_>, strings: &Strings<'_>) {
    let model = |items: Vec<slint::SharedString>| ModelRc::new(VecModel::from(items));
    let seconds = |list: &[u64]| {
        model(
            list.iter()
                .map(|s| strings.invoke_seconds(*s as i32))
                .collect(),
        )
    };
    ui.set_skip_back_options(seconds(&SKIP_BACK_SECONDS));
    ui.set_skip_forward_options(seconds(&SKIP_FORWARD_SECONDS));
    let speeds = crate::settings::SPEEDS
        .iter()
        .map(|s| format::speed(*s).into())
        .collect();
    ui.set_speed_options(model(speeds));
    let mut sleep = vec![strings.invoke_sleep_off()];
    sleep.extend(
        SLEEP_MINUTES
            .iter()
            .map(|m| strings.invoke_minutes(*m as i32)),
    );
    sleep.push(strings.invoke_sleep_chapter());
    ui.set_sleep_options(model(sleep));
    let cache = crate::settings::CACHE_MEGABYTES
        .iter()
        .map(|mb| strings.invoke_megabytes(*mb as i32))
        .collect();
    ui.set_cache_options(model(cache));
}

fn sleep_text(playback: &Playback, strings: &Strings<'_>) -> String {
    match sleep_choice(playback.sleep_index) {
        SleepChoice::Off => String::new(),
        SleepChoice::EndOfChapter => strings.invoke_end_of_chapter().into(),
        SleepChoice::After(_) => playback
            .controller
            .sleep_remaining(Instant::now())
            .map(|left| format::clock(left.as_secs_f64()))
            .unwrap_or_default(),
    }
}

/// Chapter starts after the first, as fractions of the book.
fn markers(chapters: &[abs_player::timeline::Chapter], duration: f64) -> ModelRc<f32> {
    let starts: Vec<f32> = chapters
        .iter()
        .skip(1)
        .map(|c| fraction(c.start, 0.0, duration))
        .collect();
    ModelRc::new(VecModel::from(starts))
}

fn fraction(position: f64, start: f64, end: f64) -> f32 {
    if end > start {
        ((position - start) / (end - start)).clamp(0.0, 1.0) as f32
    } else {
        0.0
    }
}
