//! Applies the chosen theme and `theme.json`, and fills the appearance settings: the
//! theme list, palettes, accents and fonts.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use super::{App, with_app};
use crate::backdrop::{self, Frame};
use crate::settings::{ACCENT, DARK_PALETTE, LIGHT_PALETTE, THEME, per_theme};
use crate::theme::{self, BUNDLED_FONTS, Resolved, ThemeDef, ThemeFile};
use crate::{Colors, Strings, Theme, ThemeCard, Ui};

impl App {
    /// Recomputes colors and shapes from the settings, the theme and `theme.json`.
    pub fn apply_theme(&mut self) {
        let choice = self.choices.theme.clone();
        let loaded = theme::load(&self.theme_file);
        let file = loaded.clone().unwrap_or_default();
        let resolved = theme::resolve(&choice, &self.catalog, &file);
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let globals = window.global::<Theme>();
        globals.set_light_colors(resolved.light.clone());
        globals.set_dark_colors(resolved.dark.clone());
        globals.set_look(resolved.look.clone());
        globals.set_mode(choice.mode as i32);
        globals.set_motion(self.choices.animations);
        let dark = globals.get_dark();
        globals.set_mica(backdrop::apply(&window, &frame(&resolved, dark)));
        let ui = window.global::<Ui>();
        let (names, swatches): (Vec<SharedString>, Vec<slint::Color>) =
            theme::accents(&self.catalog, &resolved, dark)
                .into_iter()
                .map(|(name, color)| (SharedString::from(name), color))
                .unzip();
        ui.set_accent_swatches(ModelRc::new(VecModel::from(swatches)));
        ui.set_accent_names(ModelRc::new(VecModel::from(names)));
        ui.set_accent_index(resolved.accent as i32);
        ui.set_theme_mode(choice.mode as i32);
        ui.set_font_scale_index(choice.font_scale as i32);
        ui.set_radius_index(choice.radius as i32);
        ui.set_density_index(choice.density as i32);
        ui.set_border_index(choice.border as i32);
        ui.set_focus_index(choice.focus as i32);
        ui.set_theme_file(self.theme_file.display().to_string().into());
        ui.set_theme_file_active(loaded.as_ref().is_ok_and(|f| !f.keys().is_empty()));
        ui.set_theme_note(theme_note(&loaded, &window.global::<Strings>()));
        self.show_theme_info(&ui, &resolved, dark);
        self.theme = Some(resolved);
        self.refresh_fonts();
    }

    /// The theme list, the chosen theme's details and its palettes.
    fn show_theme_info(&self, ui: &Ui<'_>, resolved: &Resolved, dark: bool) {
        let cards: Vec<ThemeCard> = self.catalog.themes.iter().map(|t| card(t, dark)).collect();
        let def = &self.catalog.themes[resolved.theme];
        let meta = &def.meta;
        let palettes: Vec<SharedString> = def
            .palettes
            .iter()
            .map(|p| p.name.as_str().into())
            .collect();
        ui.set_theme_cards(ModelRc::new(VecModel::from(cards)));
        ui.set_theme_index(resolved.theme as i32);
        ui.set_theme_description(meta.description.as_str().into());
        ui.set_theme_version(meta.version.as_str().into());
        ui.set_theme_creator(meta.creator.as_str().into());
        ui.set_theme_website(meta.website.as_str().into());
        ui.set_theme_source(meta.source.as_str().into());
        ui.set_theme_donation(meta.donation.as_str().into());
        ui.set_theme_installed(!def.bundled);
        ui.set_palette_options(ModelRc::new(VecModel::from(palettes)));
        ui.set_light_palette_index(resolved.light_palette as i32);
        ui.set_dark_palette_index(resolved.dark_palette as i32);
        ui.set_theme_errors(self.catalog.errors.join("\n").into());
    }

    /// The setting key and value for a theme, palette or accent picked by index.
    pub(super) fn theme_value(&self, key: &str, index: usize) -> Option<(String, String)> {
        if key == THEME {
            return Some((THEME.into(), self.catalog.themes.get(index)?.id.clone()));
        }
        let def = &self.catalog.themes[self.catalog.find(&self.choices.theme.theme)];
        let name = match key {
            LIGHT_PALETTE | DARK_PALETTE => def.palettes.get(index)?.name.clone(),
            ACCENT => def.accents.get(index)?.clone(),
            _ => return None,
        };
        Some((per_theme(key, &def.id), name))
    }

