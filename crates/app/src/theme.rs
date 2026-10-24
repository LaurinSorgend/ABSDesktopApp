//! Themes: a Catppuccin flavor for light and for dark, an accent, and an optional
//! `theme.json` that overrides single colors.

mod palettes;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use slint::Color;

use crate::Colors;
pub use palettes::{ACCENT_NAMES, FLAVOR_NAMES, Flavor, flavor};

/// Name of the file with color overrides in the data folder.
pub const FILE_NAME: &str = "theme.json";
/// Default accent: blue.
pub const DEFAULT_ACCENT: usize = 12;
/// Default flavors: Latte for light, Mocha for dark.
pub const DEFAULT_LIGHT: usize = 0;
pub const DEFAULT_DARK: usize = 3;

/// The user's theme choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeChoice {
    /// 0 system, 1 light, 2 dark.
    pub mode: usize,
    pub light: usize,
    pub dark: usize,
    pub accent: usize,
}

/// Colors per theme slot from `theme.json`: `{"light": {"accent": "#d20f39"}, "dark": {...}}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Overrides {
    pub light: HashMap<String, Color>,
    pub dark: HashMap<String, Color>,
}

/// The light and dark colors for a choice, with overrides applied.
pub fn colors(choice: &ThemeChoice, overrides: &Overrides) -> (Colors, Colors) {
    let mut light = from_flavor(flavor(choice.light), choice.accent);
    let mut dark = from_flavor(flavor(choice.dark), choice.accent);
    apply(&mut light, &overrides.light);
    apply(&mut dark, &overrides.dark);
    (light, dark)
}

fn from_flavor(f: &Flavor, accent: usize) -> Colors {
    let accent = f.accents[accent.min(f.accents.len() - 1)];
    Colors {
        base: rgb(f.base),
        mantle: rgb(f.mantle),
        crust: rgb(f.crust),
        surface0: rgb(f.surface0),
        surface1: rgb(f.surface1),
        surface2: rgb(f.surface2),
        overlay0: rgb(f.overlay0),
        overlay2: rgb(f.overlay2),
        text: rgb(f.text),
        subtext: rgb(f.subtext1),
        accent: rgb(accent),
        on_accent: rgb(on_accent(accent, f.base, f.text)),
        red: rgb(f.accents[4]),
        yellow: rgb(f.accents[7]),
        green: rgb(f.accents[8]),
    }
}

/// Base or text, whichever reads better on the accent.
fn on_accent(accent: u32, base: u32, text: u32) -> u32 {
    if contrast(accent, base) >= contrast(accent, text) {
        base
    } else {
        text
    }
}

/// WCAG contrast ratio of two colors.
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn luminance(color: u32) -> f64 {
    let channel = |shift: u32| {
        let c = f64::from((color >> shift) & 0xff) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn rgb(color: u32) -> Color {
    Color::from_rgb_u8((color >> 16) as u8, (color >> 8) as u8, color as u8)
}

fn apply(colors: &mut Colors, overrides: &HashMap<String, Color>) {
    for (name, color) in overrides {
        if let Some(slot) = slot(colors, name) {
            *slot = *color;
        }
    }
}

/// The color named in `theme.json`; `on-accent` and `on_accent` both work.
fn slot<'a>(colors: &'a mut Colors, name: &str) -> Option<&'a mut Color> {
    Some(match name.replace('-', "_").as_str() {
        "base" => &mut colors.base,
        "mantle" => &mut colors.mantle,
        "crust" => &mut colors.crust,
        "surface0" => &mut colors.surface0,
        "surface1" => &mut colors.surface1,
        "surface2" => &mut colors.surface2,
        "overlay0" => &mut colors.overlay0,
        "overlay2" => &mut colors.overlay2,
        "text" => &mut colors.text,
        "subtext" => &mut colors.subtext,
        "accent" => &mut colors.accent,
        "on_accent" => &mut colors.on_accent,
        "red" => &mut colors.red,
        "yellow" => &mut colors.yellow,
        "green" => &mut colors.green,
        _ => return None,
    })
}

