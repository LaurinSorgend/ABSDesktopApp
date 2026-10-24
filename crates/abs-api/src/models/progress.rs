use serde::Deserialize;

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
    pub ebook_location: Option<String>,
    pub ebook_progress: Option<f64>,
    pub last_update: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
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
