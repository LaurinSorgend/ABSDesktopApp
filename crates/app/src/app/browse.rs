//! Libraries, the library grid with sort and filter, series/authors/collections and search.

use std::collections::HashMap;

use abs_core::db::{ItemQuery, ItemSort, ItemSummary, ProgressFilter};
use abs_core::engine::{Command, Group, GroupKind};
use slint::{Image, ModelRc, VecModel};

use super::App;
use crate::grid::Grid;
use crate::{GroupData, LibraryView, NamedId, Page};

const ALL: u64 = 1_000_000;
const SEARCH_LIMIT: u64 = 200;
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

pub struct Browse {
    pub library_grid: Grid,
    pub group_grid: Grid,
    pub search_grid: Grid,
    view: LibraryView,
    sort: usize,
    desc: bool,
    filter: usize,
    groups: HashMap<(String, GroupKind), Vec<Group>>,
    waiting_for_groups: bool,
    open_group: Option<String>,
    search_text: String,
}

impl Browse {
    pub fn new() -> Self {
        Self {
            library_grid: Grid::new(),
            group_grid: Grid::new(),
            search_grid: Grid::new(),
            view: LibraryView::Items,
            sort: 0,
            desc: false,
            filter: 0,
            groups: HashMap::new(),
            waiting_for_groups: false,
            open_group: None,
            search_text: String::new(),
        }
    }

    pub fn clear(&mut self) {
        self.groups.clear();
        self.open_group = None;
        self.view = LibraryView::Items;
    }

    pub fn set_cover(&self, item_id: &str, image: Option<&Image>) {
        self.library_grid.set_cover(item_id, image);
        self.group_grid.set_cover(item_id, image);
        self.search_grid.set_cover(item_id, image);
    }

    fn grid(&mut self, name: &str) -> Option<&mut Grid> {
        match name {
            "library" => Some(&mut self.library_grid),
            "group" => Some(&mut self.group_grid),
            "search" => Some(&mut self.search_grid),
            _ => None,
        }
    }
}

fn kind_of(view: LibraryView) -> Option<GroupKind> {
    match view {
        LibraryView::Series => Some(GroupKind::Series),
        LibraryView::Authors => Some(GroupKind::Authors),
        LibraryView::Collections => Some(GroupKind::Collections),
        _ => None,
    }
}

impl App {
    pub(super) fn load_libraries(&mut self) {
        let Some(account_id) = self.active.clone() else {
            return;
        };
        let current = self.library_id();
        self.libraries = self
            .db
            .store()
            .catalog()
            .libraries(&account_id)
            .unwrap_or_default();
        self.library = current
            .and_then(|id| self.libraries.iter().position(|l| l.remote_id == id))
            .or_else(|| (!self.libraries.is_empty()).then_some(0));
        let names: Vec<NamedId> = self
            .libraries
            .iter()
            .map(|l| NamedId {
                id: l.remote_id.as_str().into(),
                name: l.name.as_str().into(),
            })
            .collect();
        let index = self.library.map_or(-1, |i| i as i32);
        self.ui(|ui, _| {
            ui.set_libraries(ModelRc::new(VecModel::from(names)));
            ui.set_library_index(index);
        });
        self.load_library_items();
    }

    pub fn select_library(&mut self, index: usize) {
        if index >= self.libraries.len() {
            return;
        }
        let changed = self.library != Some(index);
        self.library = Some(index);
        self.ui(|ui, _| ui.set_library_index(index as i32));
        if changed {
            self.home.clear();
            self.load_library_items();
            self.request_shelves();
            self.set_library_view(LibraryView::Items);
        }
        self.navigate(Page::Library);
    }

