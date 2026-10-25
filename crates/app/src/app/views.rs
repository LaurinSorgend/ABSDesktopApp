//! The mini player and full screen. Both are the main window in another shape: the mini
//! player makes it small and frameless, full screen covers the screen with the player.

use slint::{ComponentHandle, LogicalSize, PhysicalPosition, PhysicalSize};

use super::App;
use crate::{AppWindow, Ui};

/// Position and size of the mini player, as `x,y,width,height` in physical pixels.
pub const MINI_GEOMETRY: &str = "mini_geometry";
pub const MINI_ON_TOP: &str = "mini_on_top";

/// The mini player's size before the user changes it.
const MINI_SIZE: LogicalSize = LogicalSize {
    width: 440.0,
    height: 184.0,
};

pub const MAIN: i32 = 0;
pub const MINI: i32 = 1;
pub const FULL: i32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Geometry {
    pub fn parse(text: &str) -> Option<Self> {
        let parts: Vec<&str> = text.split(',').map(str::trim).collect();
        let [x, y, width, height] = parts.as_slice() else {
            return None;
        };
        let geometry = Self {
            x: x.parse().ok()?,
            y: y.parse().ok()?,
            width: width.parse().ok()?,
            height: height.parse().ok()?,
        };
        (geometry.width > 0 && geometry.height > 0).then_some(geometry)
    }

    pub fn to_setting(self) -> String {
        format!("{},{},{},{}", self.x, self.y, self.width, self.height)
    }

    fn of(window: &slint::Window) -> Self {
        let (position, size) = (window.position(), window.size());
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }

    fn apply(self, window: &slint::Window) {
        window.set_size(PhysicalSize::new(self.width, self.height));
        window.set_position(PhysicalPosition::new(self.x, self.y));
    }
}

/// The main window as it was before the mini player or full screen took over.
#[derive(Debug, Default)]
pub struct Views {
    main: Option<(Geometry, bool)>,
}

impl App {
    /// Switches between the whole window, the mini player and full screen.
    pub fn set_view(&mut self, view: i32) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let from = window.global::<Ui>().get_view();
        if from == view {
            return;
        }
        let native = window.window();
        match from {
            MAIN => {
                self.views.main = Some((Geometry::of(native), native.is_maximized()));
            }
            MINI => self.save_mini(&window),
            _ => native.set_fullscreen(false),
        }
        window.global::<Ui>().set_view(view);
        if from == MINI {
            self.restore_main(&window);
        }
        match view {
            MINI => self.show_mini(&window),
            FULL => native.set_fullscreen(true),
            _ => {}
        }
    }

    pub fn toggle_mini_on_top(&mut self) {
        let on_top = !self.ui(|ui, _| ui.get_mini_on_top()).unwrap_or(false);
        let _ = self.db.store().settings().set(MINI_ON_TOP, on_top);
        self.ui(|ui, _| ui.set_mini_on_top(on_top));
    }

    pub(super) fn load_view_settings(&self) {
        let on_top = self
            .db
            .store()
            .settings()
            .get_or(MINI_ON_TOP, false)
            .unwrap_or(false);
        self.ui(|ui, _| ui.set_mini_on_top(on_top));
    }

    /// Keeps the mini player's place when the app quits while it shows.
    pub(super) fn save_views(&self) {
        if let Some(window) = self.window.upgrade()
            && window.global::<Ui>().get_view() == MINI
        {
            self.save_mini(&window);
        }
    }

    fn save_mini(&self, window: &AppWindow) {
        let geometry = Geometry::of(window.window());
        let _ = self
            .db
            .store()
            .settings()
            .set(MINI_GEOMETRY, geometry.to_setting());
    }

    fn show_mini(&self, window: &AppWindow) {
        let native = window.window();
        native.set_maximized(false);
        let saved = self
            .db
            .store()
            .settings()
            .get(MINI_GEOMETRY)
            .ok()
            .flatten()
            .and_then(|text| Geometry::parse(&text));
        match saved {
            Some(geometry) => geometry.apply(native),
            None => native.set_size(MINI_SIZE),
        }
    }

    fn restore_main(&self, window: &AppWindow) {
        let Some((geometry, maximized)) = self.views.main else {
            return;
        };
        let native = window.window();
        geometry.apply(native);
        native.set_maximized(maximized);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_round_trips() {
        let geometry = Geometry {
            x: -1920,
            y: 40,
            width: 440,
            height: 184,
        };
        assert_eq!(Geometry::parse(&geometry.to_setting()), Some(geometry));
        assert_eq!(Geometry::parse("1, 2, 3, 4").map(|g| g.width), Some(3));
        assert_eq!(Geometry::parse("1,2,0,4"), None);
        assert_eq!(Geometry::parse("1,2,3"), None);
        assert_eq!(Geometry::parse("a,2,3,4"), None);
    }
}
