//! The updates section of the settings page.

use super::{App, with_app};
use crate::updates::{self, AUTO_CHECK, Outcome};

impl App {
    /// Shows the section in installer builds and looks for an update when the setting asks for it.
    pub fn init_updates(&mut self) {
        if !updates::available() {
            return;
        }
        let auto = self
            .db
            .store()
            .settings()
            .get_or(AUTO_CHECK, true)
            .unwrap_or(true);
        self.ui(|ui, _| {
            ui.set_can_update(true);
            ui.set_update_check(auto);
        });
        if auto {
            self.check_updates();
        }
    }

    /// From the UI: looks for an update and downloads it in the background.
    pub fn check_updates(&mut self) {
        self.ui(|ui, strings| {
            ui.set_update_busy(true);
            ui.set_update_status(strings.invoke_update_checking());
        });
        std::thread::spawn(|| {
            let outcome = updates::check();
            let _ = slint::invoke_from_event_loop(move || {
                with_app(move |app| app.update_checked(outcome));
            });
        });
    }

    fn update_checked(&mut self, outcome: Outcome) {
        self.ui(|ui, strings| {
            let status = match &outcome {
                Outcome::UpToDate => strings.invoke_update_none(),
                Outcome::Ready(version) => strings.invoke_update_ready(version.into()),
                Outcome::Failed(message) => strings.invoke_update_failed(message.into()),
            };
            ui.set_update_busy(false);
            ui.set_update_ready(matches!(outcome, Outcome::Ready(_)));
            ui.set_update_status(status);
        });
    }

    /// From the UI: quits so the updater can replace the app, which it then starts again.
    pub fn apply_update(&mut self) {
        match updates::apply_after_exit() {
            Ok(true) => {
                let _ = slint::quit_event_loop();
            }
            Ok(false) => self.check_updates(),
            Err(e) => self.show_error(e),
        }
    }

    pub(super) fn set_update_check(&mut self, on: bool) {
        let _ = self.db.store().settings().set(AUTO_CHECK, on);
        self.ui(|ui, _| ui.set_update_check(on));
    }
}
