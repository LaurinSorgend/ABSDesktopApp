//! Applies realtime events to the database.

use std::sync::Mutex;

use abs_api::RealtimeEvent;

use super::Event;
use crate::account::lock;
use crate::db::{Db, DbResult, ItemRow, ProgressRow};

/// Work the engine does after an event was applied.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Followup {
    Nothing,
    SyncCatalog,
    SyncProgress,
    LoginRequired,
}

/// Tracks the connection across events of one account.
#[derive(Default)]
pub(super) struct LiveState {
    disconnected: bool,
}

impl LiveState {
    /// Stores what the event changed and says what the UI and engine should do next.
    pub(super) fn apply(
        &mut self,
        db: &Mutex<Db>,
        account_id: &str,
        event: RealtimeEvent,
    ) -> DbResult<(Option<Event>, Followup)> {
        let account = account_id.to_owned();
        let result = match event {
            RealtimeEvent::Ready { .. } if std::mem::take(&mut self.disconnected) => {
                (None, Followup::SyncProgress)
            }
            RealtimeEvent::Disconnected(_) => {
                self.disconnected = true;
                (None, Followup::Nothing)
            }
            RealtimeEvent::AuthFailed(_) => (None, Followup::LoginRequired),
            RealtimeEvent::ProgressUpdated { progress, .. } => {
                let row = ProgressRow::from_api(&progress);
                let changed = lock(db).store().progress().upsert(account_id, &row)?;
                let event = changed.then(|| Event::ProgressChanged {
                    account_id: account,
                    item_ids: vec![row.item_id],
                });
                (event, Followup::Nothing)
            }
            RealtimeEvent::ItemsChanged(items) => {
                let rows: Vec<ItemRow> = items.iter().map(ItemRow::from_api).collect();
                let known = upsert_known(db, account_id, &rows)?;
                let event = Event::CatalogChanged {
                    account_id: account,
                };
                let followup = if known {
                    Followup::Nothing
                } else {
                    Followup::SyncCatalog
                };
                (Some(event), followup)
            }
            RealtimeEvent::ItemRemoved { item_id, .. } => {
                let removed = lock(db)
                    .store()
                    .catalog()
                    .remove_item(account_id, &item_id)?;
                let event = removed.then_some(Event::CatalogChanged {
                    account_id: account,
                });
                (event, Followup::Nothing)
            }
            RealtimeEvent::LibrariesChanged => (None, Followup::SyncCatalog),
            RealtimeEvent::Ready { .. } | RealtimeEvent::SessionClosed(_) => {
                (None, Followup::Nothing)
            }
        };
        Ok(result)
    }
}

/// Upserts items of libraries the database knows. False if some library was unknown.
fn upsert_known(db: &Mutex<Db>, account_id: &str, rows: &[ItemRow]) -> DbResult<bool> {
    let mut db = lock(db);
    db.write(|store| {
        let catalog = store.catalog();
        let mut all_known = true;
        for row in rows {
            if catalog.has_library(account_id, &row.library_id)? {
                catalog.upsert_items(account_id, std::slice::from_ref(row))?;
            } else {
                all_known = false;
            }
        }
        Ok(all_known)
    })
}

#[cfg(test)]
mod tests {
    use abs_api::Raw;

    use super::*;
    use crate::db::NewAccount;

    fn setup() -> (Mutex<Db>, String) {
        let db = Db::open_in_memory().unwrap();
        let accounts = db.store().accounts();
        let server = accounts.add_server("Test", &[]).unwrap();
        let new = NewAccount {
            server_id: &server,
            user_id: "u",
            username: "listener",
        };
        let id = accounts.add_account(&new).unwrap().id;
        (Mutex::new(db), id)
    }

    #[test]
    fn reconnect_pulls_progress_once() {
        let (db, account) = setup();
        let mut state = LiveState::default();
        let ready = || RealtimeEvent::Ready {
            user_id: "u".into(),
        };
        let (_, first) = state.apply(&db, &account, ready()).unwrap();
        assert_eq!(first, Followup::Nothing);
        state
            .apply(&db, &account, RealtimeEvent::Disconnected("x".into()))
            .unwrap();
        let (_, after) = state.apply(&db, &account, ready()).unwrap();
        assert_eq!(after, Followup::SyncProgress);
    }

    #[test]
    fn item_of_unknown_library_asks_for_catalog_sync() {
        let (db, account) = setup();
        let item: Raw<abs_api::models::LibraryItem> =
            serde_json::from_str(r#"{"id":"i","libraryId":"new-lib"}"#).unwrap();
        let mut state = LiveState::default();
        let (event, followup) = state
            .apply(&db, &account, RealtimeEvent::ItemsChanged(vec![item]))
            .unwrap();
        assert!(matches!(event, Some(Event::CatalogChanged { .. })));
        assert_eq!(followup, Followup::SyncCatalog);
    }
}
