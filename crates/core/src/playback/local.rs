//! Playing downloaded files: tracks and chapters come from the manifest.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use abs_api::models::Chapter;

use super::{ListenTracker, PlayStart, TrackSource, store_local};
use crate::account::{AccountError, lock};
use crate::db::{Db, DownloadState};
use crate::downloads::{self, Manifest};

/// Playback of a downloaded item without a server session.
pub struct LocalPlayback {
    pub account_id: String,
    pub item_id: String,
    pub tracker: ListenTracker,
}

impl LocalPlayback {
    pub fn new(start: &PlayStart) -> Self {
        Self {
            account_id: start.account_id.clone(),
            item_id: start.item_id.clone(),
            tracker: ListenTracker::new(Instant::now(), start.start_time, start.duration),
        }
    }

    /// Stores the position locally.
    pub fn sync(&mut self, db: &Mutex<Db>) -> Result<(), AccountError> {
        let sync = self.tracker.take_sync(Instant::now());
        store_local(db, &self.account_id, &self.item_id, &sync)
    }
}

/// The player data for a complete download, starting at the stored position.
pub fn local_start(db: &Mutex<Db>, account_id: &str, item_id: &str) -> Option<PlayStart> {
    let db = lock(db);
    let store = db.store();
    let row = store.downloads().get(account_id, item_id).ok()??;
    let files = store.downloads().files(account_id, item_id).ok()?;
    if row.state != DownloadState::Done || !downloads::check(&row.dir, &files) {
        return None;
    }
    let manifest = downloads::read_manifest(&row.dir).ok()?;
    let position = store
        .progress()
        .get(account_id, item_id)
        .ok()
        .flatten()
        .map_or(0.0, |p| p.current_time);
    Some(from_manifest(&row.dir, manifest, position))
}

fn from_manifest(dir: &Path, manifest: Manifest, position: f64) -> PlayStart {
    PlayStart {
        account_id: manifest.account_id,
        item_id: manifest.item_id,
        title: manifest.title,
        author: manifest.author,
        tracks: manifest
            .tracks
            .into_iter()
            .map(|t| TrackSource {
                url: dir.join(t.file_name).to_string_lossy().into_owned(),
                start_offset: t.start_offset,
                duration: t.duration,
            })
            .collect(),
        chapters: manifest
            .chapters
            .into_iter()
            .enumerate()
            .map(|(id, c)| Chapter {
                id: id as i64,
                start: c.start,
                end: c.end,
                title: c.title,
            })
            .collect(),
        start_time: position,
        duration: manifest.duration,
        http_headers: Vec::new(),
        local: true,
    }
}
