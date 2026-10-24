//! OS integration: media keys and what closing the window does.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use abs_player::media_controls::system_media_controls;
use slint::{CloseRequestResponse, ComponentHandle};

use crate::AppWindow;
use crate::app::with_app;

/// Media controls need the native window, which exists only once the window is shown.
const ATTACH_DELAY: Duration = Duration::from_millis(300);

/// Keeps OS integration alive while the app runs.
pub struct System {
    _attach: slint::Timer,
}

pub fn attach(window: &AppWindow) -> System {
    let weak = window.as_weak();
    let attach = slint::Timer::default();
    attach.start(slint::TimerMode::SingleShot, ATTACH_DELAY, move || {
        if let Some(window) = weak.upgrade() {
            attach_media_controls(&window);
        }
        with_app(|app| app.focus_input());
    });
    window.window().on_close_requested(on_close);
    System { _attach: attach }
}

fn attach_media_controls(window: &AppWindow) {
    let hwnd = match native_hwnd(window) {
        Ok(hwnd) => hwnd,
        Err(e) => {
            eprintln!("media controls: {e}");
            return;
        }
    };
    let controls = system_media_controls(hwnd, |key| {
        let _ = slint::invoke_from_event_loop(move || with_app(move |app| app.media_key(key)));
    });
    match controls {
        Ok(controls) => with_app(move |app| app.set_media_controls(controls)),
        Err(e) => eprintln!("media controls: {e}"),
    }
}

/// Quits, unless the user chose to keep playing in the tray.
fn on_close() -> CloseRequestResponse {
    let keep_running = Rc::new(Cell::new(false));
    let answer = keep_running.clone();
    with_app(move |app| answer.set(app.keeps_running_on_close()));
    if !keep_running.get() {
        let _ = slint::quit_event_loop();
    }
    CloseRequestResponse::HideWindow
}

#[cfg(windows)]
fn native_hwnd(window: &AppWindow) -> Result<Option<*mut std::ffi::c_void>, String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = window.window().window_handle();
    let handle = handle.window_handle().map_err(|e| e.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(Some(h.hwnd.get() as *mut std::ffi::c_void)),
        other => Err(format!("unexpected window handle {other:?}")),
    }
}

#[cfg(not(windows))]
fn native_hwnd(_window: &AppWindow) -> Result<Option<*mut std::ffi::c_void>, String> {
    Ok(None)
}
