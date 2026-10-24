//! UI-thread state: accounts, pages and the player, driven by engine and player events.

mod accounts;
mod banner;
mod browse;
mod detail;
mod home;
mod player;
mod preferences;
mod themes;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use abs_core::db::{Account, Db, LibraryRow};
use abs_core::engine::{Engine, Event};
use slint::ComponentHandle;

use crate::covers::{Covers, Decoded};
use crate::playback::Playback;
use crate::playback::view::PlayerView;
use crate::settings::Choices;
use crate::{AppWindow, Page, Strings, Ui};

pub use banner::Banner;
pub use player::Action;

/// Connection state of an account as the engine reported it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Conn {
    Connecting,
    Connected,
    Offline,
    NeedsLogin,
}

pub struct App {
    window: slint::Weak<AppWindow>,
    engine: Rc<Engine>,
    db: Db,
    covers: Covers,
    cover_dir: PathBuf,
    accounts: Vec<Account>,
    conns: HashMap<String, Conn>,
    syncing: bool,
    active: Option<String>,
    libraries: Vec<LibraryRow>,
    library: Option<usize>,
    nav: Vec<Page>,
    banner: Banner,
    home: home::Home,
    browse: browse::Browse,
    detail: detail::Detail,
    playback: Option<Playback>,
    player_view: PlayerView,
    choices: Choices,
    relogin: Option<String>,
    file_credentials: bool,
    theme_file: PathBuf,
}

/// What [`App::new`] needs besides the window.
pub struct Parts {
    pub engine: Rc<Engine>,
    pub db: Db,
    pub cover_dir: PathBuf,
    pub playback: Option<Playback>,
    pub file_credentials: bool,
    pub theme_file: PathBuf,
}

impl App {
    pub fn new(window: &AppWindow, parts: Parts) -> Self {
        let choices = Choices::load(&parts.db.store());
        let mut app = Self {
            window: window.as_weak(),
            covers: Covers::new(parts.engine.clone()),
            engine: parts.engine,
            db: parts.db,
            cover_dir: parts.cover_dir,
            accounts: Vec::new(),
            conns: HashMap::new(),
            syncing: false,
            active: None,
            libraries: Vec::new(),
            library: None,
            nav: Vec::new(),
            banner: Banner::default(),
            home: home::Home::new(),
            browse: browse::Browse::new(),
            detail: detail::Detail::new(),
            playback: parts.playback,
            player_view: PlayerView::new(),
            choices,
            relogin: None,
            file_credentials: parts.file_credentials,
            theme_file: parts.theme_file,
        };
        app.bind_models();
        app.apply_choices();
        app.apply_theme();
        app
    }

    fn bind_models(&self) {
        self.ui(|ui, strings| {
            ui.set_shelves(self.home.model());
            ui.set_library_rows(self.browse.library_grid.model());
            ui.set_group_rows(self.browse.group_grid.model());
            ui.set_search_rows(self.browse.search_grid.model());
            ui.set_detail_chapters(self.detail.model());
            ui.set_chapters(self.player_view.model());
            crate::playback::view::fill_options(ui, strings);
        });
    }

    /// Runs `f` with the `Ui` and `Strings` globals while the window exists.
    fn ui<R>(&self, f: impl FnOnce(&Ui<'_>, &Strings<'_>) -> R) -> Option<R> {
        let window = self.window.upgrade()?;
        let ui = window.global::<Ui>();
        let strings = window.global::<Strings>();
        Some(f(&ui, &strings))
    }

    fn active_account(&self) -> Option<&Account> {
        let id = self.active.as_deref()?;
        self.accounts.iter().find(|a| a.id == id)
    }

