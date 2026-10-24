//! Accounts: the active one, adding, logging in again, removing, connection states.

use abs_core::db::Account;
use abs_core::engine::Command;
use slint::{ModelRc, SharedString, VecModel};

use super::{App, Conn};
use crate::settings::ACTIVE_ACCOUNT;
use crate::{AccountData, Page};

impl App {
    pub(super) fn accounts_loaded(&mut self, accounts: Vec<Account>) {
        for account in &accounts {
            self.conns.insert(account.id.clone(), Conn::Connecting);
        }
        self.accounts = accounts;
        let stored = self
            .db
            .store()
            .settings()
            .get(ACTIVE_ACCOUNT)
            .ok()
            .flatten();
        let active = stored
            .filter(|id| self.accounts.iter().any(|a| &a.id == id))
            .or_else(|| self.accounts.first().map(|a| a.id.clone()));
        match active {
            Some(id) => self.activate(&id),
            None => self.show_add_account(false),
        }
    }

    /// Switches to an account and shows its home page from local data.
    pub fn activate(&mut self, account_id: &str) {
        self.active = Some(account_id.to_owned());
        let _ = self.db.store().settings().set(ACTIVE_ACCOUNT, account_id);
        self.covers.clear();
        self.home.clear();
        self.browse.clear();
        self.load_libraries();
        let label = self.account_label();
        self.ui(|ui, _| ui.set_account_label(label.into()));
        self.request_shelves();
        self.refresh_accounts();
        self.update_banner();
        self.nav.clear();
        self.show_page(Page::Home);
    }

    pub(super) fn account_added(&mut self, account: Account) {
        self.accounts.retain(|a| a.id != account.id);
        self.conns.insert(account.id.clone(), Conn::Connecting);
        let id = account.id.clone();
        self.accounts.push(account);
        self.relogin = None;
        self.ui(|ui, _| {
            ui.set_add_busy(false);
            ui.set_add_error("".into());
            ui.set_add_password("".into());
        });
        self.activate(&id);
    }

    pub(super) fn add_failed(&mut self, message: String) {
        self.ui(|ui, _| {
            ui.set_add_busy(false);
            ui.set_add_error(message.into());
        });
    }

    pub(super) fn set_conn(&mut self, account_id: String, conn: Conn) {
        let is_active = self.active.as_deref() == Some(account_id.as_str());
        let was = self.conns.insert(account_id, conn);
        if is_active && conn == Conn::Connected && was != Some(Conn::Connected) {
            self.request_shelves();
        }
        if conn != Conn::Connected {
            self.syncing = false;
            self.ui(|ui, _| ui.set_syncing(false));
        }
        self.refresh_accounts();
        self.update_banner();
        self.update_search_offer();
    }

    pub(super) fn account_removed(&mut self, account_id: &str) {
        self.accounts.retain(|a| a.id != account_id);
        self.conns.remove(account_id);
        let _ = std::fs::remove_dir_all(self.cover_dir.join(account_id));
        if self.active.as_deref() == Some(account_id) {
            self.active = None;
            match self.accounts.first().map(|a| a.id.clone()) {
                Some(next) => self.activate(&next),
                None => self.show_add_account(false),
            }
        }
        self.refresh_accounts();
    }

    pub(super) fn set_syncing(&mut self, account_id: &str, syncing: bool) {
        if self.active.as_deref() == Some(account_id) {
            self.syncing = syncing;
            self.ui(|ui, _| ui.set_syncing(syncing));
        }
    }

    pub(super) fn sync_failed(&mut self, account_id: &str, message: &str) {
        self.set_syncing(account_id, false);
        let text = self.ui(|_, s| s.invoke_sync_failed(message.into()));
        if let Some(text) = text {
            self.show_error(text.into());
        }
    }

    pub(super) fn request_failed(&mut self, message: &str) {
        self.browse_request_failed();
        let text = self.ui(|_, s| s.invoke_request_failed(message.into()));
        if let Some(text) = text {
            self.show_error(text.into());
        }
    }

    /// From the UI: log in with a new or existing account.
    pub fn add_account(&mut self, url: String, username: String, password: String) {
        self.ui(|ui, _| {
            ui.set_add_busy(true);
            ui.set_add_error("".into());
        });
        self.engine.send(Command::AddAccount {
            url: url.trim().to_owned(),
            username: username.trim().to_owned(),
            password,
        });
    }

    /// The add-account page, prefilled for an account whose login expired.
    pub fn relogin(&mut self, account_id: &str) {
        let Some(account) = self.accounts.iter().find(|a| a.id == account_id) else {
            return;
        };
        let url = self.server_url(account).unwrap_or_default();
        let username = account.username.clone();
        self.relogin = Some(account_id.to_owned());
        self.ui(|ui, strings| {
            ui.set_add_title(strings.invoke_log_in_again_title());
            ui.set_add_url(url.into());
            ui.set_add_username(username.into());
            ui.set_add_password("".into());
            ui.set_add_error("".into());
            ui.set_add_can_cancel(true);
        });
        self.navigate(Page::AddAccount);
    }

