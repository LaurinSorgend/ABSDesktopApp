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
/// Size of the blurred cover behind the player; it is scaled up, so small is enough.
const BLURRED_SIZE: u32 = 96;
/// Cache key width of a blurred cover.
const BLURRED: u32 = 0;
const DECODED_BYTES: usize = 96 * 1024 * 1024;

type Key = (String, u32);

/// A cover the decoder finished; `pixels` is `None` when there is no cover.
/// Large covers come with a blurred copy.
pub struct Decoded {
    pub item_id: String,
    pub width: u32,
    pub pixels: Option<SharedPixelBuffer<Rgba8Pixel>>,
    pub blurred: Option<SharedPixelBuffer<Rgba8Pixel>>,
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
        let mut evicted = Vec::new();
        if let Some(blurred) = decoded.blurred {
            let bytes = blurred.as_bytes().len();
            let blurred_key = (key.0.clone(), BLURRED);
            evicted.extend(
                self.images
                    .insert(blurred_key, Image::from_rgba8(blurred), bytes),
            );
        }
        let bytes = pixels.as_bytes().len();
        let image = Image::from_rgba8(pixels);
        evicted.extend(self.images.insert(key, image.clone(), bytes));
        (Some(image), evicted.into_iter().map(|(id, _)| id).collect())
    }

    /// The blurred copy of a large cover that was decoded.
    pub fn blurred(&mut self, item_id: &str) -> Option<Image> {
        self.images.get(&(item_id.to_owned(), BLURRED)).cloned()
    }

    /// Forgets everything, as after switching accounts.
    pub fn clear(&mut self) {
        self.images = ByteLru::new(DECODED_BYTES);
        self.pending.clear();
        self.missing.clear();
    }
}

/// The hue in degrees of a title's generated cover, so a book keeps its color.
pub fn title_hue(title: &str) -> f32 {
    let hash = title.bytes().fold(0x811c_9dc5_u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    });
    (hash % 360) as f32
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
                let decoded = encoded
                    .bytes
                    .and_then(|bytes| image::load_from_memory(&bytes).ok());
                let blurred = decoded
                    .as_ref()
                    .filter(|_| encoded.width == LARGE_WIDTH)
                    .map(blur);
                on_decoded(Decoded {
                    item_id: encoded.item_id,
                    width: encoded.width,
                    pixels: decoded.map(|image| pixels(&image.into_rgba8())),
                    blurred,
                });
            }
        })
        .expect("spawn cover decoder");
    sender
}

fn pixels(rgba: &image::RgbaImage) -> SharedPixelBuffer<Rgba8Pixel> {
    SharedPixelBuffer::clone_from_slice(rgba.as_raw(), rgba.width(), rgba.height())
}

/// A small, heavily blurred copy, for a background that is scaled up.
fn blur(cover: &image::DynamicImage) -> SharedPixelBuffer<Rgba8Pixel> {
    let small = cover
        .resize_to_fill(
            BLURRED_SIZE,
            BLURRED_SIZE,
            image::imageops::FilterType::Triangle,
        )
        .into_rgba8();
    pixels(&image::imageops::blur(&small, 6.0))
}

#[cfg(test)]
mod tests {
    use super::title_hue;

    #[test]
    fn a_title_keeps_its_hue_and_titles_differ() {
        assert_eq!(title_hue("Emma"), title_hue("Emma"));
        assert_ne!(title_hue("Emma"), title_hue("Dracula"));
        assert!((0.0..360.0).contains(&title_hue("")));
    }
}