    fn load_library_items(&mut self) {
        let (Some(account_id), Some(library_id)) = (self.active.clone(), self.library_id()) else {
            self.browse
                .library_grid
                .set_items(Vec::new(), &mut self.covers);
            return;
        };
        let query = ItemQuery {
            sort: SORTS[self.browse.sort],
            desc: self.browse.desc,
            filter: FILTERS[self.browse.filter],
            ..ItemQuery::new(&library_id)
        };
        let items = self
            .db
            .store()
            .catalog()
            .items_page(&account_id, &query, 0, ALL)
            .unwrap_or_default();
        let count = items.len();
        self.browse.library_grid.set_items(items, &mut self.covers);
        let (sort, desc, filter) = (self.browse.sort, self.browse.desc, self.browse.filter);
        self.ui(|ui, s| {
            ui.set_library_sort(sort as i32);
            ui.set_library_desc(desc);
            ui.set_library_filter(filter as i32);
            ui.set_library_note(if count == 0 {
                s.invoke_no_matches()
            } else {
                s.invoke_books(count as i32)
            });
        });
    }

    pub fn set_sort(&mut self, index: usize) {
        self.browse.sort = index.min(SORTS.len() - 1);
        self.load_library_items();
    }

    pub fn toggle_desc(&mut self) {
        self.browse.desc = !self.browse.desc;
        self.load_library_items();
    }

    pub fn set_filter(&mut self, index: usize) {
        self.browse.filter = index.min(FILTERS.len() - 1);
        self.load_library_items();
    }

    pub fn grid_columns(&mut self, name: &str, columns: usize) {
        if let Some(grid) = self.browse.grid(name) {
            grid.set_columns(columns, &mut self.covers);
        }
    }

    pub fn grid_visible(&mut self, name: &str, first: usize, last: usize) {
        if let Some(grid) = self.browse.grid(name) {
            grid.show_rows(first, last, &mut self.covers);
        }
    }

    pub fn set_library_view(&mut self, view: LibraryView) {
        self.browse.view = view;
        self.ui(|ui, _| ui.set_library_view(view));
        let Some(kind) = kind_of(view) else {
            return;
        };
        let (Some(account_id), Some(library_id)) = (self.active.clone(), self.library_id()) else {
            return;
        };
        if let Some(groups) = self.browse.groups.get(&(library_id.clone(), kind)) {
            let groups = groups.clone();
            self.show_groups(&groups);
            return;
        }
        self.show_groups(&[]);
        let note = if self.is_connected() {
            self.browse.waiting_for_groups = true;
            self.engine.send(Command::LoadGroups {
                account_id,
                library_id,
                kind,
            });
            self.ui(|_, s| s.invoke_loading())
        } else {
            self.ui(|_, s| s.invoke_needs_connection())
        };
        self.ui(|ui, _| ui.set_groups_note(note.unwrap_or_default()));
    }

    pub(super) fn groups_loaded(
        &mut self,
        library_id: String,
        kind: GroupKind,
        groups: Vec<Group>,
    ) {
        self.browse.waiting_for_groups = false;
        let current = self.library_id().as_deref() == Some(library_id.as_str())
            && kind_of(self.browse.view) == Some(kind);
        if current {
            self.show_groups(&groups);
        }
        self.browse.groups.insert((library_id, kind), groups);
    }

    fn show_groups(&self, groups: &[Group]) {
        let rows: Vec<GroupData> = groups
            .iter()
            .map(|g| GroupData {
                id: g.id.as_str().into(),
                name: g.name.as_str().into(),
                count: g.count.to_string().into(),
            })
            .collect();
        let empty = rows.is_empty();
        self.ui(|ui, s| {
            ui.set_groups(ModelRc::new(VecModel::from(rows)));
            ui.set_groups_note(if empty {
                s.invoke_no_matches()
            } else {
                "".into()
            });
        });
    }

    pub fn open_group(&mut self, group_id: &str) {
        let Some(kind) = kind_of(self.browse.view) else {
            return;
        };
        let Some(library_id) = self.library_id() else {
            return;
        };
        let Some(group) = self
            .browse
            .groups
            .get(&(library_id.clone(), kind))
            .and_then(|groups| groups.iter().find(|g| g.id == group_id))
            .cloned()
        else {
            return;
        };
        self.browse.open_group = Some(group.id.clone());
        self.ui(|ui, _| ui.set_group_title(group.name.as_str().into()));
        if kind == GroupKind::Authors {
            self.show_group_items(&[]);
            if let Some(account_id) = self.active.clone() {
                self.engine.send(Command::LoadAuthorItems {
                    account_id,
                    library_id,
                    author_id: group.id.clone(),
                });
            }
            let loading = self.ui(|_, s| s.invoke_loading()).unwrap_or_default();
            self.ui(|ui, _| ui.set_group_note(loading));
        } else {
            self.show_group_items(&group.item_ids);
        }
        self.navigate(Page::Group);
    }

