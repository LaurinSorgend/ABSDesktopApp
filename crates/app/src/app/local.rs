//! Local folders: the list in the settings and adding, removing and scanning them.

use std::collections::HashMap;
use std::rc::Rc;

use abs_core::db::{ItemQuery, LocalFolder};
use abs_core::engine::Command;
use abs_core::local::ScanReport;
use slint::{ModelRc, VecModel};

use super::App;
use crate::{LocalFolderData, Strings};

/// The folder list on the settings page.
pub struct LocalFolders {
    rows: Rc<VecModel<LocalFolderData>>,
    /// Books done and in total of running scans, by library.
    scans: HashMap<String, (usize, usize)>,
}

impl LocalFolders {
    pub fn new() -> Self {
        Self {
            rows: Rc::new(VecModel::default()),
            scans: HashMap::new(),
        }
    }

    pub fn model(&self) -> ModelRc<LocalFolderData> {
        self.rows.clone().into()
    }
}

impl App {
    /// Asks for a folder and adds it.
    pub fn add_local_folder(&mut self) {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            self.engine.send(Command::AddLocalFolder { path });
        }
    }

    pub fn remove_local_folder(&mut self, library_id: &str) {
        self.engine.send(Command::RemoveLocalFolder {
            library_id: library_id.to_owned(),
        });
    }

    pub fn rescan_local_folder(&mut self, library_id: &str) {
        self.engine.send(Command::RescanLocalFolder {
            library_id: library_id.to_owned(),
        });
    }

    pub(super) fn local_scan_progress(&mut self, library_id: String, done: usize, total: usize) {
        self.local_folders.scans.insert(library_id, (done, total));
        self.refresh_local_folders();
    }

    pub(super) fn local_scan_finished(&mut self, library_id: &str, report: ScanReport) {
        self.local_folders.scans.remove(library_id);
        self.refresh_local_folders();
        if report != ScanReport::default() {
            self.refresh_home();
        }
    }

    pub(super) fn refresh_local_folders(&self) {
        let folders = self.db.store().local().folders().unwrap_or_default();
        let rows = self.local_folders.rows.clone();
        let list: Vec<LocalFolderData> = self
            .ui(|_, s| folders.iter().map(|f| self.folder_row(f, s)).collect())
            .unwrap_or_default();
        rows.set_vec(list);
    }

    fn folder_row(&self, folder: &LocalFolder, s: &Strings<'_>) -> LocalFolderData {
        let scan = self.local_folders.scans.get(&folder.library_id);
        let status = match (scan, folder.available) {
            (Some((done, total)), _) => s.invoke_local_scanning(*done as i32, *total as i32),
            (None, false) => s.invoke_local_unreachable(),
            (None, true) => {
                let query = ItemQuery::new(&folder.library_id);
                let catalog = self.db.store().catalog();
                let count = catalog.item_count(&folder.account_id, &query);
                s.invoke_books(count.unwrap_or_default() as i32)
            }
        };
        LocalFolderData {
            library_id: folder.library_id.as_str().into(),
            path: folder.path.to_string_lossy().as_ref().into(),
            status,
            available: folder.available,
            scanning: scan.is_some(),
        }
    }

    /// True while the active account is the one of the local folders.
    pub(super) fn is_local_active(&self) -> bool {
        self.active_account().is_some_and(|a| a.local)
    }

    /// True when an account has its books in local folders.
    pub(super) fn is_local_account(&self, account_id: &str) -> bool {
        self.accounts.iter().any(|a| a.id == account_id && a.local)
    }
}
