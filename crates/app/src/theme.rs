//! Themes: a theme file (widget set, look, palettes and accents), the user's choice of
//! palettes, accent, fonts and shapes, and an optional `theme.json` with local changes.

mod catalog;
mod file;
mod styles;

use std::path::{Path, PathBuf};

use slint::Color;

use crate::{Colors, Look};
pub use catalog::{Catalog, DEFAULT_THEME, INSTALLED_DIR, ThemeDef, install};
pub use file::{ThemeFile, load};
pub use styles::{STYLES, Style};

/// Name of the file with local changes in the data folder.
pub const FILE_NAME: &str = "theme.json";

/// Text size choices, in percent.
pub const FONT_SCALES: [u32; 4] = [90, 100, 110, 125];
pub const DEFAULT_FONT_SCALE: usize = 1;
/// Corner radius choices after "theme default", in pixels.
pub const RADII: [f32; 6] = [0.0, 2.0, 4.0, 8.0, 12.0, 16.0];
/// Border width choices after "theme default", in pixels.
pub const BORDERS: [f32; 3] = [0.0, 1.0, 2.0];
/// Fonts compiled into the app.
pub const BUNDLED_FONTS: [&str; 4] = ["Ronzino", "Source Sans 3", "Roboto", "Fira Code"];

/// The user's theme choices. Empty names fall back to the theme's defaults; index 0 of
/// `radius`, `density`, `border` and `focus` keeps what the theme says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ThemeChoice {
    pub theme: String,
    /// 0 system, 1 light, 2 dark.
    pub mode: usize,
    /// Palette names for light and dark mode.
    pub light: String,
    pub dark: String,
    pub accent: String,
    pub font: String,
    pub mono: String,
    /// Into [`FONT_SCALES`].
    pub font_scale: usize,
    pub radius: usize,
    pub density: usize,
    pub border: usize,
    pub focus: usize,
}

/// Everything the window needs to draw a theme.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Index into the catalog.
    pub theme: usize,
    pub style: Style,
    pub light_palette: usize,
    pub dark_palette: usize,
    pub accent: usize,
    pub light: Colors,
    pub dark: Colors,
    pub look: Look,
}

/// The theme for a choice, with the local theme file applied on top.
pub fn resolve(choice: &ThemeChoice, catalog: &Catalog, file: &ThemeFile) -> Resolved {
    let index = catalog.find(&choice.theme);
    let def = &catalog.themes[index];
    let light_palette = def.palette(&choice.light, false);
    let dark_palette = def.palette(&choice.dark, true);
    let accent = def.accent_index(&choice.accent);
    let mut light = def.colors(light_palette, accent);
    let mut dark = def.colors(dark_palette, accent);
    file::apply_colors(&mut light, &file.light);
    file::apply_colors(&mut dark, &file.dark);
    let mut look = def.look.clone();
    apply_choice(&mut look, choice);
    file.apply_look(&mut look);
    Resolved {
        theme: index,
        style: def.style,
        light_palette,
        dark_palette,
        accent,
        light,
        dark,
        look,
    }
}

fn apply_choice(look: &mut Look, choice: &ThemeChoice) {
    if !choice.font.is_empty() {
        look.font = choice.font.as_str().into();
    }
    if !choice.mono.is_empty() {
        look.mono = choice.mono.as_str().into();
    }
    look.font_scale = FONT_SCALES[choice.font_scale.min(FONT_SCALES.len() - 1)] as f32 / 100.0;
    if let Some(radius) = choice.radius.checked_sub(1).and_then(|i| RADII.get(i)) {
        set_radius(look, *radius);
    }
    if let Some(density) = choice.density.checked_sub(1).filter(|d| *d < 3) {
        look.density = density as i32;
    }
    if let Some(border) = choice.border.checked_sub(1).and_then(|i| BORDERS.get(i)) {
        look.border = *border;
    }
    if let Some(focus) = choice.focus.checked_sub(1).filter(|f| *f < 3) {
        look.focus = focus as i32;
    }
}

/// One radius for everything; fully round buttons only stay with a large radius.
pub(crate) fn set_radius(look: &mut Look, radius: f32) {
    look.radius = radius;
    look.cover_radius = radius;
    look.card_radius = radius;
    look.panel_radius = radius;
    look.pill = look.pill && radius >= 12.0;
    look.round_icons = look.round_icons && radius >= 8.0;
}

/// The accents of the shown palette: names and colors.
pub fn accents(catalog: &Catalog, theme: &Resolved, dark: bool) -> Vec<(String, Color)> {
    let def = &catalog.themes[theme.theme];
    let palette = if dark {
        theme.dark_palette
    } else {
        theme.light_palette
    };
    def.accents
        .iter()
        .enumerate()
        .map(|(i, name)| (name.clone(), def.colors(palette, i).accent))
        .collect()
}

