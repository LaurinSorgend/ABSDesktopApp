//! Phase 0.4 spike: a virtualised cover grid fed from an Audiobookshelf library.
//!
//! `cargo run -p abs-desktop --example cover_grid --release -- [url] [username] [password] [library]`

mod cache;
mod grid;
mod loader;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use anyhow::Result;
use slint::ComponentHandle;

use grid::GridState;
use loader::{Server, spawn_cover_loader};

slint::slint! {
    export { GridWindow, Row, Tile } from "examples/cover_grid/ui.slint";
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args
        .next()
        .unwrap_or_else(|| "http://localhost:13379".into());
    let username = args.next().unwrap_or_else(|| "admin".into());
    let password = args.next().unwrap_or_else(|| "admin-pass".into());
    let library = args.next().unwrap_or_else(|| "Grid".into());

    let runtime = tokio::runtime::Runtime::new()?;
    let server = runtime.block_on(Server::login(&base, &username, &password))?;
    let items = runtime.block_on(server.library_items(&library))?;

    let window = GridWindow::new()?;
    let state = Rc::new(RefCell::new(GridState::new(
        items,
        start_loader(&runtime, server, &window),
    )));
    window.set_rows(state.borrow().rows_model());
    wire_callbacks(&window, &state);
    let _autoscroll = std::env::var_os("SPIKE_AUTOSCROLL").map(|_| start_autoscroll(&window));
    window.run()?;
    Ok(())
}

fn start_loader(
    runtime: &tokio::runtime::Runtime,
    server: Server,
    window: &GridWindow,
) -> tokio::sync::mpsc::UnboundedSender<(usize, String)> {
    let weak = window.as_weak();
    spawn_cover_loader(runtime.handle(), server, move |cover| {
        let _ = weak.upgrade_in_event_loop(move |window| {
            COVER_SINK.with(|sink| {
                if let Some(state) = sink.borrow().as_ref() {
                    state.borrow_mut().cover_loaded(cover);
                    let status = state.borrow().status();
                    println!("{status}");
                    window.set_status(status);
                }
            });
        });
    })
}

/// Scrolls down and back up forever, for memory measurements while browsing.
fn start_autoscroll(window: &GridWindow) -> slint::Timer {
    let weak = window.as_weak();
    let step = Cell::new(-SCROLL_STEP_PX);
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(16),
        move || {
            let Some(window) = weak.upgrade() else { return };
            let next =
                (window.get_scroll_y() + step.get()).clamp(window.get_scroll_limit().min(0.0), 0.0);
            if next == 0.0 || next <= window.get_scroll_limit() {
                step.set(-step.get());
            }
            window.set_scroll_y(next);
        },
    );
    timer
}

const SCROLL_STEP_PX: f32 = 12.0;
const INITIAL_VISIBLE_ROWS: usize = 4;

thread_local! {
    static COVER_SINK: RefCell<Option<Rc<RefCell<GridState>>>> = const { RefCell::new(None) };
}

fn wire_callbacks(window: &GridWindow, state: &Rc<RefCell<GridState>>) {
    COVER_SINK.with(|sink| *sink.borrow_mut() = Some(state.clone()));

    let (weak, columns_state) = (window.as_weak(), state.clone());
    window.on_columns_changed(move |columns| {
        columns_state
            .borrow_mut()
            .set_columns(columns.max(1) as usize);
        if let Some(window) = weak.upgrade() {
            window.set_status(columns_state.borrow().status());
        }
    });

    let (weak, range_state) = (window.as_weak(), state.clone());
    window.on_visible_range_changed(move |first, last| {
        range_state
            .borrow_mut()
            .show_rows(first.max(0) as usize, last.max(0) as usize);
        if let Some(window) = weak.upgrade() {
            window.set_status(range_state.borrow().status());
        }
    });

    state
        .borrow_mut()
        .set_columns(window.get_columns().max(1) as usize);
    state.borrow_mut().show_rows(0, INITIAL_VISIBLE_ROWS);
    window.set_status(state.borrow().status());
}
