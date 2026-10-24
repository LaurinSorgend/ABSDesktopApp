use std::io;
use std::path::{Path, PathBuf};

use abs_api::Raw;
use abs_api::models::LibraryItem;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use super::DownloadError;
use crate::db::DownloadFile;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const COVER_FILE: &str = "cover.webp";
const MANIFEST_VERSION: u32 = 1;
/// Space kept free besides the download itself.
const SPARE_BYTES: u64 = 200 * 1024 * 1024;

/// What offline playback needs, written once all files are in place.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub account_id: String,
    pub item_id: String,
    pub title: String,
    pub author: Option<String>,
    pub duration: f64,
    pub tracks: Vec<ManifestTrack>,
    pub chapters: Vec<ManifestChapter>,
    /// The expanded item as the server sent it.
    pub item: Box<RawValue>,
}

/// One audio file and its place in item time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestTrack {
    pub file_name: String,
    pub start_offset: f64,
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestChapter {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// The folder of one item under `root`.
pub fn item_dir(root: &Path, account_id: &str, item_id: &str) -> PathBuf {
    root.join(safe_name(account_id)).join(safe_name(item_id))
}

/// The files to fetch and the manifest to write when they are done.
pub fn plan(
    account_id: &str,
    item: Raw<LibraryItem>,
) -> Result<(Vec<DownloadFile>, Manifest), DownloadError> {
    let (item, json) = item.into_parts();
    if item.media.tracks.is_empty() {
        return Err(DownloadError::NoAudio);
    }
    let mut files = Vec::new();
    let mut tracks = Vec::new();
    for track in &item.media.tracks {
        let meta = track.metadata.clone().unwrap_or_default();
        let file_name = format!("{}{}", safe_name(&track.ino), extension(&meta.ext));
        files.push(DownloadFile {
            ino: track.ino.clone(),
            file_name: file_name.clone(),
            size: meta.size,
            bytes_done: 0,
        });
        tracks.push(ManifestTrack {
            file_name,
            start_offset: track.start_offset,
            duration: track.duration,
        });
    }
    let metadata = &item.media.metadata;
    let manifest = Manifest {
        version: MANIFEST_VERSION,
        account_id: account_id.to_owned(),
        item_id: item.id.clone(),
        title: metadata.title.clone().unwrap_or_default(),
        author: metadata.author_name.clone().or_else(|| {
            let names: Vec<_> = metadata.authors.iter().map(|a| a.name.as_str()).collect();
            (!names.is_empty()).then(|| names.join(", "))
        }),
        duration: item.media.duration.unwrap_or_default(),
        tracks,
        chapters: item
            .media
            .chapters
            .iter()
            .map(|c| ManifestChapter {
                start: c.start,
                end: c.end,
                title: c.title.clone(),
            })
            .collect(),
        item: json,
    };
    Ok((files, manifest))
}

pub fn write_manifest(dir: &Path, manifest: &Manifest) -> io::Result<()> {
    let text = serde_json::to_string_pretty(manifest).map_err(io::Error::other)?;
    let part = dir.join(format!("{MANIFEST_FILE}.part"));
    std::fs::write(&part, text)?;
    std::fs::rename(part, dir.join(MANIFEST_FILE))
}

pub fn read_manifest(dir: &Path) -> io::Result<Manifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_FILE))?;
    serde_json::from_str(&text).map_err(io::Error::other)
}

/// True when the manifest is there and every file has its full size.
pub fn check(dir: &Path, files: &[DownloadFile]) -> bool {
    dir.join(MANIFEST_FILE).is_file()
        && files
            .iter()
            .all(|f| std::fs::metadata(dir.join(&f.file_name)).is_ok_and(|m| m.len() == f.size))
}

/// Fails unless `root`'s disk has room for `needed` more bytes and some spare.
pub fn ensure_space(root: &Path, needed: u64) -> Result<(), DownloadError> {
    std::fs::create_dir_all(root)?;
    let available = fs4::available_space(root)?;
    if available < needed.saturating_add(SPARE_BYTES) {
        return Err(DownloadError::NoSpace { needed, available });
    }
    Ok(())
}

/// Removes an item folder; a missing folder is fine.
pub fn delete_dir(dir: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Keeps ids usable as file names.
fn safe_name(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `.mp3` from the server's `ext`; anything odd becomes no extension.
fn extension(ext: &str) -> String {
    let bare = ext.trim_start_matches('.');
    if !bare.is_empty() && bare.len() <= 8 && bare.chars().all(|c| c.is_ascii_alphanumeric()) {
        format!(".{}", bare.to_ascii_lowercase())
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Raw<LibraryItem> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/api/2.37.2/item_multi_file.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn plan_and_check_a_multi_file_book() {
        let (mut files, manifest) = plan("acc", fixture()).unwrap();
        assert_eq!(files.len(), 5);
        assert!(files[0].size > 1_000_000);
        files.iter_mut().for_each(|f| f.size = 10);
        assert!(files[0].file_name.ends_with(".mp3"));
        assert_eq!(manifest.tracks[1].start_offset, 1440.784762);
        assert!(!manifest.chapters.is_empty());
        let dir = tempfile::tempdir().unwrap();
        assert!(!check(dir.path(), &files));
        for f in &files {
            std::fs::write(dir.path().join(&f.file_name), vec![0; f.size as usize]).unwrap();
        }
        write_manifest(dir.path(), &manifest).unwrap();
        assert!(check(dir.path(), &files));
        let again = read_manifest(dir.path()).unwrap();
        assert_eq!(again.item.get(), manifest.item.get());
        std::fs::write(dir.path().join(&files[2].file_name), b"short").unwrap();
        assert!(!check(dir.path(), &files));
    }

    #[test]
    fn names_stay_inside_the_folder() {
        assert_eq!(safe_name("../x"), "___x");
        assert_eq!(extension(".MP3"), ".mp3");
        assert_eq!(extension("./../"), "");
        let dir = item_dir(Path::new("root"), "a/b", "c");
        assert_eq!(dir, Path::new("root").join("a_b").join("c"));
    }
}
