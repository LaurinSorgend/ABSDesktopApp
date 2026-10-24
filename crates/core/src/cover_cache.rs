//! Cover images on disk: `<root>/<account_id>/covers/<item_id>_<width>_<etag>.webp`.
//!
//! The etag in the file name is the item's `cover_etag`, so a changed cover is a miss.
//! Total size is capped; the least recently used files go first.

mod index;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::SystemTime;

pub use index::Slot;
use index::{Index, Removed};

/// Default size cap: 500 MB.
pub const DEFAULT_CAPACITY: u64 = 500 * 1024 * 1024;

const EXTENSION: &str = "webp";

/// The size-capped cover cache shared by all accounts.
pub struct CoverCache {
    root: PathBuf,
    index: Mutex<Index>,
}

impl CoverCache {
    /// Opens the cache at `root` and indexes the files already there, oldest use first.
    pub fn open(root: impl Into<PathBuf>, capacity: u64) -> io::Result<Self> {
        let cache = Self {
            root: root.into(),
            index: Mutex::new(Index::new(capacity)),
        };
        fs::create_dir_all(&cache.root)?;
        let mut files = cache.scan()?;
        files.sort_by_key(|(_, _, _, modified)| *modified);
        for (slot, etag, size, _) in files {
            let removed = cache.lock().insert(slot, &etag, size);
            cache.delete(removed);
        }
        Ok(cache)
    }

    /// The cached bytes if the slot holds `etag`.
    pub fn get(&self, slot: &Slot, etag: &str) -> Option<Vec<u8>> {
        if !self.lock().touch(slot, etag) {
            return None;
        }
        let path = self.path(slot, etag);
        match fs::read(&path) {
            Ok(bytes) => {
                mark_used(&path);
                Some(bytes)
            }
            Err(_) => {
                self.lock().remove(slot);
                None
            }
        }
    }

    /// Stores a cover and evicts old ones over the cap.
    pub fn put(&self, slot: &Slot, etag: &str, bytes: &[u8]) -> io::Result<()> {
        let path = self.path(slot, etag);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        let removed = self.lock().insert(slot.clone(), etag, bytes.len() as u64);
        self.delete(removed);
        Ok(())
    }

    /// Changes the size cap, evicting at once if needed.
    pub fn set_capacity(&self, capacity: u64) {
        let removed = self.lock().set_capacity(capacity);
        self.delete(removed);
    }

    pub fn total_bytes(&self) -> u64 {
        self.lock().total_bytes()
    }

    /// Deletes all covers of an account, as when the account is removed.
    pub fn remove_account(&self, account_id: &str) -> io::Result<()> {
        self.lock().remove_account(account_id);
        match fs::remove_dir_all(self.root.join(safe(account_id))) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    fn path(&self, slot: &Slot, etag: &str) -> PathBuf {
        self.root
            .join(safe(&slot.account_id))
            .join("covers")
            .join(format!(
                "{}_{}_{}.{EXTENSION}",
                safe(&slot.item_id),
                slot.width,
                safe(etag)
            ))
    }

    fn delete(&self, removed: Removed) {
        for (slot, etag) in removed.files {
            let _ = fs::remove_file(self.path(&slot, &etag));
        }
    }

    fn lock(&self) -> MutexGuard<'_, Index> {
        self.index.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn scan(&self) -> io::Result<Vec<(Slot, String, u64, SystemTime)>> {
        let mut found = Vec::new();
        for account in fs::read_dir(&self.root)? {
            let account = account?;
            let Some(account_id) = account.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let covers = account.path().join("covers");
            let Ok(entries) = fs::read_dir(&covers) else {
                continue;
            };
            for file in entries.flatten() {
                if let Some(parsed) = parse_file(&account_id, &file.path()) {
                    let meta = file.metadata()?;
                    let modified = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                    found.push((parsed.0, parsed.1, meta.len(), modified));
                }
            }
        }
        Ok(found)
    }
}

/// Sets the modification time to now, so the LRU order survives a restart.
fn mark_used(path: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// Parses `<item>_<width>_<etag>.webp`. Leftover `.tmp` files and other names are ignored.
fn parse_file(account_id: &str, path: &Path) -> Option<(Slot, String)> {
    if path.extension()? != EXTENSION {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let mut parts = stem.rsplitn(3, '_');
    let etag = parts.next()?.to_owned();
    let width = parts.next()?.parse().ok()?;
    let item_id = parts.next()?.to_owned();
    let slot = Slot {
        account_id: account_id.to_owned(),
        item_id,
        width,
    };
    Some((slot, etag))
}

/// Keeps ids usable as path parts: letters, digits and `-`; anything else becomes `-`.
fn safe(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slot(item: &str) -> Slot {
        Slot {
            account_id: "acc-1".into(),
            item_id: item.into(),
            width: 400,
        }
    }

    #[test]
    fn put_get_and_stale_etag() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CoverCache::open(dir.path(), DEFAULT_CAPACITY).unwrap();
        cache.put(&slot("item-1"), "100", b"one").unwrap();
        assert_eq!(
            cache.get(&slot("item-1"), "100").as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(cache.get(&slot("item-1"), "200"), None);
        cache.put(&slot("item-1"), "200", b"two!").unwrap();
        assert_eq!(cache.total_bytes(), 4);
        let covers = dir.path().join("acc-1/covers");
        assert_eq!(
            fs::read_dir(covers).unwrap().count(),
            1,
            "old etag file deleted"
        );
    }

    #[test]
    fn reopen_keeps_files_and_cap_evicts() {
        let dir = tempfile::tempdir().unwrap();
        {
            let cache = CoverCache::open(dir.path(), DEFAULT_CAPACITY).unwrap();
            cache.put(&slot("a"), "1", &[0; 10]).unwrap();
            cache.put(&slot("b"), "1", &[0; 10]).unwrap();
        }
        let cache = CoverCache::open(dir.path(), DEFAULT_CAPACITY).unwrap();
        assert_eq!(cache.total_bytes(), 20);
        assert!(cache.get(&slot("a"), "1").is_some());
        cache.set_capacity(10);
        assert_eq!(cache.total_bytes(), 10);
        assert!(
            cache.get(&slot("a"), "1").is_some(),
            "recently used file kept"
        );
        assert!(cache.get(&slot("b"), "1").is_none());
        cache.remove_account("acc-1").unwrap();
        assert_eq!(cache.total_bytes(), 0);
        assert!(!dir.path().join("acc-1").exists());
    }

    #[test]
    fn parses_file_names() {
        let (slot, etag) =
            parse_file("acc", Path::new("x/9c784f3c-f150_400_1792867322954.webp")).unwrap();
        assert_eq!(
            (slot.item_id.as_str(), slot.width, etag.as_str()),
            ("9c784f3c-f150", 400, "1792867322954")
        );
        assert!(parse_file("acc", Path::new("x/a_400_1.tmp")).is_none());
        assert_eq!(safe("../x_y"), "---x-y");
    }
}