/// The resolved theme as a complete theme file to share: the shown palettes with the
/// local changes, the current accent and the look with the user's choices.
pub fn export(catalog: &Catalog, theme: &Resolved) -> String {
    let base = &catalog.themes[theme.theme];
    let palette = |index: usize, colors: &Colors| catalog::Palette {
        colors: colors.clone(),
        accents: Vec::new(),
        ..base.palettes[index].clone()
    };
    let mut palettes = vec![palette(theme.light_palette, &theme.light)];
    if theme.dark_palette != theme.light_palette {
        palettes.push(palette(theme.dark_palette, &theme.dark));
    }
    let def = ThemeDef {
        meta: catalog::Meta {
            name: format!("{} (custom)", base.meta.name),
            ..base.meta.clone()
        },
        look: theme.look.clone(),
        dark: palettes.len() - 1,
        light: 0,
        palettes,
        accents: Vec::new(),
        accent: 0,
        ..base.clone()
    };
    def.to_json()
}

/// WCAG contrast ratio of two colors.
pub fn contrast(a: Color, b: Color) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn luminance(color: Color) -> f64 {
    let channel = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.red()) + 0.7152 * channel(color.green()) + 0.0722 * channel(color.blue())
}

/// Where the local theme file lives.
pub fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> Catalog {
        Catalog::load(Path::new("no installed themes"))
    }

    fn choice(theme: &str) -> ThemeChoice {
        ThemeChoice {
            theme: theme.into(),
            font_scale: DEFAULT_FONT_SCALE,
            ..ThemeChoice::default()
        }
    }

    #[test]
    fn defaults_and_palettes() {
        let catalog = catalog();
        assert!(catalog.errors.is_empty(), "{:?}", catalog.errors);
        let theme = resolve(&choice("catppuccin"), &catalog, &ThemeFile::default());
        assert_eq!(
            theme.light.base,
            Color::from_rgb_u8(0xef, 0xf1, 0xf5),
            "Latte"
        );
        assert_eq!(
            theme.dark.accent,
            Color::from_rgb_u8(0x89, 0xb4, 0xfa),
            "Mocha blue"
        );
        assert_eq!(theme.dark.on_accent, theme.dark.base);
        let frappe = ThemeChoice {
            dark: "Frappé".into(),
            accent: "Yellow".into(),
            ..choice("catppuccin")
        };
        let theme = resolve(&frappe, &catalog, &ThemeFile::default());
        assert_eq!(theme.dark.base, Color::from_rgb_u8(0x30, 0x34, 0x46));
        assert_eq!(
            theme.light.on_accent, theme.light.text,
            "dark text on Latte yellow"
        );
        let missing = resolve(&choice("gone"), &catalog, &ThemeFile::default());
        assert_eq!(catalog.themes[missing.theme].id, "catppuccin");
    }

    #[test]
    fn every_widget_set_has_a_theme() {
        let catalog = catalog();
        for style in STYLES {
            let def = catalog.themes.iter().find(|t| t.style == style);
            assert!(def.is_some(), "no bundled theme uses {style:?}");
        }
        let abs = resolve(&choice("audiobookshelf"), &catalog, &ThemeFile::default());
        assert_eq!(abs.dark.accent, Color::from_rgb_u8(0xfa, 0xcc, 0x15));
        assert_eq!(abs.look.font.as_str(), "Source Sans 3");
        let glass = &catalog.themes[catalog.find("glass")].look;
        assert!(glass.floating && glass.backdrop && glass.pill);
    }

    #[test]
    fn user_choices_override_the_theme() {
        let custom = ThemeChoice {
            font: "Fira Code".into(),
            font_scale: 3,
            radius: 1,
            density: 1,
            border: 3,
            focus: 3,
            ..choice("material")
        };
        let look = resolve(&custom, &catalog(), &ThemeFile::default()).look;
        assert_eq!(look.font.as_str(), "Fira Code");
        assert!((look.font_scale - 1.25).abs() < f32::EPSILON);
        assert_eq!(look.radius, 0.0);
        assert!(!look.pill && !look.round_icons);
        assert_eq!(look.density, 0);
        assert_eq!(look.border, 2.0);
        assert_eq!(look.focus, 2);
    }

    #[test]
    fn export_is_a_theme_file() {
        let catalog = catalog();
        let theme = resolve(&choice("winui"), &catalog, &ThemeFile::default());
        let text = export(&catalog, &theme);
        let def = ThemeDef::parse("x", &text, false).unwrap();
        assert_eq!(def.meta.name, "WinUI 3 (custom)");
        assert_eq!(def.style, Style::Winui);
        assert_eq!(def.colors(def.light, 0), theme.light);
        assert_eq!(def.colors(def.dark, 0), theme.dark);
    }
}
