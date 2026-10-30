use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use abs_api::reqwest::Client;
use abs_api::{AbsClient, Feature, Realtime};
use tokio::sync::{Notify, Semaphore, mpsc};
use tokio::task::JoinHandle;

use super::downloads;
use super::ebooks;
use super::live::{Followup, LiveState};
use super::offline;
use super::playback::{self, PlaybackCommand};
use super::requests;
use super::works;
use super::{
    Command, DEFAULT_SYNC_INTERVAL_MINUTES, Event, REMOTE_CHECK_SETTING, SYNC_INTERVAL_SETTING,
};
use crate::account::{
    AccountError, Session, add_account, add_account_with_key, earlier_url_answers, lock,
    reconnect_account, restore_account,
};
use crate::cover_cache::CoverCache;
use crate::credentials::Credentials;
use crate::db::{Account, Db};
use crate::sync::{sync_catalog, sync_progress};

const TICK: Duration = Duration::from_secs(60);
const PARALLEL_COVER_DOWNLOADS: usize = 6;
/// Gap between the startup logins of accounts on the same server, which rate limits auth.
const LOGIN_STAGGER: Duration = Duration::from_secs(1);

type Emit = Box<dyn Fn(Event) + Send + Sync>;

/// Connection state of one account.
enum Conn {
    Connecting,
    Connected(Arc<Session>),
    Offline,
    NeedsLogin,
    Disabled,
}

/// What the engine thread starts with.
pub(super) struct Parts {
    pub(super) db: Db,
    pub(super) covers: CoverCache,
    pub(super) cache_root: std::path::PathBuf,
    pub(super) download_root: std::path::PathBuf,
    pub(super) credentials: Credentials,
}

/// State shared by the engine's tasks.
pub(super) struct Shared {
    pub(super) db: Mutex<Db>,
    pub(super) covers: CoverCache,
    pub(super) cover_downloads: Semaphore,
    pub(super) cache_root: std::path::PathBuf,
    pub(super) download_root: std::path::PathBuf,
    pub(super) download_tasks: Mutex<downloads::Tasks>,
    /// The local session being recorded right now; flushes leave it alone.
    pub(super) running_local: Mutex<Option<String>>,
    /// Conflicts the UI was told about, by account and item.
    pub(super) conflicts_shown: Mutex<HashSet<(String, String)>>,
    /// Set when the works need matching again.
    pub(super) works_dirty: Notify,
    /// Set when a realtime event changed someone's progress.
    pub(super) progress_moved: Notify,
    credentials: Credentials,
    http: Client,
    pub(super) emit: Emit,
    conns: Mutex<HashMap<String, Conn>>,
    /// The latest client of each account, also while it is offline. Reconnecting shares its
    /// tokens with the sessions that still hold it.
    clients: Mutex<HashMap<String, Arc<AbsClient>>>,
    syncing: Mutex<HashSet<String>>,
    last_sync: Mutex<HashMap<String, Instant>>,
    live: Mutex<HashMap<String, JoinHandle<()>>>,
}

/// Runs until the command channel closes.
pub(super) fn run(parts: Parts, commands: mpsc::UnboundedReceiver<Command>, emit: Emit) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("engine runtime");
    let http = abs_api::http_client().expect("http client");
    let shared = Arc::new(Shared {
        db: Mutex::new(parts.db),
        covers: parts.covers,
        cover_downloads: Semaphore::new(PARALLEL_COVER_DOWNLOADS),
        cache_root: parts.cache_root,
        download_root: parts.download_root,
        download_tasks: Mutex::default(),
        running_local: Mutex::default(),
        conflicts_shown: Mutex::default(),
        works_dirty: Notify::new(),
        progress_moved: Notify::new(),
        credentials: parts.credentials,
        http,
        emit,
        conns: Mutex::default(),
        clients: Mutex::default(),
        syncing: Mutex::default(),
        last_sync: Mutex::default(),
        live: Mutex::default(),
    });
    runtime.block_on(main_loop(shared, commands));
}

