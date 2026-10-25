//! The background engine: a tokio runtime on its own thread that owns accounts and sync.
//!
//! The UI sends [`Command`]s and receives [`Event`]s through a callback; it never
//! blocks on network or database work. Each connected account also has a realtime
//! socket whose events update the database.

mod downloads;
mod ebooks;
mod live;
mod offline;
mod playback;
mod requests;
mod worker;

use std::path::PathBuf;
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::cover_cache::CoverCache;
use crate::credentials::Credentials;
use crate::db::Account;
use crate::playback::PlayStart;
use crate::sync::CatalogReport;

/// Setting key for the catalog re-sync interval in minutes.
pub const SYNC_INTERVAL_SETTING: &str = "sync_interval_minutes";
/// Default catalog re-sync interval in minutes.
pub const DEFAULT_SYNC_INTERVAL_MINUTES: u64 = 30;
/// Setting key for the cover cache cap in megabytes.
pub const COVER_CACHE_SETTING: &str = "cover_cache_mb";
/// Default cover cache cap in megabytes.
pub const DEFAULT_COVER_CACHE_MB: u64 = crate::cover_cache::DEFAULT_CAPACITY / (1024 * 1024);

/// Requests from the UI.
#[derive(Debug, Clone)]
pub enum Command {
    AddAccount {
        url: String,
        username: String,
        password: String,
    },
    RemoveAccount {
        account_id: String,
    },
    /// Sync one account, or all when `None`.
    SyncNow {
        account_id: Option<String>,
    },
    /// Start a server session for an item; answered with `PlaybackStarted`.
    /// Closes the previous session first.
    StartPlayback {
        account_id: String,
        item_id: String,
    },
    /// The player's state, about once a second and right after pause, seek or
    /// chapter change (`force_sync`).
    ReportPlayback {
        position: f64,
        playing: bool,
        force_sync: bool,
    },
    /// Close the session, as on stop.
    StopPlayback,
    /// A cover from the cache or the server; answered with `CoverLoaded`.
    LoadCover {
        account_id: String,
        item_id: String,
        /// The item's `cover_etag`; `None` means it has no cover.
        etag: Option<String>,
        width: u32,
    },
    /// The expanded item from the server; answered with `ItemLoaded`.
    LoadItem {
        account_id: String,
        item_id: String,
    },
    /// The server's home shelves of a library; answered with `ShelvesLoaded`.
    LoadShelves {
        account_id: String,
        library_id: String,
    },
    /// Server search, for what the local index misses; answered with `SearchResults`.
    SearchServer {
        account_id: String,
        library_id: String,
        text: String,
    },
    /// Series, authors or collections of a library; answered with `GroupsLoaded`.
    LoadGroups {
        account_id: String,
        library_id: String,
        kind: GroupKind,
    },
    /// The items of one author; answered with `GroupItems`.
    LoadAuthorItems {
        account_id: String,
        library_id: String,
        author_id: String,
    },
    SetFinished {
        account_id: String,
        item_id: String,
        finished: bool,
    },
    AddBookmark {
        account_id: String,
        item_id: String,
        time: f64,
        title: String,
    },
    RemoveBookmark {
        account_id: String,
        item_id: String,
        time: f64,
    },
    /// New cover cache cap; also stored as a setting.
    SetCoverCacheSize {
        megabytes: u64,
    },
    /// Replaces the server addresses of an account's server, tried in this order.
    /// Reconnects when the account is offline or uses an address no longer listed.
    SetServerUrls {
        account_id: String,
        urls: Vec<String>,
    },
    /// Turns an account on or off; answered with `AccountUpdated`. A disabled account
    /// keeps its data and login but does not connect, sync or download.
    SetAccountEnabled {
        account_id: String,
        enabled: bool,
    },
    /// The user's answer to `ProgressConflict`: keep the local position or take the server's.
    ResolveConflict {
        account_id: String,
        item_id: String,
        keep_local: bool,
    },
    /// Queue an item for download; replaces an earlier download of it.
    Download {
        account_id: String,
        item_id: String,
    },
    PauseDownload {
        account_id: String,
        item_id: String,
    },
    /// Puts a paused or failed download back in the queue.
    ResumeDownload {
        account_id: String,
        item_id: String,
    },
    /// Cancels a download or removes a finished one, with its files.
    DeleteDownload {
        account_id: String,
        item_id: String,
    },
    /// Finds the item's ebook (download, cache or server); answered with `EbookReady`
    /// or `EbookFailed`.
    OpenEbook {
        account_id: String,
        item_id: String,
    },
    /// A reading position; stored here at once, sent now or when the server is back.
    SaveEbookProgress {
        account_id: String,
        item_id: String,
        location: abs_api::models::EbookLocation,
        progress: f64,
    },
}

/// Ways to group a library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKind {
    Series,
    Authors,
    Collections,
}

/// A series, author or collection.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub id: String,
    pub name: String,
    pub count: u64,
    /// The items in order; empty for authors, whose items come from `LoadAuthorItems`.
    pub item_ids: Vec<String>,
}

/// A home shelf of books.
#[derive(Debug, Clone, PartialEq)]
pub struct Shelf {
    pub id: String,
    pub label: String,
    pub item_ids: Vec<String>,
}

