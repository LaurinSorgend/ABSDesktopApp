use serde::Deserialize;

use crate::Raw;

/// `GET /api/libraries`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Libraries {
    pub libraries: Vec<Raw<Library>>,
}

/// A library on the server.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Library {
    pub id: String,
    pub name: String,
    pub display_order: i64,
    pub icon: Option<String>,
    /// `book` or `podcast`.
    pub media_type: String,
    pub folders: Vec<Folder>,
    pub last_update: Option<i64>,
}

/// A folder a library scans.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Folder {
    pub id: String,
    pub full_path: String,
}

/// One page of a paginated list.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    #[serde(default = "Vec::new")]
    pub results: Vec<T>,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub limit: u64,
    #[serde(default)]
    pub page: u64,
}
