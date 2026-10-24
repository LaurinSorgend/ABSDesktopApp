//! Downloads: the controls on the item page and the list and folder in the settings.

use std::path::PathBuf;
use std::rc::Rc;

use abs_core::db::{DownloadRow, DownloadState};
use abs_core::downloads::ROOT_SETTING;
use abs_core::engine::Command;
use slint::{Model, ModelRc, SharedString, VecModel};

use super::App;
use crate::{DownloadData, Strings, paths};

/// The downloads list on the settings page.
pub struct Downloads {
    rows: Rc<VecModel<DownloadData>>,
}

impl Downloads {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(VecModel::default()),
        }
    }

    pub fn model(&self) -> ModelRc<DownloadData> {
        self.rows.clone().into()
    }
}

/// How the UI shows one download: state number, text and fraction done.
pub struct DownloadView {
    pub state: i32,
    pub text: SharedString,
    pub fraction: f32,
}

impl DownloadView {
    pub fn none() -> Self {
        Self {
            state: 0,
            text: SharedString::new(),
            fraction: 0.0,
        }
    }

    pub fn of(row: &DownloadRow, s: &Strings<'_>) -> Self {
        let fraction = if row.size > 0 {
            (row.bytes_done as f64 / row.size as f64).min(1.0)
        } else {
            0.0
        };
        let percent = (fraction * 100.0).floor() as i32;
        let (state, text) = match row.state {
            DownloadState::Queued => (1, s.invoke_download_queued()),
            DownloadState::Running => (
                2,
                s.invoke_download_running(percent, size_text(row.size, s)),
            ),
            DownloadState::Paused => (3, s.invoke_download_paused(percent)),
            DownloadState::Done => (4, s.invoke_download_done(size_text(row.size, s))),
            DownloadState::Failed => (
                5,
                s.invoke_download_failed(row.error.clone().unwrap_or_default().into()),
            ),
        };
        Self {
            state,
            text,
            fraction: fraction as f32,
        }
    }
}

impl App {
    pub fn download(&mut self, account_id: &str, item_id: &str) {
        self.engine.send(Command::Download {
            account_id: self.account_or_active(account_id),
            item_id: item_id.to_owned(),
        });
    }

    pub fn pause_download(&mut self, account_id: &str, item_id: &str) {
        self.engine.send(Command::PauseDownload {
            account_id: self.account_or_active(account_id),
            item_id: item_id.to_owned(),
        });
    }

    pub fn resume_download(&mut self, account_id: &str, item_id: &str) {
        self.engine.send(Command::ResumeDownload {
            account_id: self.account_or_active(account_id),
            item_id: item_id.to_owned(),
        });
    }

    pub fn delete_download(&mut self, account_id: &str, item_id: &str) {
        self.engine.send(Command::DeleteDownload {
            account_id: self.account_or_active(account_id),
            item_id: item_id.to_owned(),
        });
    }

    /// Asks for a folder and makes it the place for new downloads.
    pub fn choose_download_folder(&mut self) {
        let picked = rfd::FileDialog::new()
            .set_directory(self.download_folder())
            .pick_folder();
        if let Some(path) = picked {
            let _ = self
                .db
                .store()
                .settings()
                .set(ROOT_SETTING, path.to_string_lossy());
            self.refresh_downloads();
        }
    }

    pub fn reset_download_folder(&mut self) {
        let _ = self.db.store().settings().remove(ROOT_SETTING);
        self.refresh_downloads();
    }

    /// The folder new downloads go to.
    fn download_folder(&self) -> PathBuf {
        self.custom_download_folder()
            .unwrap_or_else(paths::download_dir)
    }

    fn custom_download_folder(&self) -> Option<PathBuf> {
        let value = self.db.store().settings().get(ROOT_SETTING).ok().flatten();
        value.map(PathBuf::from)
    }

    /// A download was queued, finished, removed or changed state.
    pub(super) fn downloads_changed(&mut self, account_id: &str, item_id: &str) {
        self.refresh_downloads();
        self.progress_changed(account_id, &[item_id.to_owned()]);
        self.refresh_detail_download(account_id, item_id);
    }

    /// Bytes arrived for a running download.
    pub(super) fn download_progress(&mut self, account_id: &str, item_id: &str) {
        let row = self
            .db
            .store()
            .downloads()
            .get(account_id, item_id)
            .ok()
            .flatten();
        let Some(row) = row else {
            return;
        };
        let rows = self.downloads.rows.clone();
        let index = rows
            .iter()
            .position(|d| d.account_id == account_id && d.item_id == item_id);
        self.ui(|_, s| {
            if let Some(index) = index {
                rows.set_row_data(index, download_data(&row, s));
            }
        });
        self.refresh_detail_download(account_id, item_id);
    }

    /// The list, folder and space used on the settings page.
    pub(super) fn refresh_downloads(&self) {
        let rows = self.db.store().downloads().list().unwrap_or_default();
        let used: u64 = rows.iter().map(|r| r.bytes_done).sum();
        let folder = self.download_folder();
        let custom = self.custom_download_folder().is_some();
        let model = self.downloads.rows.clone();
        self.ui(|ui, s| {
            model.set_vec(rows.iter().map(|r| download_data(r, s)).collect::<Vec<_>>());
            ui.set_download_folder(folder.to_string_lossy().as_ref().into());
            ui.set_download_folder_custom(custom);
            ui.set_downloads_used_text(size_text(used, s));
        });
    }

    /// The download state of an item, for the item page.
    pub(super) fn download_view(&self, account_id: &str, item_id: &str) -> DownloadView {
        let row = self
            .db
            .store()
            .downloads()
            .get(account_id, item_id)
            .ok()
            .flatten();
        self.ui(|_, s| row.as_ref().map(|r| DownloadView::of(r, s)))
            .flatten()
            .unwrap_or_else(DownloadView::none)
    }

    /// An empty account id from the item page means the active account.
    fn account_or_active(&self, account_id: &str) -> String {
        if account_id.is_empty() {
            self.active.clone().unwrap_or_default()
        } else {
            account_id.to_owned()
        }
    }
}

fn download_data(row: &DownloadRow, s: &Strings<'_>) -> DownloadData {
    let view = DownloadView::of(row, s);
    DownloadData {
        account_id: row.account_id.as_str().into(),
        item_id: row.item_id.as_str().into(),
        title: row.title.as_str().into(),
        status: view.text,
        state: view.state,
        fraction: view.fraction,
    }
}

/// `512 MB` or `1.4 GB`.
pub fn size_text(bytes: u64, s: &Strings<'_>) -> SharedString {
    let megabytes = bytes as f64 / (1024.0 * 1024.0);
    if megabytes < 1024.0 {
        s.invoke_megabytes(megabytes.round() as i32)
    } else {
        s.invoke_gigabytes(format!("{:.1}", megabytes / 1024.0).into())
    }
}
