use abs_api::Raw;
use abs_api::models::{BookMetadata, LibraryItem};

use super::ItemRow;

impl ItemRow {
    /// Builds the row from a minified or expanded server item.
    pub fn from_api(item: &Raw<LibraryItem>) -> Self {
        let media = &item.media;
        let meta = &media.metadata;
        let title = meta.title.clone().unwrap_or_default();
        Self {
            remote_id: item.id.clone(),
            library_id: item.library_id.clone(),
            title_sort: meta
                .title_ignore_prefix
                .clone()
                .unwrap_or_else(|| title.clone()),
            title,
            author: author(meta),
            narrator: narrator(meta),
            series: series(meta),
            asin: meta.asin.clone(),
            isbn: meta.isbn.clone(),
            duration: media.duration,
            cover_etag: media
                .cover_path
                .as_ref()
                .map(|_| item.updated_at.to_string()),
            added_at: item.added_at,
            updated_at: item.updated_at,
            raw_json: item.json().to_owned(),
        }
    }
}

fn author(meta: &BookMetadata) -> Option<String> {
    non_empty(meta.author_name.clone())
        .or_else(|| join(meta.authors.iter().map(|a| a.name.clone())))
}

fn narrator(meta: &BookMetadata) -> Option<String> {
    non_empty(meta.narrator_name.clone()).or_else(|| join(meta.narrators.iter().cloned()))
}

fn series(meta: &BookMetadata) -> Option<String> {
    non_empty(meta.series_name.clone()).or_else(|| {
        join(meta.series.iter().map(|s| match &s.sequence {
            Some(sequence) => format!("{} #{sequence}", s.name),
            None => s.name.clone(),
        }))
    })
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.trim().is_empty())
}

fn join(values: impl Iterator<Item = String>) -> Option<String> {
    non_empty(Some(values.collect::<Vec<_>>().join(", ")))
}