/// The names `theme.json` accepts.
pub const COLOR_NAMES: [&str; 15] = [
    "base",
    "mantle",
    "crust",
    "surface0",
    "surface1",
    "surface2",
    "overlay0",
    "overlay2",
    "text",
    "subtext",
    "accent",
    "on-accent",
    "red",
    "yellow",
    "green",
];

/// Where the theme file lives.
pub fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join(FILE_NAME)
}

/// Reads `theme.json`. A missing file means no overrides.
pub fn load_overrides(path: &Path) -> Result<Overrides, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Overrides::default()),
        Err(e) => return Err(e.to_string()),
    };
    parse_overrides(&text)
}

pub fn parse_overrides(text: &str) -> Result<Overrides, String> {
    let raw: HashMap<String, HashMap<String, String>> =
        serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut overrides = Overrides::default();
    for (slot, colors) in raw {
        let target = match slot.as_str() {
            "light" => &mut overrides.light,
            "dark" => &mut overrides.dark,
            other => {
                return Err(format!(
                    "unknown theme \"{other}\", use \"light\" or \"dark\""
                ));
            }
        };
        for (name, value) in colors {
            if !COLOR_NAMES.contains(&name.replace('_', "-").as_str()) {
                return Err(format!("unknown color \"{name}\""));
            }
            let color =
                parse_hex(&value).ok_or_else(|| format!("{name}: \"{value}\" is not #rrggbb"))?;
            target.insert(name, color);
        }
    }
    Ok(overrides)
}

/// `#rrggbb` or `#rrggbbaa`.
fn parse_hex(value: &str) -> Option<Color> {
    let hex = value.strip_prefix('#')?;
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    match hex.len() {
        6 => Some(Color::from_rgb_u8(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color::from_argb_u8(byte(6)?, byte(0)?, byte(2)?, byte(4)?)),
        _ => None,
    }
}

/// A starting `theme.json` with the current colors, for the user to edit.
pub fn template(light: &Colors, dark: &Colors) -> String {
    let block = |colors: &Colors| {
        let mut copy = colors.clone();
        COLOR_NAMES
            .iter()
            .map(|name| {
                let c = slot(&mut copy, name).map_or(Color::default(), |c| *c);
                format!(
                    "    \"{name}\": \"#{:02x}{:02x}{:02x}\"",
                    c.red(),
                    c.green(),
                    c.blue()
                )
            })
            .collect::<Vec<_>>()
            .join(",\n")
    };
    format!(
        "{{\n  \"light\": {{\n{}\n  }},\n  \"dark\": {{\n{}\n  }}\n}}\n",
        block(light),
        block(dark)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_colors_and_readable_accent_text() {
        let choice = ThemeChoice {
            mode: 0,
            light: DEFAULT_LIGHT,
            dark: DEFAULT_DARK,
            accent: DEFAULT_ACCENT,
        };
        let (light, dark) = colors(&choice, &Overrides::default());
        assert_eq!(light.base, Color::from_rgb_u8(0xef, 0xf1, 0xf5));
        assert_eq!(dark.accent, Color::from_rgb_u8(0x89, 0xb4, 0xfa));
        assert_eq!(dark.on_accent, dark.base);
        let yellow = ThemeChoice {
            accent: 7,
            ..choice
        };
        let (light, _) = colors(&yellow, &Overrides::default());
        assert_eq!(light.on_accent, light.text, "dark text on Latte yellow");
    }

    #[test]
    fn overrides_parse_apply_and_round_trip() {
        let overrides =
            parse_overrides(r##"{"dark": {"accent": "#ff0000", "on_accent": "#00000080"}}"##)
                .unwrap();
        let choice = ThemeChoice {
            mode: 2,
            light: 0,
            dark: 3,
            accent: 12,
        };
        let (light, dark) = colors(&choice, &overrides);
        assert_eq!(dark.accent, Color::from_rgb_u8(255, 0, 0));
        assert_eq!(dark.on_accent.alpha(), 0x80);
        let again = parse_overrides(&template(&light, &dark)).unwrap();
        assert_eq!(again.dark["accent"], Color::from_rgb_u8(255, 0, 0));
        assert!(parse_overrides(r#"{"dark": {"accent": "red"}}"#).is_err());
        assert!(parse_overrides(r#"{"dim": {}}"#).is_err());
    }
}
