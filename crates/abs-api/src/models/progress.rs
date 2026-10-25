use serde::{Deserialize, Deserializer, Serialize};

/// Listening or reading progress of one item (or podcast episode).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaProgress {
    pub id: String,
    pub library_item_id: String,
    pub episode_id: Option<String>,
    pub duration: Option<f64>,
    pub progress: Option<f64>,
    pub current_time: Option<f64>,
    pub is_finished: bool,
    pub hide_from_continue_listening: bool,
    /// A CFI, or a page number for PDF and comics (sent as a number by the web reader).
    #[serde(deserialize_with = "string_or_number")]
    pub ebook_location: Option<String>,
    pub ebook_progress: Option<f64>,
    pub last_update: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
}

/// What `ebookLocation` holds, as the web reader writes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum EbookLocation {
    /// A point CFI, for EPUB.
    Cfi(String),
    /// A 1-based page, for PDF and comics.
    Page(u32),
}

fn string_or_number<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(
        match Option::<serde_json::Value>::deserialize(deserializer)? {
            Some(serde_json::Value::String(s)) => Some(s),
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            _ => None,
        },
    )
}

/// `GET /api/me/progress` (2.36+).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProgressList {
    pub media_progress: Vec<MediaProgress>,
}

/// A bookmark at a position in an item.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Bookmark {
    pub library_item_id: String,
    pub time: f64,
    pub title: String,
    pub created_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ebook_location_is_a_string_or_a_page() {
        let cfi: MediaProgress =
            serde_json::from_str(r#"{"ebookLocation":"epubcfi(/6/2!/4)"}"#).unwrap();
        assert_eq!(cfi.ebook_location.as_deref(), Some("epubcfi(/6/2!/4)"));
        let page: MediaProgress = serde_json::from_str(r#"{"ebookLocation":12}"#).unwrap();
        assert_eq!(page.ebook_location.as_deref(), Some("12"));
        let none: MediaProgress = serde_json::from_str(r#"{"ebookLocation":null}"#).unwrap();
        assert_eq!(none.ebook_location, None);
        assert_eq!(serde_json::to_string(&EbookLocation::Page(3)).unwrap(), "3");
    }
}
