use serde::Deserialize;
use serde_json::Value;

use super::item::LibraryItem;

/// A series with its books.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Series {
    pub id: String,
    pub name: String,
    pub name_ignore_prefix: Option<String>,
    pub description: Option<String>,
    pub library_id: String,
    pub added_at: i64,
    pub updated_at: i64,
    pub books: Vec<LibraryItem>,
}

/// `GET /api/libraries/:id/authors`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Authors {
    pub authors: Vec<Author>,
}

/// An author in a library.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Author {
    pub id: String,
    pub name: String,
    pub asin: Option<String>,
    pub description: Option<String>,
    pub image_path: Option<String>,
    pub library_id: String,
    pub num_books: Option<u64>,
    pub last_first: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
}

/// A user-made collection of books.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub library_id: String,
    pub name: String,
    pub description: Option<String>,
    pub books: Vec<LibraryItem>,
    pub last_update: i64,
    pub created_at: i64,
}

/// `GET /api/libraries/:id/search`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SearchResults {
    pub book: Vec<BookMatch>,
    pub narrators: Vec<NarratorMatch>,
    pub series: Vec<SeriesMatch>,
    pub authors: Vec<Author>,
    pub tags: Vec<Value>,
    pub genres: Vec<Value>,
}

/// A book hit in search results.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BookMatch {
    pub library_item: LibraryItem,
    pub match_key: Option<String>,
    pub match_text: Option<String>,
}

/// A narrator hit in search results.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct NarratorMatch {
    pub name: String,
    pub num_books: u64,
}

/// A series hit in search results.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SeriesMatch {
    pub series: Series,
    pub books: Vec<LibraryItem>,
}

/// One shelf from `GET /api/libraries/:id/personalized`.
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawShelf")]
pub struct Shelf {
    pub id: String,
    pub label: String,
    pub label_string_key: Option<String>,
    pub total: u64,
    pub content: ShelfContent,
}

/// The entities on a shelf, by shelf type.
#[derive(Debug, Clone)]
pub enum ShelfContent {
    Books(Vec<LibraryItem>),
    Series(Vec<Series>),
    Authors(Vec<Author>),
    /// Podcast episodes and shelf types this client does not know yet.
    Other {
        shelf_type: String,
        entities: Vec<Value>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawShelf {
    id: String,
    #[serde(default)]
    label: String,
    label_string_key: Option<String>,
    #[serde(rename = "type")]
    shelf_type: String,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    entities: Vec<Value>,
}

impl TryFrom<RawShelf> for Shelf {
    type Error = serde_json::Error;

    fn try_from(raw: RawShelf) -> Result<Self, Self::Error> {
        let content = match raw.shelf_type.as_str() {
            "book" => ShelfContent::Books(decode_all(raw.entities)),
            "series" => ShelfContent::Series(decode_all(raw.entities)),
            "authors" => ShelfContent::Authors(decode_all(raw.entities)),
            _ => ShelfContent::Other {
                shelf_type: raw.shelf_type,
                entities: raw.entities,
            },
        };
        Ok(Self {
            id: raw.id,
            label: raw.label,
            label_string_key: raw.label_string_key,
            total: raw.total,
            content,
        })
    }
}

/// Decodes each entity on its own and skips those that do not fit, so one odd
/// item does not hide the whole shelf.
fn decode_all<T: serde::de::DeserializeOwned>(values: Vec<Value>) -> Vec<T> {
    values
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect()
}