    /// The add-account page; `can_cancel` is false while there is no account at all.
    pub fn show_add_account(&mut self, can_cancel: bool) {
        self.relogin = None;
        self.ui(|ui, strings| {
            ui.set_add_title(strings.invoke_add_account_title());
            ui.set_add_url("".into());
            ui.set_add_username("".into());
            ui.set_add_password("".into());
            ui.set_add_error("".into());
            ui.set_add_can_cancel(can_cancel);
        });
        if can_cancel {
            self.navigate(Page::AddAccount);
        } else {
            self.nav.clear();
            self.show_page(Page::AddAccount);
        }
    }

    pub fn remove_account(&mut self, account_id: &str) {
        self.engine.send(Command::RemoveAccount {
            account_id: account_id.to_owned(),
        });
    }

    pub fn sync_now(&mut self) {
        self.engine.send(Command::SyncNow {
            account_id: self.active.clone(),
        });
    }

    fn server_url(&self, account: &Account) -> Option<String> {
        let urls = self
            .db
            .store()
            .accounts()
            .server_urls(&account.server_id)
            .ok()?;
        urls.into_iter().next()
    }

    pub(super) fn server_label(&self) -> String {
        self.active_account()
            .and_then(|a| self.server_url(a))
            .map(|url| host(&url))
            .unwrap_or_default()
    }

    fn account_label(&self) -> String {
        match self.active_account() {
            Some(a) => format!("{} @ {}", a.username, self.server_label()),
            None => String::new(),
        }
    }

    pub(super) fn refresh_accounts(&self) {
        let rows: Vec<AccountData> = self.accounts.iter().map(|a| self.account_row(a)).collect();
        let urls: Vec<SharedString> = self
            .active_server_urls()
            .iter()
            .map(|u| u.as_str().into())
            .collect();
        self.ui(|ui, _| {
            ui.set_accounts(ModelRc::new(VecModel::from(rows)));
            ui.set_server_urls(ModelRc::new(VecModel::from(urls)));
        });
    }

    fn active_server_urls(&self) -> Vec<String> {
        let Some(account) = self.active_account() else {
            return Vec::new();
        };
        self.db
            .store()
            .accounts()
            .server_urls(&account.server_id)
            .unwrap_or_default()
    }

    pub fn add_server_url(&mut self, url: &str) {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            let message = self
                .ui(|_, s| s.invoke_address_needs_scheme())
                .unwrap_or_default();
            self.show_error(message.into());
            return;
        }
        let mut urls = self.active_server_urls();
        if !urls
            .iter()
            .any(|u| u.trim_end_matches('/') == url.trim_end_matches('/'))
        {
            urls.push(url.to_owned());
        }
        self.ui(|ui, _| ui.set_new_server_url("".into()));
        self.save_server_urls(urls);
    }

    pub fn remove_server_url(&mut self, index: usize) {
        let mut urls = self.active_server_urls();
        if index < urls.len() && urls.len() > 1 {
            urls.remove(index);
            self.save_server_urls(urls);
        }
    }

    pub fn raise_server_url(&mut self, index: usize) {
        let mut urls = self.active_server_urls();
        if index > 0 && index < urls.len() {
            urls.swap(index - 1, index);
            self.save_server_urls(urls);
        }
    }

    /// Stores the list right away for the page; the engine stores it too and reconnects if needed.
    fn save_server_urls(&mut self, urls: Vec<String>) {
        let Some(account) = self.active_account().cloned() else {
            return;
        };
        let refs: Vec<&str> = urls.iter().map(String::as_str).collect();
        let _ = self
            .db
            .store()
            .accounts()
            .set_server_urls(&account.server_id, &refs);
        self.engine.send(Command::SetServerUrls {
            account_id: account.id,
            urls,
        });
        self.refresh_accounts();
    }

    fn account_row(&self, account: &Account) -> AccountData {
        let conn = self.conns.get(&account.id).copied();
        let status = self
            .ui(|_, s| match conn {
                Some(Conn::Connected) => s.invoke_status_connected(),
                Some(Conn::Offline) => s.invoke_status_offline(),
                Some(Conn::NeedsLogin) => s.invoke_status_needs_login(),
                _ => s.invoke_status_connecting(),
            })
            .unwrap_or_default();
        AccountData {
            id: account.id.as_str().into(),
            username: account.username.as_str().into(),
            server: self
                .server_url(account)
                .map(|u| host(&u))
                .unwrap_or_default()
                .into(),
            status,
            active: self.active.as_deref() == Some(account.id.as_str()),
            needs_login: conn == Some(Conn::NeedsLogin),
        }
    }
}

/// `https://abs.example.com/` becomes `abs.example.com`.
fn host(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    rest.trim_end_matches('/').to_owned()
}
