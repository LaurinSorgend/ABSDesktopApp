//! The question when offline listening and another device disagree on a position.

use abs_core::engine::Command;
use abs_core::offline::Conflict;

use super::App;
use crate::format;

impl App {
    /// Queues a conflict; the first one waiting is shown.
    pub(super) fn conflict_found(&mut self, conflict: Conflict) {
        let known = self
            .conflicts
            .iter()
            .any(|c| c.account_id == conflict.account_id && c.item_id == conflict.item_id);
        if !known {
            self.conflicts.push(conflict);
        }
        self.show_conflict();
    }

    /// Answers the shown conflict and moves on to the next.
    pub fn resolve_conflict(&mut self, keep_local: bool) {
        if self.conflicts.is_empty() {
            return;
        }
        let conflict = self.conflicts.remove(0);
        self.engine.send(Command::ResolveConflict {
            account_id: conflict.account_id,
            item_id: conflict.item_id,
            keep_local,
        });
        self.show_conflict();
    }

    fn show_conflict(&self) {
        let conflict = self.conflicts.first().cloned();
        self.ui(|ui, s| match conflict {
            Some(c) => {
                let here = format::clock(c.local_position);
                let there = format::clock(c.server_position);
                ui.set_conflict_text(s.invoke_conflict(
                    c.title.as_str().into(),
                    here.as_str().into(),
                    there.as_str().into(),
                ));
                ui.set_conflict_keep_text(s.invoke_conflict_keep(here.into()));
                ui.set_conflict_take_text(s.invoke_conflict_take(there.into()));
            }
            None => ui.set_conflict_text("".into()),
        });
    }
}
