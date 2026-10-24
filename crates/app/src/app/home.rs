//! Home: continue listening and recently added from local data, then the server's shelves.

use std::rc::Rc;

use abs_core::db::{ItemQuery, ItemSort, ItemSummary};
use abs_core::engine::{Command, Shelf};
use slint::{Image, Model, ModelRc, VecModel};

use super::App;
use crate::covers::TILE_WIDTH;
use crate::grid::tile_for;
use crate::{ShelfData, TileData};

const SHELF_SIZE: u64 = 12;
/// Server shelves the local ones already cover.
const LOCAL_SHELF_IDS: [&str; 2] = ["continue-listening", "recently-added"];

pub struct Home {
    shelves: Rc<VecModel<ShelfData>>,
    tiles: Vec<Rc<VecModel<TileData>>>,
    server: Vec<Shelf>,
}

impl Home {
    pub fn new() -> Self {
        Self {
            shelves: Rc::new(VecModel::default()),
            tiles: Vec::new(),
            server: Vec::new(),
        }
    }

    pub fn model(&self) -> ModelRc<ShelfData> {
        self.shelves.clone().into()
    }

    pub fn clear(&mut self) {
        self.server.clear();
        self.tiles.clear();
        self.shelves.set_vec(Vec::new());
    }

    pub fn set_cover(&self, item_id: &str, image: Option<&Image>) {
        let Some(image) = image else {
            return;
        };
        for tiles in &self.tiles {
            for row in 0..tiles.row_count() {
                if let Some(mut tile) = tiles.row_data(row)
                    && tile.id == item_id
                {
                    tile.cover = image.clone();
                    tile.has_cover = true;
                    tiles.set_row_data(row, tile);
                }
            }
        }
    }
}

impl App {
    pub(super) fn request_shelves(&self) {
        let (Some(account_id), Some(library_id)) = (self.active.clone(), self.library_id()) else {
            return;
        };
        if self.is_connected() {
            self.engine.send(Command::LoadShelves {
                account_id,
                library_id,
            });
        }
    }

    pub(super) fn shelves_loaded(&mut self, library_id: String, shelves: Vec<Shelf>) {
        if self.library_id().as_deref() == Some(library_id.as_str()) {
            self.home.server = shelves;
            self.refresh_home();
        }
    }

    pub(super) fn refresh_home(&mut self) {
        let mut shelves = self.local_shelves();
        let server: Vec<(String, String, Vec<String>)> = self
            .home
            .server
            .iter()
            .filter(|s| !LOCAL_SHELF_IDS.contains(&s.id.as_str()))
            .map(|s| (s.id.clone(), s.label.clone(), s.item_ids.clone()))
            .collect();
        for (id, label, item_ids) in server {
            let items = self.items_by_ids(&item_ids);
            shelves.push((id, label, items));
        }
        shelves.retain(|(_, _, items)| !items.is_empty());
        let empty = shelves.is_empty();
        self.home.tiles = shelves
            .iter()
            .map(|(_, _, items)| Rc::new(VecModel::from(self.tiles(items))))
            .collect();
        let rows: Vec<ShelfData> = shelves
            .iter()
            .zip(&self.home.tiles)
            .map(|((id, label, _), tiles)| ShelfData {
                id: id.as_str().into(),
                title: label.as_str().into(),
                tiles: tiles.clone().into(),
            })
            .collect();
        self.home.shelves.set_vec(rows);
        self.ui(|ui, s| {
            ui.set_home_note(if empty {
                s.invoke_home_empty()
            } else {
                "".into()
            });
        });
    }

    fn local_shelves(&self) -> Vec<(String, String, Vec<ItemSummary>)> {
        let Some(account_id) = self.active.as_deref() else {
            return Vec::new();
        };
        let store = self.db.store();
        let started: Vec<String> = store
            .progress()
            .in_progress(account_id, SHELF_SIZE)
            .unwrap_or_default()
            .into_iter()
            .map(|p| p.item_id)
            .collect();
        let recent = self.library_id().map(|library_id| {
            let query = ItemQuery {
                sort: ItemSort::AddedAt,
                desc: true,
                ..ItemQuery::new(&library_id)
            };
            store
                .catalog()
                .items_page(account_id, &query, 0, SHELF_SIZE)
                .unwrap_or_default()
        });
        let (continue_label, recent_label) = self
            .ui(|_, s| (s.invoke_continue_listening(), s.invoke_recently_added()))
            .unwrap_or_default();
        vec![
            (
                LOCAL_SHELF_IDS[0].to_owned(),
                continue_label.into(),
                self.items_by_ids(&started),
            ),
            (
                LOCAL_SHELF_IDS[1].to_owned(),
                recent_label.into(),
                recent.unwrap_or_default(),
            ),
        ]
    }

    pub(super) fn items_by_ids(&self, ids: &[String]) -> Vec<ItemSummary> {
        let Some(account_id) = self.active.as_deref() else {
            return Vec::new();
        };
        let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
        self.db
            .store()
            .catalog()
            .items_by_ids(account_id, &ids)
            .unwrap_or_default()
    }

    fn tiles(&mut self, items: &[ItemSummary]) -> Vec<TileData> {
        items
            .iter()
            .map(|item| {
                let cover = self.covers.want(
                    &item.account_id,
                    &item.remote_id,
                    item.cover_etag.as_deref(),
                    TILE_WIDTH,
                );
                tile_for(item, cover)
            })
            .collect()
    }
}