    pub(super) fn group_items(&mut self, group_id: &str, item_ids: &[String]) {
        if self.browse.open_group.as_deref() == Some(group_id) {
            self.show_group_items(item_ids);
        }
    }

    fn show_group_items(&mut self, item_ids: &[String]) {
        let items = self.items_by_ids(item_ids);
        let count = items.len() as i32;
        self.browse.group_grid.set_items(items, &mut self.covers);
        self.ui(|ui, s| ui.set_group_note(s.invoke_books(count)));
    }

    pub(super) fn browse_request_failed(&mut self) {
        if std::mem::take(&mut self.browse.waiting_for_groups) {
            let note = self
                .ui(|_, s| s.invoke_needs_connection())
                .unwrap_or_default();
            self.ui(|ui, _| ui.set_groups_note(note));
        }
    }
}

/// Search: the local index on every keystroke, the server on request.
impl App {
    pub fn search(&mut self, text: &str) {
        self.browse.search_text = text.to_owned();
        let items = match self.active.as_deref() {
            Some(account_id) => self
                .db
                .store()
                .catalog()
                .search(Some(account_id), text, SEARCH_LIMIT)
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let count = items.len() as i32;
        let blank = text.trim().is_empty();
        self.browse.search_grid.set_items(items, &mut self.covers);
        self.ui(|ui, s| {
            ui.set_search_note(if blank {
                "".into()
            } else if count == 0 {
                s.invoke_no_matches()
            } else {
                s.invoke_local_results(count)
            })
        });
        self.update_search_offer();
    }

    pub fn search_server(&mut self) {
        let (Some(account_id), Some(library_id)) = (self.active.clone(), self.library_id()) else {
            return;
        };
        let text = self.browse.search_text.trim().to_owned();
        if !text.is_empty() {
            self.engine.send(Command::SearchServer {
                account_id,
                library_id,
                text,
            });
        }
    }

    pub(super) fn server_results(&mut self, text: &str, item_ids: Vec<String>) {
        if self.browse.search_text.trim() != text {
            return;
        }
        let mut ids: Vec<String> = self.browse.search_grid.ids().map(str::to_owned).collect();
        for id in item_ids {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let items = self.items_by_ids(&ids);
        let count = items.len() as i32;
        self.browse.search_grid.set_items(items, &mut self.covers);
        self.ui(|ui, s| {
            ui.set_search_note(if count == 0 {
                s.invoke_no_matches()
            } else {
                s.invoke_server_results(count)
            });
            ui.set_search_server_offered(false);
        });
    }

    pub(super) fn update_search_offer(&self) {
        let offered = self.is_connected() && !self.browse.search_text.trim().is_empty();
        self.ui(|ui, _| ui.set_search_server_offered(offered));
    }

    /// The catalog of an account changed: reload what the pages show.
    pub(super) fn catalog_changed(&mut self, account_id: &str) {
        self.set_syncing(account_id, false);
        if self.active.as_deref() != Some(account_id) {
            return;
        }
        let had_library = self.library.is_some();
        self.load_libraries();
        if !had_library {
            self.request_shelves();
        }
        self.refresh_home();
        let text = self.browse.search_text.clone();
        if !text.is_empty() {
            self.search(&text);
        }
    }

    /// Progress changed on this or another device.
    pub(super) fn progress_changed(&mut self, account_id: &str, item_ids: &[String]) {
        if self.active.as_deref() != Some(account_id) {
            return;
        }
        let items: Vec<ItemSummary> = self.items_by_ids(item_ids);
        self.browse.library_grid.update_items(&items);
        self.browse.group_grid.update_items(&items);
        self.browse.search_grid.update_items(&items);
        self.refresh_home();
        self.refresh_detail_progress(item_ids);
    }
}