async fn main_loop(shared: Arc<Shared>, mut commands: mpsc::UnboundedReceiver<Command>) {
    let accounts = lock(&shared.db)
        .store()
        .accounts()
        .list()
        .unwrap_or_default();
    downloads::recover(&shared);
    (shared.emit)(Event::AccountsLoaded(accounts.clone()));
    connect_staggered(&shared, accounts);
    let (playback, playback_commands) = mpsc::unbounded_channel();
    let playback_task = tokio::spawn(playback::run(shared.clone(), playback_commands));
    let works_task = tokio::spawn(works::run(shared.clone()));
    works::request(&shared);
    let mut tick = tokio::time::interval(TICK);
    tick.tick().await;
    loop {
        tokio::select! {
            command = commands.recv() => match command {
                Some(command) => handle(&shared, &playback, command),
                None => break,
            },
            _ = tick.tick() => {
                tokio::spawn(on_tick(shared.clone()));
            }
        }
    }
    drop(playback);
    let _ = playback_task.await;
    works_task.abort();
}

fn handle(
    shared: &Arc<Shared>,
    playback: &mpsc::UnboundedSender<PlaybackCommand>,
    command: Command,
) {
    let forward = |command| {
        let _ = playback.send(command);
    };
    match command {
        Command::StartPlayback {
            account_id,
            item_id,
        } => forward(PlaybackCommand::Start {
            account_id,
            item_id,
        }),
        Command::ReportPlayback {
            item_id,
            position,
            playing,
            force_sync,
        } => forward(PlaybackCommand::Report {
            item_id,
            position,
            playing,
            force_sync,
        }),
        Command::StopPlayback => forward(PlaybackCommand::Stop),
        Command::SetRemoteCheck { seconds } => {
            let _ = lock(&shared.db)
                .store()
                .settings()
                .set(REMOTE_CHECK_SETTING, seconds);
            forward(PlaybackCommand::RemoteCheck { seconds });
        }
        Command::AddAccount {
            url,
            username,
            password,
        } => {
            tokio::spawn(add(shared.clone(), url, username, password));
        }
        Command::AddAccountWithKey { url, api_key } => {
            tokio::spawn(add_with_key(shared.clone(), url, api_key));
        }
        Command::RemoveAccount { account_id } => {
            tokio::spawn(remove(shared.clone(), account_id));
        }
        Command::SetServerUrls { account_id, urls } => set_server_urls(shared, &account_id, &urls),
        Command::SetAccountEnabled {
            account_id,
            enabled,
        } => {
            tokio::spawn(set_enabled(shared.clone(), account_id, enabled));
        }
        command @ Command::ResolveConflict { .. } => offline::handle(shared, command),
        command @ (Command::MergeWorks { .. }
        | Command::SplitFromWork { .. }
        | Command::ResetWorkMatch { .. }
        | Command::SetPreferredAccount { .. }) => works::handle(shared, command),
        Command::SyncNow { account_id } => {
            for session in sessions(shared, account_id.as_deref()) {
                tokio::spawn(sync(shared.clone(), session));
            }
        }
        command @ (Command::Download { .. }
        | Command::PauseDownload { .. }
        | Command::ResumeDownload { .. }
        | Command::DeleteDownload { .. }) => downloads::handle(shared, command),
        command @ (Command::OpenEbook { .. } | Command::SaveEbookProgress { .. }) => {
            ebooks::handle(shared, command)
        }
        request => requests::handle(shared, request),
    }
}

async fn add(shared: Arc<Shared>, url: String, username: String, password: String) {
    let result = add_account(
        &shared.db,
        &shared.credentials,
        shared.http.clone(),
        &url,
        &username,
        &password,
    )
    .await;
    added(&shared, result).await;
}

async fn add_with_key(shared: Arc<Shared>, url: String, api_key: String) {
    let result = add_account_with_key(
        &shared.db,
        &shared.credentials,
        shared.http.clone(),
        &url,
        &api_key,
    )
    .await;
    added(&shared, result).await;
}

async fn added(shared: &Arc<Shared>, result: Result<Session, AccountError>) {
    match result {
        Ok(session) => {
            let account = session.account.clone();
            (shared.emit)(Event::AccountAdded(account.clone()));
            connected(shared, session).await;
        }
        Err(e) => (shared.emit)(Event::AddAccountFailed {
            needs_login: e.needs_login(),
            message: e.to_string(),
        }),
    }
}

/// Connects enabled accounts, spacing out logins to the same server.
fn connect_staggered(shared: &Arc<Shared>, accounts: Vec<Account>) {
    let mut per_server: HashMap<String, u32> = HashMap::new();
    for account in accounts {
        if !account.enabled {
            set_conn(shared, &account.id, Conn::Disabled);
            continue;
        }
        let index = per_server.entry(account.server_id.clone()).or_default();
        let delay = LOGIN_STAGGER * *index;
        *index += 1;
        set_conn(shared, &account.id, Conn::Connecting);
        let shared = shared.clone();
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            connect(shared, account).await;
        });
    }
}

