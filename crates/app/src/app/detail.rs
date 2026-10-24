//! The item page: stored item first, then the expanded item from the server.

use std::rc::Rc;

use abs_core::db::ProgressRow;
use abs_core::detail::ItemDetail;
use abs_core::engine::Command;
use slint::{Image, ModelRc, VecModel};

use super::App;
use super::downloads::DownloadView;
use crate::covers::LARGE_WIDTH;
use crate::format;
use crate::playback::view::chapter_row;
use crate::{ChapterData, DetailData, Page, Strings};

pub struct Detail {
    item_id: Option<String>,
    chapters: Rc<VecModel<ChapterData>>,
    /// Kept so the server answer can be merged with what the page shows.
    shown: Option<ItemDetail>,
}

impl Detail {
    pub fn new() -> Self {
        Self {
            item_id: None,
            chapters: Rc::new(VecModel::default()),
            shown: None,
        }
    }

    pub fn model(&self) -> ModelRc<ChapterData> {
        self.chapters.clone().into()
    }
}

impl App {
    pub fn open_item(&mut self, item_id: &str) {
        let Some(account_id) = self.active.clone() else {
            return;
        };
        let json = self
            .db
            .store()
            .catalog()
            .item_json(&account_id, item_id)
            .ok()
            .flatten();
        let Some(detail) = json.and_then(|j| ItemDetail::from_json(&j).ok()) else {
            return;
        };
        self.detail.item_id = Some(item_id.to_owned());
        self.show_detail(item_id, detail);
        if self.is_connected() {
            self.engine.send(Command::LoadItem {
                account_id,
                item_id: item_id.to_owned(),
            });
        }
        self.navigate(Page::Detail);
    }

    pub(super) fn item_loaded(&mut self, item_id: &str, json: &str) {
        if self.detail.item_id.as_deref() != Some(item_id) {
            return;
        }
        if let Ok(detail) = ItemDetail::from_json(json) {
            self.show_detail(item_id, detail);
        }
    }

    fn show_detail(&mut self, item_id: &str, detail: ItemDetail) {
        let account_id = self.active.clone().unwrap_or_default();
        let progress = self
            .db
            .store()
            .progress()
            .get(&account_id, item_id)
            .ok()
            .flatten();
        let etag = self
            .db
            .store()
            .catalog()
            .items_by_ids(&account_id, &[item_id])
            .ok()
            .and_then(|items| items.into_iter().next())
            .and_then(|item| item.cover_etag);
        let cover = self
            .covers
            .want(&account_id, item_id, etag.as_deref(), LARGE_WIDTH);
        let rows: Vec<ChapterData> = detail
            .chapters
            .iter()
            .map(|c| chapter_row(&c.title, c.start, c.end, false))
            .collect();
        self.detail.chapters.set_vec(rows);
        let connected = self.is_connected();
        let download = self.download_view(&account_id, item_id);
        self.ui(|ui, s| {
            let mut data = detail_data(item_id, &detail, progress.as_ref(), cover, connected, s);
            data.account_id = account_id.as_str().into();
            data.can_download = connected && detail.has_audio;
            set_download(&mut data, download);
            ui.set_detail(data);
        });
        self.detail.shown = Some(detail);
    }

    /// Updates progress on the item page when it shows one of `item_ids`.
    pub(super) fn refresh_detail_progress(&mut self, item_ids: &[String]) {
        let Some(item_id) = self.detail.item_id.clone() else {
            return;
        };
        if !item_ids.contains(&item_id) {
            return;
        }
        if let Some(detail) = self.detail.shown.clone() {
            self.show_detail(&item_id, detail);
        }
    }

    /// Updates the download controls when the item page shows this item.
    pub(super) fn refresh_detail_download(&self, account_id: &str, item_id: &str) {
        let shown = self.detail.item_id.as_deref() == Some(item_id)
            && self.active.as_deref() == Some(account_id);
        if !shown {
            return;
        }
        let download = self.download_view(account_id, item_id);
        self.ui(|ui, _| {
            let mut data = ui.get_detail();
            set_download(&mut data, download);
            ui.set_detail(data);
        });
    }

    pub(super) fn detail_cover(&self, item_id: &str, image: Option<&Image>) {
        if self.detail.item_id.as_deref() != Some(item_id) {
            return;
        }
        let Some(image) = image else {
            return;
        };
        self.ui(|ui, _| {
            let mut data = ui.get_detail();
            data.cover = image.clone();
            data.has_cover = true;
            ui.set_detail(data);
        });
    }

    pub fn set_finished(&mut self, item_id: &str, finished: bool) {
        if let Some(account_id) = self.active.clone() {
            self.engine.send(Command::SetFinished {
                account_id,
                item_id: item_id.to_owned(),
                finished,
            });
        }
    }
}

fn set_download(data: &mut DetailData, download: DownloadView) {
    data.download_state = download.state;
    data.download_text = download.text;
    data.download_fraction = download.fraction;
}

fn detail_data(
    item_id: &str,
    detail: &ItemDetail,
    progress: Option<&ProgressRow>,
    cover: Option<Image>,
    connected: bool,
    s: &Strings<'_>,
) -> DetailData {
    let text = |value: &Option<String>| value.clone().unwrap_or_default().into();
    let duration = detail.duration.unwrap_or_default();
    let chapters_note = match (detail.chapters.is_empty(), connected) {
        (false, _) => "".into(),
        (true, true) => s.invoke_chapters_from_server(),
        (true, false) => s.invoke_needs_connection(),
    };
    DetailData {
        id: item_id.into(),
        title: detail.title.as_str().into(),
        subtitle: text(&detail.subtitle),
        author: text(&detail.author),
        narrator: text(&detail.narrator),
        series: text(&detail.series),
        year: text(&detail.published_year),
        genres: detail.genres.join(", ").into(),
        duration_text: duration_text(duration, s).into(),
        progress_text: progress_text(progress, duration, s).into(),
        progress: progress.map_or(0.0, |p| p.progress as f32),
        finished: progress.is_some_and(|p| p.is_finished),
        description: text(&detail.description),
        has_cover: cover.is_some(),
        cover: cover.unwrap_or_default(),
        has_audio: detail.has_audio,
        chapters_note,
        ..DetailData::default()
    }
}

pub fn duration_text(seconds: f64, s: &Strings<'_>) -> String {
    if seconds <= 0.0 {
        return String::new();
    }
    match format::hours_minutes(seconds) {
        (0, minutes) => s.invoke_minutes(minutes).into(),
        (hours, minutes) => s.invoke_hours_minutes(hours, minutes).into(),
    }
}

fn progress_text(progress: Option<&ProgressRow>, duration: f64, s: &Strings<'_>) -> String {
    match progress {
        Some(p) if p.is_finished => s.invoke_finished().into(),
        Some(p) if p.current_time > 0.0 => {
            let percent = (p.progress * 100.0).round() as i32;
            let left = duration_text((duration - p.current_time).max(0.0), s);
            s.invoke_progress(percent, left.into()).into()
        }
        _ => s.invoke_not_started().into(),
    }
}
