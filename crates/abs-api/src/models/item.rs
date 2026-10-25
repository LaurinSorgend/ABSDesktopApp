use serde::{Deserialize, Deserializer};

/// A library item, minified (list endpoints) or expanded (`?expanded=1`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryItem {
    pub id: String,
    pub ino: String,
    pub library_id: String,
    pub folder_id: String,
    pub path: String,
    pub rel_path: String,
    pub is_file: bool,
    pub is_missing: bool,
    pub is_invalid: bool,
    /// `book` or `podcast`.
    pub media_type: String,
    pub media: Media,
    pub added_at: i64,
    pub updated_at: i64,
    pub size: Option<u64>,
    pub num_files: Option<u64>,
    pub library_files: Vec<LibraryFile>,
}

/// The book part of an item. Podcast fields stay in the raw JSON.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Media {
    pub id: String,
    pub metadata: BookMetadata,
    pub cover_path: Option<String>,
    pub tags: Vec<String>,
    pub duration: Option<f64>,
    pub size: Option<u64>,
    pub num_tracks: Option<u64>,
    pub num_chapters: Option<u64>,
    pub chapters: Vec<Chapter>,
    pub tracks: Vec<AudioTrack>,
    pub ebook_file: Option<EbookFile>,
    /// The ebook's format in minified items, which have no `ebook_file`.
    pub ebook_format: Option<String>,
}

/// Book metadata. Minified items carry the `*_name` strings; expanded items the lists.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BookMetadata {
    pub title: Option<String>,
    pub title_ignore_prefix: Option<String>,
    pub subtitle: Option<String>,
    pub authors: Vec<NamedRef>,
    pub narrators: Vec<String>,
    /// A list, or one object on shelves such as "continue series".
    #[serde(deserialize_with = "one_or_many")]
    pub series: Vec<SeriesRef>,
    pub genres: Vec<String>,
    pub author_name: Option<String>,
    #[serde(rename = "authorNameLF")]
    pub author_name_lf: Option<String>,
    pub narrator_name: Option<String>,
    pub series_name: Option<String>,
    pub published_year: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub language: Option<String>,
    pub explicit: bool,
    pub abridged: bool,
}

/// An id and name, as used for authors in item metadata.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct NamedRef {
    pub id: String,
    pub name: String,
}

/// A series an item belongs to, with its position.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SeriesRef {
    pub id: String,
    pub name: String,
    pub sequence: Option<String>,
}

/// Accepts a list, a single value or `null`.
fn one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany<T> {
        Many(Vec<T>),
        One(T),
    }
    Ok(match Option::<OneOrMany<T>>::deserialize(deserializer)? {
        Some(OneOrMany::Many(values)) => values,
        Some(OneOrMany::One(value)) => vec![value],
        None => Vec::new(),
    })
}

/// A chapter in global item time (seconds).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Chapter {
    pub id: i64,
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// One playable audio file; `start_offset` is its position in global item time.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AudioTrack {
    pub index: i64,
    /// File id on the server; empty in playback sessions of old servers.
    pub ino: String,
    pub start_offset: f64,
    pub duration: f64,
    pub title: Option<String>,
    /// Path relative to the server root, such as `/api/items/<id>/file/<ino>`.
    pub content_url: String,
    pub mime_type: Option<String>,
    pub codec: Option<String>,
    pub metadata: Option<FileMetadata>,
}

/// The ebook file of a book.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EbookFile {
    pub ino: String,
    pub ebook_format: Option<String>,
    pub metadata: FileMetadata,
}

/// Any file in the item folder.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryFile {
    pub ino: String,
    pub file_type: Option<String>,
    pub metadata: FileMetadata,
}

/// File name, path and size on the server.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FileMetadata {
    pub filename: String,
    pub ext: String,
    pub path: String,
    pub rel_path: String,
    pub size: u64,
    pub mtime_ms: Option<i64>,
}
