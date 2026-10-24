use std::collections::HashSet;
use std::rc::Rc;

use slint::{Image, Model, ModelRc, SharedString, VecModel};
use tokio::sync::mpsc::UnboundedSender;

use crate::cache::ByteLru;
use crate::loader::{GridItem, LoadedCover};
use crate::{Row, Tile};

const DECODED_CACHE_BYTES: usize = 64 * 1024 * 1024;
const PREFETCH_ROWS: usize = 1;

/// Owns the row models and decides which covers to load or drop.
pub struct GridState {
    items: Vec<GridItem>,
    columns: usize,
    rows: Rc<VecModel<Row>>,
    row_tiles: Vec<Rc<VecModel<Tile>>>,
    cache: ByteLru<Image>,
    pending: HashSet<usize>,
    visible_rows: (usize, usize),
    requests: UnboundedSender<(usize, String)>,
}

impl GridState {
    /// Creates the state with one column; call [`GridState::set_columns`] once the window knows its width.
    pub fn new(items: Vec<GridItem>, requests: UnboundedSender<(usize, String)>) -> Self {
        let mut state = Self {
            items,
            columns: 0,
            rows: Rc::new(VecModel::default()),
            row_tiles: Vec::new(),
            cache: ByteLru::new(DECODED_CACHE_BYTES),
            pending: HashSet::new(),
            visible_rows: (0, 0),
            requests,
        };
        state.set_columns(1);
        state
    }

    /// The model bound to the window's `rows` property.
    pub fn rows_model(&self) -> ModelRc<Row> {
        self.rows.clone().into()
    }

    /// Text for the status line.
    pub fn status(&self) -> SharedString {
        let mb = self.cache.used() as f64 / (1024.0 * 1024.0);
        format!(
            "{} items, {} columns, decoded cache {mb:.1} MB, {} loading",
            self.items.len(),
            self.columns,
            self.pending.len()
        )
        .into()
    }

    /// Re-flows the items into rows of `columns` tiles.
    pub fn set_columns(&mut self, columns: usize) {
        let columns = columns.max(1);
        if columns == self.columns {
            return;
        }
        self.columns = columns;
        self.row_tiles = self
            .items
            .chunks(columns)
            .enumerate()
            .map(|(row, chunk)| self.tiles_for(row * columns, chunk))
            .collect();
        let rows: Vec<Row> = self
            .row_tiles
            .iter()
            .map(|tiles| Row {
                tiles: tiles.clone().into(),
            })
            .collect();
        self.rows.set_vec(rows);
        let (first, last) = self.visible_rows;
        self.show_rows(first, last);
    }

    fn tiles_for(&self, first: usize, chunk: &[GridItem]) -> Rc<VecModel<Tile>> {
        let tiles = chunk.iter().enumerate().map(|(offset, item)| {
            let index = first + offset;
            Tile {
                index: index as i32,
                title: item.title.as_str().into(),
                cover: self.cache.peek(index).cloned().unwrap_or_default(),
            }
        });
        Rc::new(VecModel::from(tiles.collect::<Vec<_>>()))
    }

    /// Requests covers for the visible rows plus a small prefetch margin.
    pub fn show_rows(&mut self, first: usize, last: usize) {
        self.visible_rows = (first, last);
        let first_item = first.saturating_sub(PREFETCH_ROWS) * self.columns;
        let last_item = ((last + PREFETCH_ROWS + 1) * self.columns).min(self.items.len());
        for index in first_item..last_item {
            if self.cache.touch(index) || self.pending.contains(&index) {
                continue;
            }
            self.pending.insert(index);
            let _ = self.requests.send((index, self.items[index].id.clone()));
        }
    }

    /// Stores a decoded cover, shows it and clears the tiles that were evicted.
    pub fn cover_loaded(&mut self, cover: LoadedCover) {
        self.pending.remove(&cover.index);
        let bytes = cover.pixels.as_bytes().len();
        let image = Image::from_rgba8(cover.pixels);
        self.set_tile_cover(cover.index, image.clone());
        for evicted in self.cache.insert(cover.index, image, bytes) {
            self.set_tile_cover(evicted, Image::default());
        }
    }

    fn set_tile_cover(&self, index: usize, cover: Image) {
        let (row, column) = (index / self.columns, index % self.columns);
        let Some(tiles) = self.row_tiles.get(row) else {
            return;
        };
        if let Some(mut tile) = tiles.row_data(column) {
            tile.cover = cover;
            tiles.set_row_data(column, tile);
        }
    }
}
