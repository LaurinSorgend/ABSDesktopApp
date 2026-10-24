//! The banner: the last error, else the active account's offline or login state.

use super::{App, Conn};

#[derive(Debug, Default)]
pub struct Banner {
    error: Option<String>,
    /// The account state the user dismissed; shown again once it changes.
    dismissed: Option<(String, Conn)>,
}

impl App {
    pub(super) fn show_error(&mut self, text: String) {
        self.banner.error = Some(text);
        self.update_banner();
    }

    pub fn banner_dismiss(&mut self) {
        if self.banner.error.take().is_none() {
            self.banner.dismissed = self.account_state();
        }
        self.update_banner();
    }

    pub fn banner_act(&mut self) {
        if self.banner.error.is_none()
            && let Some((account_id, Conn::NeedsLogin)) = self.account_state()
        {
            self.relogin(&account_id);
        }
    }

    fn account_state(&self) -> Option<(String, Conn)> {
        let id = self.active.clone()?;
        let conn = *self.conns.get(&id)?;
        Some((id, conn))
    }

    pub(super) fn update_banner(&self) {
        let state = self
            .account_state()
            .filter(|s| self.banner.dismissed.as_ref() != Some(s));
        let user = self
            .active_account()
            .map(|a| a.username.clone())
            .unwrap_or_default();
        let server = self.server_label();
        self.ui(|ui, strings| {
            let (text, kind, action) = match (&self.banner.error, state) {
                (Some(error), _) => (error.as_str().into(), 2, "".into()),
                (None, Some((_, Conn::NeedsLogin))) => (
                    strings.invoke_login_required(user.into()),
                    1,
                    strings.invoke_log_in(),
                ),
                (None, Some((_, Conn::Offline))) => {
                    (strings.invoke_offline(server.into()), 0, "".into())
                }
                _ => ("".into(), 0, "".into()),
            };
            ui.set_banner_text(text);
            ui.set_banner_kind(kind);
            ui.set_banner_action(action);
        });
    }
}
