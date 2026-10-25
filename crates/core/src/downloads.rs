//! Downloaded items: `<root>/<account_id>/<item_id>/<ino>.<ext>` plus `manifest.json`
//! (item JSON, chapters, track offsets) and `cover.webp`.

mod storage;
mod transfer;

pub use storage::{
    COVER_FILE, MANIFEST_FILE, Manifest, ManifestChapter, ManifestEbook, ManifestTrack, check,
    commit_manifest, delete_dir, ensure_space, item_dir, plan, read_manifest, write_manifest,
};
pub use transfer::fetch_file;

/// Setting key for the folder new downloads go to.
pub const ROOT_SETTING: &str = "download_root";

/// Why a download failed.
#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error(transparent)]
    Api(#[from] abs_api::Error),
    #[error(transparent)]
    Http(#[from] abs_api::reqwest::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("{file} has {actual} bytes, expected {expected}")]
    SizeMismatch {
        file: String,
        expected: u64,
        actual: u64,
    },
    #[error("not enough disk space: {needed} bytes needed, {available} free")]
    NoSpace { needed: u64, available: u64 },
    #[error("the server is not connected")]
    Offline,
    #[error("this account may not download")]
    NotAllowed,
    #[error("the item has no audio or ebook files")]
    NoFiles,
}

impl DownloadError {
    /// True for errors that a later try may not hit again.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(_) | Self::SizeMismatch { .. } => true,
            Self::Api(e) => !e.needs_login() && e.status().is_none_or(|s| s.is_server_error()),
            _ => false,
        }
    }
}
