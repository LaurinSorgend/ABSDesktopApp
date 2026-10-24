//! Decoded cover images on the UI thread, loaded through the engine.

use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc;

use abs_core::engine::{Command, Engine};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

use crate::lru::ByteLru;

/// Cover width for tiles, about twice the tile size for high-DPI screens.
pub const TILE_WIDTH: u32 = 320;
/// Cover width for the item and player pages.
pub const LARGE_WIDTH: u32 = 800;
const DECODED_BYTES: usize = 96 * 1024 * 1024;

type Key = (String, u32);

/// A cover the decoder finished; `pixels` is `None` when there is no cover.
pub struct Decoded {
    pub item_id: String,
    pub width: u32,
    pub pixels: Option<SharedPixelBuffer<Rgba8Pixel>>,
}

pub struct Covers {
    engine: Rc<Engine>,
    images: ByteLru<Key, Image>,
    pending: HashSet<Key>,
    missing: HashSet<Key>,
}

impl Covers {
    pub fn new(engine: Rc<Engine>) -> Self {
        Self {
            engine,
            images: ByteLru::new(DECODED_BYTES),
            pending: HashSet::new(),
            missing: HashSet::new(),
        }
    }

    /// The decoded cover, or `None` while it loads or when the item has none.
    pub fn want(
        &mut self,
        account_id: &str,
        item_id: &str,
        etag: Option<&str>,
        width: u32,
    ) -> Option<Image> {
        let key = (item_id.to_owned(), width);
        if let Some(image) = self.images.get(&key) {
            return Some(image.clone());
        }
        if etag.is_none() || self.missing.contains(&key) || !self.pending.insert(key) {
            return None;
        }
        self.engine.send(Command::LoadCover {
            account_id: account_id.to_owned(),
            item_id: item_id.to_owned(),
            etag: etag.map(str::to_owned),
            width,
        });
        None
    }

    /// Stores a decoded cover. Returns the image and the item ids whose covers were dropped.
    pub fn loaded(&mut self, decoded: Decoded) -> (Option<Image>, Vec<String>) {
        let key = (decoded.item_id, decoded.width);
        self.pending.remove(&key);
        let Some(pixels) = decoded.pixels else {
            self.missing.insert(key);
            return (None, Vec::new());
        };
        let bytes = pixels.as_bytes().len();
        let image = Image::from_rgba8(pixels);
        let evicted = self.images.insert(key, image.clone(), bytes);
        (Some(image), evicted.into_iter().map(|(id, _)| id).collect())
    }

    /// Forgets everything, as after switching accounts.
    pub fn clear(&mut self) {
        self.images = ByteLru::new(DECODED_BYTES);
        self.pending.clear();
        self.missing.clear();
    }
}

/// Bytes from the engine, decoded off the UI thread.
pub struct Encoded {
    pub item_id: String,
    pub width: u32,
    pub bytes: Option<std::sync::Arc<Vec<u8>>>,
}

/// Starts the decoder thread; decoded covers go to `on_decoded` on that thread.
pub fn spawn_decoder(on_decoded: impl Fn(Decoded) + Send + 'static) -> mpsc::Sender<Encoded> {
    let (sender, receiver) = mpsc::channel::<Encoded>();
    std::thread::Builder::new()
        .name("cover-decoder".into())
        .spawn(move || {
            for encoded in receiver {
                let pixels = encoded.bytes.and_then(|bytes| decode(&bytes));
                on_decoded(Decoded {
                    item_id: encoded.item_id,
                    width: encoded.width,
                    pixels,
                });
            }
        })
        .expect("spawn cover decoder");
    sender
}

fn decode(bytes: &[u8]) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let rgba = image::load_from_memory(bytes).ok()?.into_rgba8();
    Some(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}
