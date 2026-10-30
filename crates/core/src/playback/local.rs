//! Playing without a server session: from downloaded files, or after the server went
//! away. Listening is recorded as a local session and sent on reconnect.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use abs_api::models::Chapter;

use super::{ListenTracker, PlayStart, TrackSource, store_local};
use crate::account::{AccountError, lock};
use crate::db::{Db, DownloadState, LocalSessionRow, now_ms};
use crate::downloads::{self, Manifest};

/// Playback recorded as a local session.
pub struct LocalPlayback {
    pub tracker: ListenTracker,
    record: LocalSessionRow,
    /// Server time minus local time, as last seen.
    clock_offset: Option<i64>,
}

impl LocalPlayback {
    /// A new local session for an item about to play from `start.start_time`.
    pub fn new(start: &PlayStart, clock_offset: Option<i64>) -> Self {
        let tracker = ListenTracker::new(Instant::now(), start.start_time, start.duration);
        Self::resume(start, tracker, clock_offset)
    }

    /// Takes over the tracker of a server session that lost its server.
    pub fn resume(start: &PlayStart, tracker: ListenTracker, clock_offset: Option<i64>) -> Self {
        let now = now_ms();
        let record = LocalSessionRow {
            id: format!("play_local_{}", uuid::Uuid::new_v4()),
            account_id: start.account_id.clone(),
            item_id: start.item_id.clone(),
            library_id: start.library_id.clone(),
            title: start.title.clone(),
            author: start.author.clone().unwrap_or_default(),
            duration: start.duration,
            start_position: tracker.position(),
            position: tracker.position(),
            time_listening: 0.0,
            started_at: now,
            updated_at: now,
            decision: None,
        };
        Self {
            tracker,
            record,
            clock_offset,
        }
    }

    pub fn account_id(&self) -> &str {
        &self.record.account_id
    }

    pub fn item_id(&self) -> &str {
        &self.record.item_id
    }

    /// The `play_local_` id, so a flush can leave the running session alone.
    pub fn session_id(&self) -> &str {
        &self.record.id
    }

    /// Stores the position, and the session once something was listened to.
    pub fn sync(&mut self, db: &Mutex<Db>) -> Result<(), AccountError> {
        let sync = self.tracker.take_sync(Instant::now());
        store_local(
            db,
            &self.record.account_id,
            &self.record.item_id,
            &sync,
            self.clock_offset,
        )?;
        self.record.position = sync.current_time;
        self.record.time_listening += sync.time_listened;
        self.record.updated_at = now_ms();
        if self.record.time_listening > 0.0 {
            lock(db).store().local_sessions().save(&self.record)?;
        }
        Ok(())
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
    if manifest.tracks.is_empty() {
        return None;
    }
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
        library_id: manifest.library_id,
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
