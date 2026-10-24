//! Starting the engine against a temporary folder and waiting for its events.
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use abs_core::credentials::Credentials;
use abs_core::engine::{Engine, EngineConfig, Event};

const WAIT: Duration = Duration::from_secs(30);

pub fn start(db_path: &std::path::Path, credentials: &Credentials) -> (Engine, Receiver<Event>) {
    let (sender, events) = channel();
    let config = EngineConfig {
        db_path: db_path.to_owned(),
        cover_root: db_path.with_file_name("covers"),
        download_root: db_path.with_file_name("downloads"),
        credentials: credentials.clone(),
    };
    let engine = Engine::start(config, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    (engine, events)
}

/// Waits for the first event `pick` accepts and returns what it extracted.
pub fn wait_for<T>(events: &Receiver<Event>, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    loop {
        let event = events.recv_timeout(WAIT).expect("engine event");
        if let Some(value) = pick(&event) {
            return value;
        }
        assert!(
            !matches!(
                event,
                Event::AddAccountFailed { .. }
                    | Event::SyncFailed { .. }
                    | Event::RequestFailed { .. }
            ),
            "{event:?}"
        );
    }
}