fn start_connect(shared: &Arc<Shared>, account: Account) {
    set_conn(shared, &account.id, Conn::Connecting);
    tokio::spawn(connect(shared.clone(), account));
}

async fn connect(shared: Arc<Shared>, account: Account) {
    let account_id = account.id.clone();
    if !is_enabled(&shared, &account_id) {
        set_conn(&shared, &account_id, Conn::Disabled);
        return;
    }
    let previous = lock_map(&shared.clients)
        .get(&account_id)
        .filter(|client| client.is_logged_in())
        .cloned();
    let http = shared.http.clone();
    let result = match previous {
        Some(previous) => reconnect_account(&shared.db, http, &previous, account).await,
        None => restore_account(&shared.db, &shared.credentials, http, account).await,
    };
    match result {
        Ok(session) => connected(&shared, session).await,
        Err(e) => report_connect_error(&shared, account_id, &e),
    }
}

fn report_connect_error(shared: &Shared, account_id: String, error: &AccountError) {
    if !is_enabled(shared, &account_id) {
        set_conn(shared, &account_id, Conn::Disabled);
        return;
    }
    let event = if error.needs_login() {
        set_conn(shared, &account_id, Conn::NeedsLogin);
        Event::LoginRequired { account_id }
    } else {
        set_conn(shared, &account_id, Conn::Offline);
        Event::Offline {
            account_id,
            message: error.to_string(),
        }
    };
    (shared.emit)(event);
}

async fn connected(shared: &Arc<Shared>, session: Session) {
    let session = Arc::new(session);
    let account_id = session.account.id.clone();
    if !is_enabled(shared, &account_id) {
        set_conn(shared, &account_id, Conn::Disabled);
        return;
    }
    lock_map(&shared.clients).insert(account_id.clone(), session.client.clone());
    set_conn(shared, &account_id, Conn::Connected(session.clone()));
    (shared.emit)(Event::AccountConnected {
        account_id: account_id.clone(),
    });
    if has_realtime(&session.client) {
        let task = tokio::spawn(live(shared.clone(), session.clone()));
        if let Some(old) = lock_map(&shared.live).insert(account_id, task) {
            old.abort();
        }
    }
    downloads::schedule(shared);
    sync(shared.clone(), session).await;
}

/// False for API keys on servers whose socket does not accept them; those accounts
/// rely on the periodic sync instead.
fn has_realtime(client: &AbsClient) -> bool {
    !client.uses_api_key() || client.supports(Feature::ApiKeySocket)
}

/// Applies realtime events of one account until its socket gives up.
async fn live(shared: Arc<Shared>, session: Arc<Session>) {
    let account_id = session.account.id.clone();
    let mut realtime = Realtime::spawn(session.client.clone());
    let mut state = LiveState::default();
    while let Some(event) = realtime.next().await {
        let (event, followup) = match state.apply(&shared.db, &account_id, event) {
            Ok(result) => result,
            Err(_) => (None, Followup::SyncCatalog),
        };
        if let Some(event) = event {
            match event {
                Event::CatalogChanged { .. } => works::request(&shared),
                Event::ProgressChanged { .. } => shared.progress_moved.notify_one(),
                _ => {}
            }
            (shared.emit)(event);
        }
        match followup {
            Followup::Nothing => {}
            Followup::SyncCatalog => {
                tokio::spawn(sync(shared.clone(), session.clone()));
            }
            Followup::SyncProgress => {
                tokio::spawn(pull_progress(shared.clone(), session.clone()));
            }
            Followup::LoginRequired => {
                set_conn(&shared, &account_id, Conn::NeedsLogin);
                (shared.emit)(Event::LoginRequired { account_id });
                return;
            }
        }
    }
}

/// Progress only, as after a socket reconnect.
async fn pull_progress(shared: Arc<Shared>, session: Arc<Session>) {
    let account_id = session.account.id.clone();
    match sync_progress(&session, &shared.db).await {
        Ok(item_ids) if !item_ids.is_empty() => (shared.emit)(Event::ProgressChanged {
            account_id,
            item_ids,
        }),
        Ok(_) => {}
        Err(e) => (shared.emit)(Event::SyncFailed {
            account_id,
            message: e.to_string(),
        }),
    }
}

