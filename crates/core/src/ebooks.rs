//! Ebooks to read: from a finished download, the ebook cache or the server. The cache
//! is `<root>/<account_id>/ebooks/<item_id>_<ino>.<format>` and keeps the newest few.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use abs_api::models::{EbookFile, EbookLocation};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::account::{AccountError, Session, lock};
use crate::db::{Db, DownloadState};
use crate::downloads;

/// Cached ebooks kept per account.
pub const CACHE_KEEP: usize = 10;
/// The `pending_sync` kind of a reading position that still has to go to the server.
pub const QUEUE_KIND: &str = "ebook_progress";
/// Formats the reader opens. CBR needs unrar, whose license does not fit.
const READABLE: [&str; 6] = ["epub", "pdf", "cbz", "mobi", "azw3", "fb2"];

/// A book file ready for the reader.
#[derive(Debug, Clone, PartialEq)]
pub struct EbookOpen {
    pub account_id: String,
    pub item_id: String,
    pub title: String,
    pub path: PathBuf,
    pub format: String,
    /// The stored `ebookLocation`: a CFI or a page number.
    pub location: Option<String>,
}

/// A reading position as queued while the server is away.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PendingPosition {
    pub item_id: String,
    pub location: EbookLocation,
    pub progress: f64,
}

/// Why an ebook could not be opened.
#[derive(Debug, thiserror::Error)]
pub enum EbookError {
    #[error(transparent)]
    Account(#[from] AccountError),
    #[error(transparent)]
    Http(#[from] abs_api::reqwest::Error),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("the item has no ebook")]
    NoEbook,
    #[error("{0} books cannot be opened")]
    Unsupported(String),
    #[error("the ebook is not downloaded and the server is not connected")]
    Offline,
    #[error("{0} cannot be reached")]
    Unreachable(String),
}

impl From<abs_api::Error> for EbookError {
    fn from(e: abs_api::Error) -> Self {
        Self::Account(e.into())
    }
}

/// True for formats the reader opens.
pub fn is_readable(format: &str) -> bool {
    READABLE.contains(&format.to_ascii_lowercase().as_str())
}

/// The format of an item's ebook, from its `ebookFile` or the minified `ebookFormat`.
pub fn format_of(file: &EbookFile) -> String {
    file.ebook_format
        .clone()
        .unwrap_or_else(|| file.metadata.ext.trim_start_matches('.').to_owned())
        .to_ascii_lowercase()
}

pub fn cache_dir(root: &Path, account_id: &str) -> PathBuf {
    root.join(safe(account_id)).join("ebooks")
}

/// The ebook of a finished download, as path and format.
pub fn from_download(db: &Mutex<Db>, account_id: &str, item_id: &str) -> Option<(PathBuf, String)> {
    let row = lock(db)
        .store()
        .downloads()
        .get(account_id, item_id)
        .ok()??;
    if row.state != DownloadState::Done {
        return None;
    }
    let ebook = downloads::read_manifest(&row.dir).ok()?.ebook?;
    let path = row.dir.join(&ebook.file_name);
    path.is_file().then_some((path, ebook.format))
}

/// The ebook of a book in a local folder, as path and format. `None` for other items.
pub fn from_local_folder(
    db: &Mutex<Db>,
    account_id: &str,
    item_id: &str,
) -> Option<Result<(PathBuf, String), EbookError>> {
    let book = lock(db).store().local().book(account_id, item_id).ok()??;
    let Some(path) = book.ebook_path else {
        return Some(Err(EbookError::NoEbook));
    };
    if !path.is_file() {
        return Some(Err(EbookError::Unreachable(path.display().to_string())));
    }
    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    Some(Ok((path, format)))
}

/// Any cached copy of the item's ebook, for when the server is away.
pub fn from_cache(root: &Path, account_id: &str, item_id: &str) -> Option<(PathBuf, String)> {
    let prefix = format!("{}_", safe(item_id));
    let entries = std::fs::read_dir(cache_dir(root, account_id)).ok()?;
    entries.flatten().map(|e| e.path()).find_map(|path| {
        let name = path.file_name()?.to_str()?;
        let format = path.extension()?.to_str()?.to_owned();
        (name.starts_with(&prefix) && format != "part").then_some((path, format))
    })
}

/// Loads the item from the server and returns its cached ebook, fetching it first
/// when the cache has no copy of this file. Also returns the item title.
pub async fn fetch(
    session: &Session,
    root: &Path,
    item_id: &str,
) -> Result<(PathBuf, String, String), EbookError> {
    let item = session.client.item(item_id).await?;
    let file = item.media.ebook_file.clone().ok_or(EbookError::NoEbook)?;
    let format = format_of(&file);
    if !is_readable(&format) {
        return Err(EbookError::Unsupported(format));
    }
    let title = item.media.metadata.title.clone().unwrap_or_default();
    let dir = cache_dir(root, &session.account.id);
    let path = dir.join(format!("{}_{}.{format}", safe(item_id), safe(&file.ino)));
    let size = file.metadata.size;
    if !std::fs::metadata(&path).is_ok_and(|m| size == 0 || m.len() == size) {
        download(session, item_id, &file.ino, &path).await?;
        prune(&dir, CACHE_KEEP);
    }
    touch(&path);
    Ok((path, format, title))
}

async fn download(
    session: &Session,
    item_id: &str,
    ino: &str,
    path: &Path,
) -> Result<(), EbookError> {
    tokio::fs::create_dir_all(path.parent().expect("cache file has a folder")).await?;
    let part = path.with_extension("part");
    let mut out = tokio::fs::File::create(&part).await?;
    let mut body = session.client.ebook(item_id, ino).await?.bytes_stream();
    while let Some(chunk) = body.next().await {
        out.write_all(&chunk?).await?;
    }
    out.flush().await?;
    drop(out);
    tokio::fs::rename(&part, path).await?;
    Ok(())
}

/// Marks a cache file as used, so pruning keeps it.
fn touch(path: &Path) {
    if let Ok(file) = std::fs::File::options().append(true).open(path) {
        let _ = file.set_modified(std::time::SystemTime::now());
    }
}

/// Deletes all but the `keep` most recently used files in `dir`.
pub fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<_> = entries
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// Deletes the cached ebooks of an account.
pub fn forget_account(root: &Path, account_id: &str) {
    let _ = downloads::delete_dir(&cache_dir(root, account_id));
}

fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readable_formats() {
        assert!(is_readable("EPUB"));
        assert!(is_readable("cbz"));
        assert!(!is_readable("cbr"));
    }

    #[test]
    fn cache_lookup_and_prune() {
        let root = tempfile::tempdir().unwrap();
        let dir = cache_dir(root.path(), "acc");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("li_1_77.epub.part"), b"half").unwrap();
        assert_eq!(from_cache(root.path(), "acc", "li_1"), None);
        for i in 0..4 {
            let path = dir.join(format!("li_{i}_9.pdf"));
            std::fs::write(&path, b"%PDF").unwrap();
            let age = std::time::Duration::from_secs(100 - i);
            let file = std::fs::File::options().append(true).open(&path).unwrap();
            file.set_modified(std::time::SystemTime::now() - age)
                .unwrap();
        }
        let (path, format) = from_cache(root.path(), "acc", "li_2").unwrap();
        assert_eq!(
            (path.file_name().unwrap(), format.as_str()),
            ("li_2_9.pdf".as_ref(), "pdf")
        );
        prune(&dir, 2);
        assert!(from_cache(root.path(), "acc", "li_3").is_some());
        assert!(from_cache(root.path(), "acc", "li_0").is_none());
        forget_account(root.path(), "acc");
        assert!(!dir.exists());
    }

    #[test]
    fn pending_positions_keep_numbers_and_strings() {
        let page = PendingPosition {
            item_id: "li_1".into(),
            location: EbookLocation::Page(4),
            progress: 0.1,
        };
        let json = serde_json::to_string(&page).unwrap();
        assert_eq!(
            serde_json::from_str::<PendingPosition>(&json).unwrap(),
            page
        );
    }
}
