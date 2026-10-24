//! The background engine: a tokio runtime on its own thread that owns accounts and sync.
//!
//! The UI sends [`Command`]s and receives [`Event`]s through a callback; it never
//! blocks on network or database work. Each connected account also has a realtime
//! socket whose events update the database.

mod live;
mod playback;
mod worker;

use std::path::PathBuf;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::credentials::Credentials;
use crate::db::Account;
use crate::playback::PlayStart;
use crate::sync::CatalogReport;

/// Setting key for the catalog re-sync interval in minutes.
pub const SYNC_INTERVAL_SETTING: &str = "sync_interval_minutes";
/// Default catalog re-sync interval in minutes.
pub const DEFAULT_SYNC_INTERVAL_MINUTES: u64 = 30;

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
}

/// Where the engine keeps its data.
pub struct EngineConfig {
    pub db_path: PathBuf,
    pub credentials: Credentials,
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
    ) -> Result<Self, crate::db::DbError> {
        let db = crate::db::Db::open(&config.db_path)?;
        let (commands, receiver) = mpsc::unbounded_channel();
        let thread = std::thread::Builder::new()
            .name("abs-engine".into())
            .spawn(move || worker::run(db, config.credentials, receiver, Box::new(on_event)))
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
