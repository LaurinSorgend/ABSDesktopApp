//! Renders the player views in every bundled theme to PNG files, to look at by eye:
//! `SCREENS_DIR=<folder> cargo test -p abs-desktop screens -- --ignored`.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, PointerEventButton, WindowAdapter, WindowEvent};
use slint::{
    ComponentHandle, Image, LogicalPosition, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel,
};

use crate::theme::{Catalog, ThemeChoice, ThemeFile, resolve};
use crate::{AppWindow, Page, Theme, Ui};

struct Offscreen(Rc<MinimalSoftwareWindow>);

impl Platform for Offscreen {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

/// One shot: a view at a window size, optionally with a click before rendering.
struct Shot {
    name: &'static str,
    view: i32,
    size: (u32, u32),
    /// Where to click, measured from the bottom right corner.
    click_from_corner: Option<(f32, f32)>,
}

const SHOTS: [Shot; 4] = [
    Shot {
        name: "player",
        view: 0,
        size: (1280, 800),
        click_from_corner: None,
    },
    Shot {
        name: "speed-list",
        view: 0,
        size: (1280, 800),
        click_from_corner: Some((40.0, 62.0)),
    },
    Shot {
        name: "mini",
        view: 1,
        size: (440, 184),
        click_from_corner: None,
    },
    Shot {
        name: "full",
        view: 2,
        size: (1280, 800),
        click_from_corner: None,
    },
];

#[test]
#[ignore = "writes images for a person to look at"]
fn screens() {
    let out = PathBuf::from(std::env::var("SCREENS_DIR").expect("set SCREENS_DIR"));
    std::fs::create_dir_all(&out).unwrap();
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Offscreen(window.clone()))).unwrap();
    let catalog = Catalog::load(Path::new("no installed themes"));
    let only = std::env::var("SCREENS_THEMES").unwrap_or_default();
    for def in catalog
        .themes
        .iter()
        .filter(|t| only.is_empty() || only.split(',').any(|o| o == t.id))
    {
        for (mode, mode_name) in [(2, "dark"), (1, "light")] {
            for shot in &SHOTS {
                let file = out.join(format!("{}-{mode_name}-{}.png", def.id, shot.name));
                render(&window, &catalog, &def.id, mode, shot, &file);
            }
        }
    }
}

fn render(
    window: &Rc<MinimalSoftwareWindow>,
    catalog: &Catalog,
    theme: &str,
    mode: usize,
    shot: &Shot,
    file: &Path,
) {
    let app = AppWindow::new().unwrap();
    let choice = ThemeChoice {
        theme: theme.into(),
        mode,
        font_scale: crate::theme::DEFAULT_FONT_SCALE,
        ..ThemeChoice::default()
    };
    let resolved = resolve(&choice, catalog, &ThemeFile::default());
    let globals = app.global::<Theme>();
    globals.set_light_colors(resolved.light);
    globals.set_dark_colors(resolved.dark);
    globals.set_look(resolved.look);
    globals.set_mode(mode as i32);
    globals.set_motion(false);
    fill_player(&app.global::<Ui>(), shot.view);
    let (width, height) = shot.size;
    window.set_size(slint::PhysicalSize::new(width, height));
    app.show().unwrap();
    slint::platform::update_timers_and_animations();
    if let Some((dx, dy)) = shot.click_from_corner {
        let position = LogicalPosition::new(width as f32 - dx, height as f32 - dy);
        let button = PointerEventButton::Left;
        window.dispatch_event(WindowEvent::PointerMoved { position });
        window.dispatch_event(WindowEvent::PointerPressed { position, button });
        window.dispatch_event(WindowEvent::PointerReleased { position, button });
        slint::platform::update_timers_and_animations();
    }
    save(window, width, height, file);
    app.hide().unwrap();
}

fn save(window: &MinimalSoftwareWindow, width: u32, height: u32, file: &Path) {
    let mut buffer = vec![PremultipliedRgbaColor::default(); (width * height) as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });
    let bytes: Vec<u8> = buffer
        .iter()
        .flat_map(|p| [p.red, p.green, p.blue, 255])
        .collect();
    image::RgbaImage::from_raw(width, height, bytes)
        .unwrap()
        .save(file)
        .unwrap();
}

/// A loaded book halfway through a chapter.
fn fill_player(ui: &Ui<'_>, view: i32) {
    let strings = |items: &[&str]| -> ModelRc<slint::SharedString> {
        ModelRc::new(VecModel::from(
            items
                .iter()
                .map(|s| (*s).into())
                .collect::<Vec<slint::SharedString>>(),
        ))
    };
    ui.set_page(Page::Player);
    ui.set_view(view);
    ui.set_has_media(true);
    ui.set_now_title("The Mesopotamian Riddle".into());
    ui.set_now_author("Agatha Christie".into());
    ui.set_chapter_title("Chapter 2: Persepolis".into());
    ui.set_position_text("41:59".into());
    ui.set_remaining_text("5:19:22".into());
    ui.set_chapter_elapsed_text("10:18".into());
    ui.set_chapter_remaining_text("7:34".into());
    ui.set_position_fraction(0.12);
    ui.set_chapter_fraction(0.58);
    ui.set_chapter_markers(ModelRc::new(VecModel::from(vec![
        0.05f32, 0.11, 0.2, 0.32, 0.47, 0.6, 0.78,
    ])));
    ui.set_progress_mode(2);
    ui.set_volume(70.0);
    ui.set_speed_options(strings(&[
        "0.75×", "1×", "1.1×", "1.25×", "1.5×", "1.75×", "2×", "2.5×",
    ]));
    ui.set_speed_index(5);
    ui.set_sleep_options(strings(&[
        "Off",
        "10 min",
        "15 min",
        "30 min",
        "45 min",
        "60 min",
        "End of chapter",
    ]));
    ui.set_skip_back_text("10 s".into());
    ui.set_skip_forward_text("30 s".into());
    ui.set_account_label("laurin @ books.example".into());
    ui.set_now_cover(cover(256, false));
    ui.set_now_has_cover(true);
    ui.set_now_backdrop(cover(96, true));
    ui.set_now_has_backdrop(true);
}

/// A warm painted-looking cover, or its blurred copy.
fn cover(size: u32, soft: bool) -> Image {
    let mut pixels = SharedPixelBuffer::<Rgba8Pixel>::new(size, size);
    for (i, p) in pixels.make_mut_slice().iter_mut().enumerate() {
        let (x, y) = (
            (i as u32 % size) as f32 / size as f32,
            (i as u32 / size) as f32 / size as f32,
        );
        let band = if soft {
            0.0
        } else {
            ((y * 14.0).floor() % 2.0) * 18.0
        };
        *p = Rgba8Pixel {
            r: (200.0 - 120.0 * y + band) as u8,
            g: (150.0 - 90.0 * y + 40.0 * x) as u8,
            b: (90.0 + 60.0 * x) as u8,
            a: 255,
        };
    }
    Image::from_rgba8(pixels)
}
