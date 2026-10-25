//! The all-accounts page: books of the chosen libraries, one tile per work or per copy.

use std::collections::{HashMap, HashSet};

use abs_core::db::{BrowseQuery, ItemSort, ProgressFilter};
use slint::{ModelRc, VecModel};

use super::{App, Conn};
use crate::grid::Grid;
use crate::{LibraryChoice, Page};

const WORKS_MERGED: &str = "works_merged";
/// Libraries hidden on the page, one `account<TAB>library` per line, so new ones show up.
const WORKS_HIDDEN: &str = "works_hidden";
const SORTS: [ItemSort; 4] = [
    ItemSort::Title,
    ItemSort::Author,
    ItemSort::AddedAt,
    ItemSort::Duration,
];
const FILTERS: [ProgressFilter; 5] = [
    ProgressFilter::All,
    ProgressFilter::NotStarted,
    ProgressFilter::InProgress,
    ProgressFilter::Finished,
    ProgressFilter::Downloaded,
];

type LibraryKey = (String, String);

/// A library the page can show.
struct Choice {
    key: LibraryKey,
    label: String,
}

pub struct Works {
    pub grid: Grid,
    merged: bool,
    sort: usize,
    desc: bool,
    filter: usize,
    choices: Vec<Choice>,
    hidden: HashSet<LibraryKey>,
    /// Work and number of copies of each tile, by tile key.
    tiles: HashMap<String, (Option<i64>, i64)>,
    loaded: bool,
}

impl Works {
    pub fn new() -> Self {
        Self {
            grid: Grid::with_key(|item| tile_key(&item.account_id, &item.remote_id)),
            merged: true,
            sort: 0,
            desc: false,
            filter: 0,
            choices: Vec::new(),
            hidden: HashSet::new(),
            tiles: HashMap::new(),
            loaded: false,
        }
    }
}

/// `account/item`; ids are UUIDs, so the slash cannot be part of them.
pub fn tile_key(account_id: &str, item_id: &str) -> String {
    format!("{account_id}/{item_id}")
}

/// The account and item of a tile key, or `None` for a plain item id.
pub fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once('/')
}

impl App {
    /// Rebuilds the page from the database; only while it is shown.
    pub(super) fn refresh_works(&mut self) {
        if self.ui(|ui, _| ui.get_page()) != Some(Page::Works) {
            return;
        }
        self.load_works_view();
        self.works.choices = self.library_choices();
        let selected: Vec<LibraryKey> = self
            .works
            .choices
            .iter()
            .filter(|c| !self.works.hidden.contains(&c.key))
            .map(|c| c.key.clone())
            .collect();
        let query = BrowseQuery {
            libraries: &selected,
            merged: self.works.merged,
            sort: SORTS[self.works.sort],
            desc: self.works.desc,
            filter: FILTERS[self.works.filter],
        };
        let rows = self.db.store().works().browse(&query).unwrap_or_default();
        self.works.tiles = rows
            .iter()
            .map(|r| {
                let key = tile_key(&r.item.account_id, &r.item.remote_id);
                (key, (r.work_id, r.copies))
            })
            .collect();
        let count = rows.len() as i32;
        let items = rows.into_iter().map(|r| r.item).collect();
        self.works.grid.set_items(items, &mut self.covers);
        self.show_works_state(selected.is_empty(), count);
    }

    fn show_works_state(&self, nothing_selected: bool, count: i32) {
        let chips: Vec<LibraryChoice> = self
            .works
            .choices
            .iter()
            .map(|c| LibraryChoice {
                label: c.label.as_str().into(),
                selected: !self.works.hidden.contains(&c.key),
            })
            .collect();
        let works = &self.works;
        let (merged, sort, desc, filter) = (works.merged, works.sort, works.desc, works.filter);
        self.ui(|ui, s| {
            ui.set_works_libraries(ModelRc::new(VecModel::from(chips)));
            ui.set_works_merged(merged);
            ui.set_works_sort(sort as i32);
            ui.set_works_desc(desc);
            ui.set_works_filter(filter as i32);
            ui.set_works_note(if nothing_selected {
                s.invoke_no_libraries_chosen()
            } else if count == 0 {
                s.invoke_no_matches()
            } else {
                s.invoke_books(count)
            });
        });
    }

    /// Book libraries of the enabled accounts, oldest account first.
    fn library_choices(&self) -> Vec<Choice> {
        let catalog = self.db.store().catalog();
        let mut choices = Vec::new();
        for account in self.accounts.iter().filter(|a| a.enabled) {
            let place = self.account_place(account);
            for library in catalog.libraries(&account.id).unwrap_or_default() {
                if library.media_type == "book" {
                    choices.push(Choice {
                        key: (account.id.clone(), library.remote_id),
                        label: format!("{} · {place}", library.name),
                    });
                }
            }
        }
        choices
    }

    /// Reads the stored mode and hidden libraries once.
    fn load_works_view(&mut self) {
        if std::mem::replace(&mut self.works.loaded, true) {
            return;
        }
        let settings = self.db.store().settings();
        self.works.merged = settings.get_or(WORKS_MERGED, true).unwrap_or(true);
        let hidden = settings
            .get(WORKS_HIDDEN)
            .ok()
            .flatten()
            .unwrap_or_default();
        self.works.hidden = hidden
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(a, l)| (a.to_owned(), l.to_owned()))
            .collect();
    }

    fn save_works_view(&self) {
        let hidden: Vec<String> = self
            .works
            .hidden
            .iter()
            .map(|(a, l)| format!("{a}\t{l}"))
            .collect();
        let settings = self.db.store().settings();
        let _ = settings.set(WORKS_MERGED, self.works.merged);
        let _ = settings.set(WORKS_HIDDEN, hidden.join("\n"));
    }

    pub fn set_works_merged(&mut self, merged: bool) {
        self.works.merged = merged;
        self.save_works_view();
        self.refresh_works();
    }

    pub fn toggle_works_library(&mut self, index: usize) {
        let Some(key) = self.works.choices.get(index).map(|c| c.key.clone()) else {
            return;
        };
        if !self.works.hidden.remove(&key) {
            self.works.hidden.insert(key);
        }
        self.save_works_view();
        self.refresh_works();
    }

    pub fn set_works_sort(&mut self, index: usize) {
        self.works.sort = index.min(SORTS.len() - 1);
        self.refresh_works();
    }

    pub fn toggle_works_desc(&mut self) {
        self.works.desc = !self.works.desc;
        self.refresh_works();
    }

    pub fn set_works_filter(&mut self, index: usize) {
        self.works.filter = index.min(FILTERS.len() - 1);
        self.refresh_works();
    }

    /// A merged tile with several copies opens the work, anything else the item.
    pub fn open_works_tile(&mut self, key: &str) {
        let Some((account_id, item_id)) = split_key(key) else {
            return;
        };
        match self.works.tiles.get(key) {
            Some((Some(work_id), copies)) if self.works.merged && *copies > 1 => {
                self.open_work(*work_id)
            }
            _ => self.open_account_item(account_id, item_id),
        }
    }

    /// "user @ host", as the settings list the account.
    pub(super) fn account_place(&self, account: &abs_core::db::Account) -> String {
        format!("{} @ {}", account.username, self.account_host(account))
    }

    pub(super) fn is_account_connected(&self, account_id: &str) -> bool {
        self.conns.get(account_id) == Some(&Conn::Connected)
    }
}
