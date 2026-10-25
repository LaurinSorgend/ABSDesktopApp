//! A tile grid: items flowed into rows, covers loaded for the visible rows.

use std::collections::HashMap;
use std::rc::Rc;

use abs_core::db::ItemSummary;
use slint::{Image, Model, ModelRc, VecModel};

use crate::covers::{Covers, TILE_WIDTH};
use crate::{TileData, TileRow};

const PREFETCH_ROWS: usize = 1;

/// Builds a tile's id from its item.
pub type TileKey = fn(&ItemSummary) -> String;

pub struct Grid {
    items: Vec<ItemSummary>,
    /// Positions of each item id; one id can appear for several accounts.
    index: HashMap<String, Vec<usize>>,
    key: TileKey,
    columns: usize,
    rows: Rc<VecModel<TileRow>>,
    row_tiles: Vec<Rc<VecModel<TileData>>>,
    visible: (usize, usize),
}

impl Grid {
    /// Tiles named by item id, for grids of one account.
    pub fn new() -> Self {
        Self::with_key(|item| item.remote_id.clone())
    }

    /// Tiles named by `key`, for grids that mix accounts.
    pub fn with_key(key: TileKey) -> Self {
        Self {
            items: Vec::new(),
            index: HashMap::new(),
            key,
            columns: 1,
            rows: Rc::new(VecModel::default()),
            row_tiles: Vec::new(),
            visible: (0, 4),
        }
    }

    pub fn model(&self) -> ModelRc<TileRow> {
        self.rows.clone().into()
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.items.iter().map(|i| i.remote_id.as_str())
    }

    /// Replaces the items and scrolls the cover window back to the top.
    pub fn set_items(&mut self, items: Vec<ItemSummary>, covers: &mut Covers) {
        self.index.clear();
        for (i, item) in items.iter().enumerate() {
            self.index
                .entry(item.remote_id.clone())
                .or_default()
                .push(i);
        }
        self.items = items;
        self.visible = (0, self.visible.1.saturating_sub(self.visible.0).max(4));
        self.reflow(covers);
    }

    /// Re-flows the rows for a new column count.
    pub fn set_columns(&mut self, columns: usize, covers: &mut Covers) {
        let columns = columns.max(1);
        if columns != self.columns {
            self.columns = columns;
            self.reflow(covers);
        }
    }

    /// Loads covers for rows `first..=last` and one row around them.
    pub fn show_rows(&mut self, first: usize, last: usize, covers: &mut Covers) {
        self.visible = (first, last);
        let start = first.saturating_sub(PREFETCH_ROWS) * self.columns;
        let end = ((last + PREFETCH_ROWS + 1) * self.columns).min(self.items.len());
        for index in start..end {
            let item = &self.items[index];
            if let Some(image) = covers.want(
                &item.account_id,
                &item.remote_id,
                item.cover_etag.as_deref(),
                TILE_WIDTH,
            ) {
                self.update_tile(index, |tile| {
                    tile.cover = image.clone();
                    tile.has_cover = true;
                });
            }
        }
    }

    /// Shows a cover that finished loading, or clears one that was evicted.
    pub fn set_cover(&self, item_id: &str, image: Option<&Image>) {
        for &index in self.index.get(item_id).into_iter().flatten() {
            self.update_tile(index, |tile| {
                tile.cover = image.cloned().unwrap_or_default();
                tile.has_cover = image.is_some();
            });
        }
    }

    /// Updates progress and titles of items that changed.
    pub fn update_items(&mut self, changed: &[ItemSummary]) {
        for item in changed {
            let positions = self.index.get(&item.remote_id).cloned().unwrap_or_default();
            for index in positions {
                if self.items[index].account_id == item.account_id {
                    self.update_item(index, item);
                }
            }
        }
    }

    fn update_item(&mut self, index: usize, item: &ItemSummary) {
        self.items[index] = item.clone();
        self.update_tile(index, |tile| {
            let fresh = tile_for(item, None);
            tile.title = fresh.title;
            tile.author = fresh.author;
            tile.progress = fresh.progress;
            tile.finished = fresh.finished;
            tile.downloaded = fresh.downloaded;
        });
    }

    fn reflow(&mut self, covers: &mut Covers) {
        self.row_tiles = self
            .items
            .chunks(self.columns)
            .map(|chunk| {
                Rc::new(VecModel::from(
                    chunk
                        .iter()
                        .map(|i| TileData {
                            id: (self.key)(i).into(),
                            ..tile_for(i, None)
                        })
                        .collect::<Vec<_>>(),
                ))
            })
            .collect();
        let rows: Vec<TileRow> = self
            .row_tiles
            .iter()
            .map(|tiles| TileRow {
                tiles: tiles.clone().into(),
            })
            .collect();
        self.rows.set_vec(rows);
        let (first, last) = self.visible;
        self.show_rows(first, last, covers);
    }

    fn update_tile(&self, index: usize, change: impl FnOnce(&mut TileData)) {
        let (row, column) = (index / self.columns, index % self.columns);
        let Some(tiles) = self.row_tiles.get(row) else {
            return;
        };
        if let Some(mut tile) = tiles.row_data(column) {
            change(&mut tile);
            tiles.set_row_data(column, tile);
        }
    }
}

/// The tile for an item, with a cover if one is at hand.
pub fn tile_for(item: &ItemSummary, cover: Option<Image>) -> TileData {
    TileData {
        id: item.remote_id.as_str().into(),
        title: item.title.as_str().into(),
        author: item.author.as_deref().unwrap_or_default().into(),
        has_cover: cover.is_some(),
        cover: cover.unwrap_or_default(),
        progress: item.progress as f32,
        finished: item.is_finished,
        downloaded: item.downloaded,
    }
}
