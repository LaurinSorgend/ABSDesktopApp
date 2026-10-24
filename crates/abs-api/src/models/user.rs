use serde::Deserialize;

use super::progress::{Bookmark, MediaProgress};

/// `GET /status`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Status {
    pub app: String,
    pub server_version: String,
    pub is_init: bool,
    pub language: Option<String>,
    pub auth_methods: Vec<String>,
}

/// A user as returned by `/login`, `/api/authorize` and `/api/me`. Tokens are left out.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub username: String,
    pub email: Option<String>,
    #[serde(rename = "type")]
    pub user_type: String,
    pub media_progress: Vec<MediaProgress>,
    pub bookmarks: Vec<Bookmark>,
    pub is_active: bool,
    pub is_locked: bool,
    pub permissions: Permissions,
    pub libraries_accessible: Vec<String>,
}

/// What a user may do on the server.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Permissions {
    pub download: bool,
    pub update: bool,
    pub delete: bool,
    pub upload: bool,
    pub access_all_libraries: bool,
    pub access_explicit_content: bool,
}

/// Body of `/login`, `/auth/refresh` and `/api/authorize`, without tokens.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LoginResponse {
    pub user: User,
    pub user_default_library_id: Option<String>,
}