async fn remove(shared: Arc<Shared>, account_id: String) {
    if let Some(task) = lock_map(&shared.live).remove(&account_id) {
        task.abort();
    }
    downloads::forget_account(&shared, &account_id).await;
    crate::ebooks::forget_account(&shared.cache_root, &account_id);
    lock_map(&shared.clients).remove(&account_id);
    let conn = lock_map(&shared.conns).remove(&account_id);
    if let Some(Conn::Connected(session)) = conn {
        let _ = session.client.logout().await;
    }
    let _ = shared.credentials.delete(&account_id);
    let removed = lock(&shared.db)
        .store()
        .accounts()
        .remove_account(&account_id);
    if removed.is_ok() {
        works::request(&shared);
        (shared.emit)(Event::AccountRemoved { account_id });
    }
}

/// Disabling drops the session and socket but keeps the login, so enabling needs no password.
async fn set_enabled(shared: Arc<Shared>, account_id: String, enabled: bool) {
    let account = {
        let db = lock(&shared.db);
        let accounts = db.store().accounts();
        match accounts.set_enabled(&account_id, enabled) {
            Ok(true) => accounts.account(&account_id).ok().flatten(),
            _ => None,
        }
    };
    let Some(account) = account else {
        return;
    };
    (shared.emit)(Event::AccountUpdated(account.clone()));
    works::request(&shared);
    if enabled {
        start_connect(&shared, account);
        return;
    }
    if let Some(task) = lock_map(&shared.live).remove(&account_id) {
        task.abort();
    }
    set_conn(&shared, &account_id, Conn::Disabled);
    downloads::suspend_account(&shared, &account_id);
}

fn is_enabled(shared: &Shared, account_id: &str) -> bool {
    lock(&shared.db)
        .store()
        .accounts()
        .account(account_id)
        .ok()
        .flatten()
        .is_some_and(|a| a.enabled)
}

/// Catalog then progress sync. Skipped while the account is already syncing.
async fn sync(shared: Arc<Shared>, session: Arc<Session>) {
    let account_id = session.account.id.clone();
    if !lock_map(&shared.syncing).insert(account_id.clone()) {
        return;
    }
    (shared.emit)(Event::SyncStarted {
        account_id: account_id.clone(),
    });
    let result = sync_all(&shared, &session).await;
    lock_map(&shared.syncing).remove(&account_id);
    match result {
        Ok(()) => {
            lock_map(&shared.last_sync).insert(account_id, Instant::now());
        }
        Err(e) if e.needs_login() => {
            set_conn(&shared, &account_id, Conn::NeedsLogin);
            (shared.emit)(Event::LoginRequired { account_id });
        }
        Err(e) if e.is_network() => went_offline(&shared, &account_id, &e),
        Err(e) => (shared.emit)(Event::SyncFailed {
            account_id,
            message: e.to_string(),
        }),
    }
}

async fn sync_all(shared: &Arc<Shared>, session: &Arc<Session>) -> Result<(), AccountError> {
    let account_id = session.account.id.clone();
    let report = sync_catalog(session, &shared.db).await?;
    works::request(shared);
    (shared.emit)(Event::CatalogSynced {
        account_id: account_id.clone(),
        report,
    });
    let item_ids = sync_progress(session, &shared.db).await?;
    if !item_ids.is_empty() {
        (shared.emit)(Event::ProgressChanged {
            account_id,
            item_ids,
        });
    }
    offline::flush(shared.clone(), session.clone()).await;
    Ok(())
}

/// Every minute: reconnect offline accounts and run each connected account's own tick,
/// so a slow server does not hold up the others.
async fn on_tick(shared: Arc<Shared>) {
    for session in sessions(&shared, None) {
        tokio::spawn(account_tick(shared.clone(), session));
    }
    reconnect_offline(&shared);
    downloads::schedule(&shared);
}

/// Keeps the token fresh, checks the server, flushes offline sessions, re-syncs when due.
async fn account_tick(shared: Arc<Shared>, session: Arc<Session>) {
    let account_id = session.account.id.clone();
    if let Err(e) = session.client.refresh_if_due().await
        && e.needs_login()
    {
        set_conn(&shared, &account_id, Conn::NeedsLogin);
        (shared.emit)(Event::LoginRequired { account_id });
        return;
    }
    if !check_connection(&shared, &session).await {
        return;
    }
    offline::flush(shared.clone(), session.clone()).await;
    let last = lock_map(&shared.last_sync).get(&account_id).copied();
    if last.is_none_or(|t| t.elapsed() >= sync_interval(&shared)) {
        sync(shared, session).await;
    }
}

