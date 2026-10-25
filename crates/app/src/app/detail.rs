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

/// Enough for about five lines on the hover card.
const PEEK_DESCRIPTION_CHARS: usize = 240;

pub struct Detail {
    item_id: Option<String>,
    /// The account of the item shown; it can differ from the active one.
    account_id: Option<String>,
    chapters: Rc<VecModel<ChapterData>>,
    /// Kept so the server answer can be merged with what the page shows.
    shown: Option<ItemDetail>,
}

impl Detail {
    pub fn new() -> Self {
        Self {
            item_id: None,
            account_id: None,
            chapters: Rc::new(VecModel::default()),
            shown: None,
        }
    }

    pub fn model(&self) -> ModelRc<ChapterData> {
        self.chapters.clone().into()
    }
}

impl App {
    /// Opens a tile: an item of the active account, or `account/item` from a mixed grid.
    pub fn open_item(&mut self, id: &str) {
        match super::works::split_key(id) {
            Some((account_id, item_id)) => self.open_account_item(account_id, item_id),
            None => {
                if let Some(account_id) = self.active.clone() {
                    self.open_account_item(&account_id, id);
                }
            }
        }
    }

    pub fn open_account_item(&mut self, account_id: &str, item_id: &str) {
        let account_id = account_id.to_owned();
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
        self.detail.account_id = Some(account_id.clone());
        self.show_detail(item_id, detail);
        if self.is_account_connected(&account_id) {
            self.engine.send(Command::LoadItem {
                account_id,
                item_id: item_id.to_owned(),
            });
        }
        self.navigate(Page::Detail);
    }

    /// Fills the hover card for a tile from the stored item.
    pub fn peek_item(&self, id: &str) {
        let (account_id, item_id) = match super::works::split_key(id) {
            Some(pair) => pair,
            None => match self.active.as_deref() {
                Some(account_id) => (account_id, id),
                None => return,
            },
        };
        let store = self.db.store();
        let json = store
            .catalog()
            .item_json(account_id, item_id)
            .ok()
            .flatten();
        let Some(detail) = json.and_then(|j| ItemDetail::from_json(&j).ok()) else {
            return;
        };
        let progress = store.progress().get(account_id, item_id).ok().flatten();
        self.ui(|ui, s| {
            let mut data = detail_data(item_id, &detail, progress.as_ref(), None, true, s);
            data.description = format::excerpt(&data.description, PEEK_DESCRIPTION_CHARS).into();
            ui.set_peek(data);
        });
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
        let account_id = self.detail.account_id.clone().unwrap_or_default();
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
        let connected = self.is_account_connected(&account_id);
        let download = self.download_view(&account_id, item_id);
        let (work_id, copies) = self.work_of(&account_id, item_id);
        self.ui(|ui, s| {
            let mut data = detail_data(item_id, &detail, progress.as_ref(), cover, connected, s);
            data.account_id = account_id.as_str().into();
            data.can_download = connected && (detail.has_audio || detail.ebook_format.is_some());
            data.work_id = work_id as i32;
            data.copies = copies as i32;
            set_download(&mut data, download);
            ui.set_detail(data);
        });
        self.detail.shown = Some(detail);
    }

    /// Updates progress on the item page when it shows one of `item_ids`.
    pub(super) fn refresh_detail_progress(&mut self, account_id: &str, item_ids: &[String]) {
        let Some(item_id) = self.detail.item_id.clone() else {
            return;
        };
        if self.detail.account_id.as_deref() != Some(account_id) || !item_ids.contains(&item_id) {
            return;
        }
        if let Some(detail) = self.detail.shown.clone() {
            self.show_detail(&item_id, detail);
        }
    }

    /// The work of an item and how many copies it has; 0 and 0 without one.
    fn work_of(&self, account_id: &str, item_id: &str) -> (i64, usize) {
        let works = self.db.store().works();
        let Some(work_id) = works.work_of(account_id, item_id).ok().flatten() else {
            return (0, 0);
        };
        let copies = works.members(work_id).map_or(0, |m| m.len());
        (work_id, copies)
    }

    /// Updates the copies button after the works were matched again.
    pub(super) fn refresh_detail_work(&self) {
        let (Some(account_id), Some(item_id)) = (&self.detail.account_id, &self.detail.item_id)
        else {
            return;
        };
        let (work_id, copies) = self.work_of(account_id, item_id);
        self.ui(|ui, _| {
            let mut data = ui.get_detail();
            data.work_id = work_id as i32;
            data.copies = copies as i32;
            ui.set_detail(data);
        });
    }

    /// The account of an item the pages act on: the item page's, else the active one.
    pub(super) fn item_account(&self, item_id: &str) -> Option<String> {
        if self.detail.item_id.as_deref() == Some(item_id) {
            return self.detail.account_id.clone();
        }
        self.active.clone()
    }

    /// Updates the download controls when the item page shows this item.
    pub(super) fn refresh_detail_download(&self, account_id: &str, item_id: &str) {
        let shown = self.detail.item_id.as_deref() == Some(item_id)
            && self.detail.account_id.as_deref() == Some(account_id);
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
        if let Some(account_id) = self.item_account(item_id) {
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
        progress_text: if detail.has_audio {
            progress_text(progress, duration, s).into()
        } else {
            "".into()
        },
        progress: progress.map_or(0.0, |p| p.progress as f32),
        finished: progress.is_some_and(|p| p.is_finished),
        description: text(&detail.description),
        has_cover: cover.is_some(),
        cover: cover.unwrap_or_default(),
        has_audio: detail.has_audio,
        has_ebook: detail.ebook_format.is_some(),
        reading_text: reading_text(progress, s).into(),
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

fn reading_text(progress: Option<&ProgressRow>, s: &Strings<'_>) -> String {
    match progress.and_then(|p| p.ebook_progress) {
        Some(fraction) if fraction > 0.0 => s
            .invoke_read_percent((fraction * 100.0).round() as i32)
            .into(),
        _ => String::new(),
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
