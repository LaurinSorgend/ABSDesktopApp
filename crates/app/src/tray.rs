//! The tray icon that keeps the app reachable while it plays with the window closed.
//! Windows only for now; on Linux it needs a GTK main loop that Slint does not run.

use crate::app::{Action, with_app};

/// Labels for the tray menu.
#[cfg_attr(not(windows), allow(dead_code, reason = "read by the Windows tray"))]
pub struct Labels {
    pub show: String,
    pub toggle: String,
    pub quit: String,
}

#[cfg(windows)]
pub use windows::Tray;

#[cfg(windows)]
mod windows {
    use tray_icon::menu::{Menu, MenuEvent, MenuItem};
    use tray_icon::{Icon, MouseButton, TrayIcon, TrayIconBuilder, TrayIconEvent};

    use super::{Labels, on_menu, show_window};

    const SIZE: u32 = 32;

    pub struct Tray {
        _icon: TrayIcon,
    }

    impl Tray {
        pub fn new(labels: &Labels, tooltip: &str, accent: [u8; 3]) -> Result<Self, String> {
            let menu = Menu::new();
            let items = [
                MenuItem::with_id("show", &labels.show, true, None),
                MenuItem::with_id("toggle", &labels.toggle, true, None),
                MenuItem::with_id("quit", &labels.quit, true, None),
            ];
            for item in &items {
                menu.append(item).map_err(|e| e.to_string())?;
            }
            MenuEvent::set_event_handler(Some(|event: MenuEvent| {
                let id = event.id().0.clone();
                let _ = slint::invoke_from_event_loop(move || on_menu(&id));
            }));
            TrayIconEvent::set_event_handler(Some(|event| {
                if let TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                } = event
                {
                    let _ = slint::invoke_from_event_loop(show_window);
                }
            }));
            let icon =
                Icon::from_rgba(icon_pixels(accent), SIZE, SIZE).map_err(|e| e.to_string())?;
            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip(tooltip)
                .with_icon(icon)
                .build()
                .map_err(|e| e.to_string())?;
            Ok(Self { _icon: icon })
        }
    }

    /// A square in the accent color with a white play triangle.
    fn icon_pixels(accent: [u8; 3]) -> Vec<u8> {
        let mut pixels = Vec::with_capacity((SIZE * SIZE * 4) as usize);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let (fx, fy) = (x as f32, y as f32);
                let half = (fx - 10.0) * 0.6;
                let inside = (10.0..=24.0).contains(&fx) && (fy - 16.0).abs() <= 8.0 - half;
                let [r, g, b] = if inside { [255, 255, 255] } else { accent };
                pixels.extend_from_slice(&[r, g, b, 255]);
            }
        }
        pixels
    }
}

/// Without a tray the window always quits on close.
#[cfg(not(windows))]
pub struct Tray;

#[cfg(not(windows))]
impl Tray {
    pub fn new(_labels: &Labels, _tooltip: &str, _accent: [u8; 3]) -> Result<Self, String> {
        Err("no tray on this platform".into())
    }
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "called from the Windows tray")
)]
fn on_menu(id: &str) {
    match id {
        "show" => show_window(),
        "toggle" => with_app(|app| app.player_action(Action::Toggle)),
        "quit" => {
            let _ = slint::quit_event_loop();
        }
        _ => {}
    }
}

#[cfg_attr(
    not(windows),
    allow(dead_code, reason = "called from the Windows tray")
)]
fn show_window() {
    with_app(|app| app.show_window());
}