    /// Reads the bundled and installed themes again, as after an import.
    pub(super) fn reload_themes(&mut self) {
        self.catalog = theme::Catalog::load(&self.themes_dir);
    }

    /// The font lists: theme default, bundled fonts, then the system's.
    pub(super) fn refresh_fonts(&self) {
        let choice = &self.choices.theme;
        let Some(resolved) = &self.theme else {
            return;
        };
        let look = &self.catalog.themes[resolved.theme].look;
        let index = |name: &str| {
            self.fonts
                .iter()
                .position(|f| f == name)
                .map_or(0, |i| i as i32 + 1)
        };
        let (font, mono) = (index(&choice.font), index(&choice.mono));
        self.ui(|ui, s| {
            let options = |default: &str| {
                let default = if default.is_empty() {
                    s.invoke_system_font()
                } else {
                    default.into()
                };
                let mut list = vec![s.invoke_style_font(default)];
                list.extend(self.fonts.iter().map(SharedString::from));
                ModelRc::new(VecModel::from(list))
            };
            ui.set_font_options(options(&look.font));
            ui.set_mono_options(options(&look.mono));
            ui.set_font_index(if choice.font.is_empty() { 0 } else { font });
            ui.set_mono_index(if choice.mono.is_empty() { 0 } else { mono });
        });
    }

    /// The system's fonts arrive from a thread, since reading them takes a moment.
    pub(super) fn load_system_fonts(&self) {
        std::thread::spawn(|| {
            let fonts = system_fonts();
            let _ = slint::invoke_from_event_loop(move || {
                with_app(move |app| {
                    app.fonts = BUNDLED_FONTS.iter().map(|f| (*f).to_owned()).collect();
                    app.fonts.extend(fonts);
                    app.refresh_fonts();
                });
            });
        });
    }

    /// The font name at an option index; 0 is the theme's font.
    pub(super) fn font_at(&self, index: usize) -> String {
        index
            .checked_sub(1)
            .and_then(|i| self.fonts.get(i))
            .cloned()
            .unwrap_or_default()
    }
}

/// A small drawing of a theme for the picker, in the colors of the shown mode.
fn card(def: &ThemeDef, dark: bool) -> ThemeCard {
    let palette = if dark { def.dark } else { def.light };
    let c: Colors = def.colors(palette, def.accent);
    ThemeCard {
        name: def.meta.name.as_str().into(),
        creator: def.meta.creator.as_str().into(),
        page: c.base,
        side: c.sidebar,
        cover: c.surface1,
        accent: c.accent,
        action: c.action,
        stroke: c.stroke,
        glow: c.glow2,
        radius: (def.look.card_radius / 2.0).min(6.0),
        pill: def.look.pill,
        floating: def.look.floating,
        backdrop: def.look.backdrop,
    }
}

/// Mica for WinUI; a title bar in the window's colors for the other styles.
fn frame(theme: &Resolved, dark: bool) -> Frame {
    let colors = if dark { &theme.dark } else { &theme.light };
    Frame {
        mica: theme.style == theme::Style::Winui,
        dark,
        caption: if theme.style == theme::Style::Audiobookshelf {
            colors.mantle
        } else {
            colors.base
        },
        caption_text: colors.text,
    }
}

fn system_fonts() -> Vec<String> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let mut names: Vec<String> = db
        .faces()
        .filter_map(|face| face.families.first().map(|(name, _)| name.clone()))
        .filter(|name| !name.starts_with('.') && !BUNDLED_FONTS.contains(&name.as_str()))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    names
}

fn theme_note(loaded: &Result<ThemeFile, String>, strings: &Strings<'_>) -> SharedString {
    match loaded {
        Err(message) => strings.invoke_theme_file_error(message.into()),
        Ok(file) if !file.keys().is_empty() => {
            strings.invoke_theme_file_sets(file.keys().join(", ").into())
        }
        Ok(_) => "".into(),
    }
}