    fn is_connected(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|id| self.conns.get(id) == Some(&Conn::Connected))
    }

    fn library_id(&self) -> Option<String> {
        self.library
            .and_then(|i| self.libraries.get(i))
            .map(|l| l.remote_id.clone())
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::AccountsLoaded(accounts) => self.accounts_loaded(accounts),
            Event::AccountAdded(account) => self.account_added(account),
            Event::AddAccountFailed { message, .. } => self.add_failed(message),
            Event::AccountConnected { account_id } => self.set_conn(account_id, Conn::Connected),
            Event::AccountRemoved { account_id } => self.account_removed(&account_id),
            Event::LoginRequired { account_id } => self.set_conn(account_id, Conn::NeedsLogin),
            Event::Offline { account_id, .. } => self.set_conn(account_id, Conn::Offline),
            Event::SyncStarted { account_id } => self.set_syncing(&account_id, true),
            Event::CatalogSynced { account_id, .. } | Event::CatalogChanged { account_id } => {
                self.catalog_changed(&account_id)
            }
            Event::ProgressChanged {
                account_id,
                item_ids,
            } => self.progress_changed(&account_id, &item_ids),
            Event::SyncFailed {
                account_id,
                message,
            } => self.sync_failed(&account_id, &message),
            Event::RequestFailed { message, .. } => self.request_failed(&message),
            event => self.on_data_event(event),
        }
    }

    /// Answers to requests the pages made.
    fn on_data_event(&mut self, event: Event) {
        match event {
            Event::PlaybackStarted(start) => self.playback_started(*start),
            Event::PlaybackFailed { message } => self.playback_failed(&message),
            Event::PlaybackHeaders { headers } => {
                if let Some(playback) = &self.playback {
                    playback.set_headers(&headers);
                }
            }
            Event::ItemLoaded { item_id, json, .. } => self.item_loaded(&item_id, &json),
            Event::ShelvesLoaded {
                library_id,
                shelves,
                ..
            } => self.shelves_loaded(library_id, shelves),
            Event::SearchResults { text, item_ids, .. } => self.server_results(&text, item_ids),
            Event::GroupsLoaded {
                library_id,
                kind,
                groups,
                ..
            } => self.groups_loaded(library_id, kind, groups),
            Event::GroupItems {
                group_id, item_ids, ..
            } => self.group_items(&group_id, &item_ids),
            Event::BookmarksChanged { item_id, .. } => self.bookmarks_changed(&item_id),
            _ => {}
        }
    }

    /// A cover came back from the decoder; show it wherever its item is.
    pub fn cover_decoded(&mut self, decoded: Decoded) {
        let (item_id, width) = (decoded.item_id.clone(), decoded.width);
        let (image, evicted) = self.covers.loaded(decoded);
        for id in &evicted {
            self.browse.set_cover(id, None);
        }
        if width == crate::covers::TILE_WIDTH {
            self.browse.set_cover(&item_id, image.as_ref());
            self.home.set_cover(&item_id, image.as_ref());
        } else {
            self.detail_cover(&item_id, image.as_ref());
            self.now_cover(&item_id, image.as_ref());
        }
    }

    pub fn navigate(&mut self, page: Page) {
        let current = self.ui(|ui, _| ui.get_page()).unwrap_or(Page::Home);
        if current == page {
            return;
        }
        if current != Page::AddAccount {
            self.nav.push(current);
        }
        self.show_page(page);
    }

    pub fn back(&mut self) {
        let current = self.ui(|ui, _| ui.get_page());
        if current == Some(Page::AddAccount) && self.accounts.is_empty() {
            return;
        }
        if let Some(page) = self.nav.pop() {
            self.show_page(page);
        }
    }

    fn show_page(&mut self, page: Page) {
        if page == Page::Settings {
            self.refresh_settings();
        }
        if page == Page::Home {
            self.refresh_home();
        }
        let can_go_back = !self.nav.is_empty();
        self.ui(|ui, _| {
            ui.set_page(page);
            ui.set_can_go_back(can_go_back);
        });
        if matches!(page, Page::Search | Page::AddAccount) {
            self.focus_input();
        }
    }

    /// Focuses the text field of the current page.
    pub fn focus_input(&self) {
        self.ui(|ui, _| ui.set_focus_request(ui.get_focus_request() + 1));
    }

    /// Before the window closes: last position to the server, then stop the player.
    pub fn shutdown(&mut self) {
        if let Some(playback) = &mut self.playback {
            playback.shutdown();
        }
    }
}

thread_local! {
    static APP: RefCell<Option<Rc<RefCell<App>>>> = const { RefCell::new(None) };
}

/// Makes `app` the target of [`with_app`].
pub fn install(app: App) {
    APP.with(|slot| *slot.borrow_mut() = Some(Rc::new(RefCell::new(app))));
}

/// Runs `f` on the app. While the app is busy (a callback fired during an update),
/// `f` runs on the next turn of the event loop instead.
pub fn with_app(f: impl FnOnce(&mut App) + 'static) {
    let Some(app) = APP.with(|slot| slot.borrow().clone()) else {
        return;
    };
    match app.try_borrow_mut() {
        Ok(mut app) => f(&mut app),
        Err(_) => {
            slint::Timer::single_shot(std::time::Duration::ZERO, move || with_app(f));
        }
    }
}

/// Drops the app, which stops the player.
pub fn uninstall() {
    APP.with(|slot| slot.borrow_mut().take());
}