/// Notices a server that went away, and moves to an earlier address that answers again.
/// False when the session should not be used any more.
async fn check_connection(shared: &Arc<Shared>, session: &Session) -> bool {
    if let Err(e) = session.client.ping().await {
        went_offline(shared, &session.account.id, &e.into());
        return false;
    }
    let urls = lock(&shared.db)
        .store()
        .accounts()
        .server_urls(&session.account.server_id)
        .unwrap_or_default();
    let current = session.client.base_url().as_str();
    if earlier_url_answers(&shared.http, &urls, current).await {
        start_connect(shared, session.account.clone());
        return false;
    }
    true
}

/// The server stopped answering: stop its socket and let the tick reconnect.
pub(super) fn went_offline(shared: &Shared, account_id: &str, error: &AccountError) {
    if let Some(task) = lock_map(&shared.live).remove(account_id) {
        task.abort();
    }
    set_conn(shared, account_id, Conn::Offline);
    (shared.emit)(Event::Offline {
        account_id: account_id.to_owned(),
        message: error.to_string(),
    });
}

fn set_server_urls(shared: &Arc<Shared>, account_id: &str, urls: &[String]) {
    let Ok(Some(account)) = lock(&shared.db).store().accounts().account(account_id) else {
        return;
    };
    let urls: Vec<&str> = urls
        .iter()
        .map(|u| u.trim())
        .filter(|u| !u.is_empty())
        .collect();
    if urls.is_empty() {
        return;
    }
    let stored = lock(&shared.db)
        .store()
        .accounts()
        .set_server_urls(&account.server_id, &urls);
    if let Err(e) = stored {
        (shared.emit)(Event::RequestFailed {
            account_id: account_id.to_owned(),
            message: e.to_string(),
        });
        return;
    }
    let listed = |current: &str| {
        urls.iter()
            .any(|u| u.trim_end_matches('/') == current.trim_end_matches('/'))
    };
    let keep = sessions(shared, Some(account_id))
        .first()
        .is_some_and(|s| listed(s.client.base_url().as_str()));
    let waits_for_login = matches!(
        lock_map(&shared.conns).get(account_id),
        Some(Conn::NeedsLogin | Conn::Disabled)
    );
    if !keep && !waits_for_login {
        start_connect(shared, account);
    }
}

/// Retries accounts whose server was unreachable. Accounts that need a login wait for the user.
fn reconnect_offline(shared: &Arc<Shared>) {
    let accounts = lock(&shared.db)
        .store()
        .accounts()
        .list()
        .unwrap_or_default();
    let offline: Vec<Account> = {
        let conns = lock_map(&shared.conns);
        accounts
            .into_iter()
            .filter(|a| a.enabled && matches!(conns.get(&a.id), None | Some(Conn::Offline)))
            .collect()
    };
    for account in offline {
        start_connect(shared, account);
    }
}

fn sync_interval(shared: &Shared) -> Duration {
    let minutes = lock(&shared.db)
        .store()
        .settings()
        .get_or(SYNC_INTERVAL_SETTING, DEFAULT_SYNC_INTERVAL_MINUTES)
        .unwrap_or(DEFAULT_SYNC_INTERVAL_MINUTES);
    Duration::from_secs(minutes.max(1) * 60)
}

/// Connected sessions, all or the one of `account_id`.
pub(super) fn sessions(shared: &Shared, account_id: Option<&str>) -> Vec<Arc<Session>> {
    lock_map(&shared.conns)
        .values()
        .filter_map(|conn| match conn {
            Conn::Connected(session) => Some(session.clone()),
            _ => None,
        })
        .filter(|s| account_id.is_none_or(|id| s.account.id == id))
        .collect()
}

/// Server time minus local time, as last seen for the account.
pub(super) fn clock_offset(shared: &Shared, account_id: &str) -> Option<i64> {
    let clients = lock_map(&shared.clients);
    clients.get(account_id)?.clock_offset_ms()
}

fn set_conn(shared: &Shared, account_id: &str, conn: Conn) {
    lock_map(&shared.conns).insert(account_id.to_owned(), conn);
}

pub(super) fn lock_map<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
