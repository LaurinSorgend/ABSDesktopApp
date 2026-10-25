//! ABS Desktop: an Audiobookshelf client.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod callbacks;
mod covers;
mod format;
mod grid;
mod lru;
mod paths;
mod playback;
mod reader;
mod settings;
mod system;
mod theme;
mod tray;

use std::path::Path;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::Duration;

use abs_core::credentials::Credentials;
use abs_core::db::Db;
use abs_core::engine::{Engine, EngineConfig, Event};
use anyhow::Context;
use slint::ComponentHandle;

use app::{App, Parts, with_app};
use covers::Encoded;
use playback::Playback;

slint::include_modules!();

fn main() -> anyhow::Result<()> {
    let window = AppWindow::new()?;
    let data_dir = paths::data_dir();
    std::fs::create_dir_all(&data_dir).context("create the data folder")?;
    let db_path = data_dir.join("library.db");
    let cover_dir = paths::cache_dir();
    let (credentials, file_credentials) = credentials(&data_dir);

    let engine = Rc::new(start_engine(&db_path, &cover_dir, credentials)?);
    let db = Db::open(&db_path).context("open the database")?;
    let playback = start_player(&engine);
    app::install(App::new(
        &window,
        Parts {
            engine: engine.clone(),
            db,
            cover_dir,
            playback,
            file_credentials,
            theme_file: theme::file_path(&data_dir),
        },
    ));
    callbacks::connect(&window);
    let _tick = start_tick();
    let _system = system::attach(&window);

    window.show()?;
    slint::run_event_loop_until_quit()?;
    window.hide()?;

    with_app(|app| app.shutdown());
    app::uninstall();
    if let Ok(engine) = Rc::try_unwrap(engine) {
        engine.shutdown();
    }
    Ok(())
}

/// The OS keyring, or a private file when there is none or the data folder is overridden.
fn credentials(data_dir: &Path) -> (Credentials, bool) {
    if paths::override_dir().is_some() {
        return (Credentials::files(data_dir.join("credentials")), true);
    }
    match Credentials::system() {
        Ok(credentials) => (credentials, false),
        Err(e) => {
            eprintln!("keyring unavailable, using files: {e}");
            (Credentials::files(data_dir.join("credentials")), true)
        }
    }
}

fn start_engine(
    db_path: &Path,
    cover_dir: &Path,
    credentials: Credentials,
) -> anyhow::Result<Engine> {
    let decoder = Mutex::new(covers::spawn_decoder(|decoded| {
        let _ =
            slint::invoke_from_event_loop(move || with_app(move |app| app.cover_decoded(decoded)));
    }));
    let config = EngineConfig {
        db_path: db_path.to_owned(),
        cover_root: cover_dir.to_owned(),
        download_root: paths::download_dir(),
        credentials,
    };
    Engine::start(config, move |event| forward(event, &decoder)).context("start the engine")
}

/// Engine events go to the UI thread; cover bytes go to the decoder first.
fn forward(event: Event, decoder: &Mutex<Sender<Encoded>>) {
    match event {
        Event::CoverLoaded {
            item_id,
            width,
            bytes,
            ..
        } => {
            let decoder = decoder.lock().unwrap_or_else(|p| p.into_inner());
            let _ = decoder.send(Encoded {
                item_id,
                width,
                bytes,
            });
        }
        event => {
            let _ = slint::invoke_from_event_loop(move || with_app(move |app| app.on_event(event)));
        }
    }
}

fn start_player(engine: &Rc<Engine>) -> Option<Playback> {
    let player = Playback::new(engine.clone(), |event| {
        let _ = slint::invoke_from_event_loop(move || with_app(move |app| app.player_event(event)));
    });
    player
        .map_err(|e| eprintln!("audio player unavailable: {e}"))
        .ok()
}

fn start_tick() -> slint::Timer {
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_secs(1), || {
        with_app(|app| app.tick());
    });
    timer
}