/// Notifications for the UI.
#[derive(Debug, Clone)]
pub enum Event {
    /// The stored accounts, sent once at start.
    AccountsLoaded(Vec<Account>),
    AccountAdded(Account),
    AddAccountFailed {
        message: String,
        needs_login: bool,
    },
    AccountConnected {
        account_id: String,
    },
    AccountRemoved {
        account_id: String,
    },
    /// An account row changed, as when it was enabled or disabled.
    AccountUpdated(Account),
    /// The stored login is no longer valid.
    LoginRequired {
        account_id: String,
    },
    /// The server could not be reached; the engine retries on its own.
    Offline {
        account_id: String,
        message: String,
    },
    SyncStarted {
        account_id: String,
    },
    CatalogSynced {
        account_id: String,
        report: CatalogReport,
    },
    ProgressChanged {
        account_id: String,
        item_ids: Vec<String>,
    },
    /// Items were added, changed or removed by a realtime event.
    CatalogChanged {
        account_id: String,
    },
    /// Session started; load these tracks into the player.
    PlaybackStarted(Box<PlayStart>),
    PlaybackFailed {
        message: String,
    },
    /// New HTTP headers for the player after a token refresh.
    PlaybackHeaders {
        headers: Vec<String>,
    },
    SyncFailed {
        account_id: String,
        message: String,
    },
    /// `bytes` is `None` when the item has no cover or it could not be loaded.
    CoverLoaded {
        account_id: String,
        item_id: String,
        width: u32,
        bytes: Option<Arc<Vec<u8>>>,
    },
    /// The expanded item JSON.
    ItemLoaded {
        account_id: String,
        item_id: String,
        json: String,
    },
    ShelvesLoaded {
        account_id: String,
        library_id: String,
        shelves: Vec<Shelf>,
    },
    SearchResults {
        account_id: String,
        text: String,
        item_ids: Vec<String>,
    },
    GroupsLoaded {
        account_id: String,
        library_id: String,
        kind: GroupKind,
        groups: Vec<Group>,
    },
    GroupItems {
        account_id: String,
        group_id: String,
        item_ids: Vec<String>,
    },
    BookmarksChanged {
        account_id: String,
        item_id: String,
    },
    /// Offline listening and the server disagree on an item's position; answer with
    /// `ResolveConflict`. Sent once per item until answered.
    ProgressConflict(crate::offline::Conflict),
    /// A download was queued, started, paused, finished, failed or removed.
    DownloadsChanged {
        account_id: String,
        item_id: String,
    },
    /// Bytes on disk of a running download, at most twice a second.
    DownloadProgress {
        account_id: String,
        item_id: String,
        bytes_done: u64,
        size: u64,
    },
    /// The book file to give the reader.
    EbookReady(Box<crate::ebooks::EbookOpen>),
    EbookFailed {
        account_id: String,
        item_id: String,
        message: String,
    },
    /// A request other than sync or playback failed.
    RequestFailed {
        account_id: String,
        message: String,
    },
}

/// Where the engine keeps its data.
pub struct EngineConfig {
    pub db_path: PathBuf,
    /// Cache folder for covers and ebooks.
    pub cover_root: PathBuf,
    /// Default folder for downloads; the `download_root` setting overrides it.
    pub download_root: PathBuf,
    pub credentials: Credentials,
}

/// Why the engine could not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    Db(#[from] crate::db::DbError),
    #[error("cover cache: {0}")]
    Covers(#[from] std::io::Error),
}

/// Handle to the running engine. Dropping it stops the engine.
pub struct Engine {
    commands: Option<mpsc::UnboundedSender<Command>>,
    thread: Option<JoinHandle<()>>,
}

impl Engine {
    /// Opens the database and starts the engine thread. `on_event` runs on that thread.
    pub fn start(
        config: EngineConfig,
        on_event: impl Fn(Event) + Send + Sync + 'static,
    ) -> Result<Self, StartError> {
        let db = crate::db::Db::open(&config.db_path)?;
        let megabytes = db
            .store()
            .settings()
            .get_or(COVER_CACHE_SETTING, DEFAULT_COVER_CACHE_MB)?;
        let covers = CoverCache::open(&config.cover_root, megabytes * 1024 * 1024)?;
        let (commands, receiver) = mpsc::unbounded_channel();
        let thread = std::thread::Builder::new()
            .name("abs-engine".into())
            .spawn(move || {
                let parts = worker::Parts {
                    db,
                    covers,
                    cache_root: config.cover_root,
                    download_root: config.download_root,
                    credentials: config.credentials,
                };
                worker::run(parts, receiver, Box::new(on_event))
            })
            .expect("spawn engine thread");
        Ok(Self {
            commands: Some(commands),
            thread: Some(thread),
        })
    }

    /// Queues a command. Ignored after the engine stopped.
    pub fn send(&self, command: Command) {
        if let Some(commands) = &self.commands {
            let _ = commands.send(command);
        }
    }

    /// Stops the engine and waits for its thread.
    pub fn shutdown(mut self) {
        self.stop();
    }

    fn stop(&mut self) {
        self.commands.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.stop();
    }
}
