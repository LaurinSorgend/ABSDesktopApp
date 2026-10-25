//! `theme.json`: local changes on top of the chosen theme, and helpers for reading
//! colors and values that theme files share.
//!
//! ```json
//! {
//!   "font": "Roboto", "mono-font": "Fira Code", "font-scale": 1.1,
//!   "radius": 8, "density": "compact", "border-width": 1, "focus-ring": "glow",
//!   "layout": {"nav": "top"}, "widgets": {"slider": "dot"},
//!   "light": {"accent": "#d20f39"},
//!   "dark": {"accent": "#f38ba8"}
//! }
//! ```
//! Every key is optional.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};
use slint::Color;

use super::{layout, set_radius};
use crate::{Colors, Look};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Density {
    Compact,
    Normal,
    Large,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusRing {
    Outline,
    Glow,
    Double,
}

const DENSITIES: [(&str, Density); 3] = [
    ("compact", Density::Compact),
    ("normal", Density::Normal),
    ("large", Density::Large),
];
pub(super) const FOCUS_RINGS: [(&str, FocusRing); 3] = [
    ("outline", FocusRing::Outline),
    ("glow", FocusRing::Glow),
    ("double", FocusRing::Double),
];

/// What a theme file sets; `None` and missing colors keep the settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ThemeFile {
    pub font: Option<String>,
    pub mono: Option<String>,
    pub font_scale: Option<f32>,
    pub radius: Option<f32>,
    pub density: Option<Density>,
    pub border: Option<f32>,
    pub focus: Option<FocusRing>,
    /// `layout` and `widgets` blocks as in theme files, checked when read.
    pub layout: Option<Value>,
    pub widgets: Option<Value>,
    pub light: HashMap<String, Color>,
    pub dark: HashMap<String, Color>,
}

impl ThemeFile {
    /// The keys this file sets, for the settings page to name.
    pub fn keys(&self) -> Vec<&'static str> {
        let mut keys = Vec::new();
        let mut add = |set: bool, key| {
            if set {
                keys.push(key);
            }
        };
        add(self.font.is_some(), "font");
        add(self.mono.is_some(), "mono-font");
        add(self.font_scale.is_some(), "font-scale");
        add(self.radius.is_some(), "radius");
        add(self.density.is_some(), "density");
        add(self.border.is_some(), "border-width");
        add(self.focus.is_some(), "focus-ring");
        add(self.layout.is_some(), "layout");
        add(self.widgets.is_some(), "widgets");
        add(!self.light.is_empty(), "light");
        add(!self.dark.is_empty(), "dark");
        keys
    }

    pub(super) fn apply_look(&self, look: &mut Look) {
        if let Some(font) = &self.font {
            look.font = font.as_str().into();
        }
        if let Some(mono) = &self.mono {
            look.mono = mono.as_str().into();
        }
        if let Some(scale) = self.font_scale {
            look.font_scale = scale;
        }
        if let Some(radius) = self.radius {
            set_radius(look, radius);
        }
        if let Some(density) = self.density {
            look.density = density as i32;
        }
        if let Some(border) = self.border {
            look.border = border;
        }
        if let Some(focus) = self.focus {
            look.focus = focus as i32;
        }
        if let Some(value) = &self.layout {
            let _ = layout::parse_layout(look, value);
        }
        if let Some(value) = &self.widgets {
            let _ = layout::parse_widgets(look, value);
        }
    }
}

/// The names `light` and `dark` accept.
pub const COLOR_NAMES: [&str; 33] = [
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
    "action",
    "on-action",
    "accent-soft",
    "on-accent-soft",
    "control",
    "control-hover",
    "control-press",
    "stroke",
    "layer",
    "card",
    "popup",
    "sidebar",
    "bar",
    "shadow",
    "glow1",
    "glow2",
    "glow3",
    "shelf",
];

/// The color named in a theme file; `on-accent` and `on_accent` both work.
pub(super) fn slot<'a>(colors: &'a mut Colors, name: &str) -> Option<&'a mut Color> {
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
        "action" => &mut colors.action,
        "on_action" => &mut colors.on_action,
        "accent_soft" => &mut colors.accent_soft,
        "on_accent_soft" => &mut colors.on_accent_soft,
        "control" => &mut colors.control,
        "control_hover" => &mut colors.control_hover,
        "control_press" => &mut colors.control_press,
        "stroke" => &mut colors.stroke,
        "layer" => &mut colors.layer,
        "card" => &mut colors.card,
        "popup" => &mut colors.popup,
        "sidebar" => &mut colors.sidebar,
        "bar" => &mut colors.bar,
        "shadow" => &mut colors.shadow,
        "glow1" => &mut colors.glow1,
        "glow2" => &mut colors.glow2,
        "glow3" => &mut colors.glow3,
        "shelf" => &mut colors.shelf,
        _ => return None,
    })
}

/// The color named `name`.
pub(super) fn get(colors: &mut Colors, name: &str) -> Color {
    slot(colors, name).map_or(Color::default(), |c| *c)
}

pub(super) fn apply_colors(colors: &mut Colors, overrides: &HashMap<String, Color>) {
    for (name, color) in overrides {
        if let Some(slot) = slot(colors, name) {
            *slot = *color;
        }
    }
}

/// Reads a theme file. A missing file sets nothing.
pub fn load(path: &Path) -> Result<ThemeFile, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ThemeFile::default()),
        Err(e) => return Err(e.to_string()),
    };
    parse(&text)
}

pub fn parse(text: &str) -> Result<ThemeFile, String> {
    let raw: Map<String, Value> = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut file = ThemeFile::default();
    for (key, value) in &raw {
        match key.as_str() {
            "font" => file.font = Some(string(key, value)?.to_owned()),
            "mono-font" => file.mono = Some(string(key, value)?.to_owned()),
            "font-scale" => file.font_scale = Some(number(key, value, 0.75, 1.5)?),
            "radius" => file.radius = Some(number(key, value, 0.0, 32.0)?),
            "border-width" => file.border = Some(number(key, value, 0.0, 4.0)?),
            "density" => file.density = Some(named(key, value, &DENSITIES)?),
            "focus-ring" => file.focus = Some(named(key, value, &FOCUS_RINGS)?),
            "layout" => {
                layout::parse_layout(&mut Look::default(), value)?;
                file.layout = Some(value.clone());
            }
            "widgets" => {
                layout::parse_widgets(&mut Look::default(), value)?;
                file.widgets = Some(value.clone());
            }
            "light" => file.light = colors(value)?,
            "dark" => file.dark = colors(value)?,
            other => return Err(format!("unknown key \"{other}\"")),
        }
    }
    Ok(file)
}

pub(super) fn string<'a>(key: &str, value: &'a Value) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{key}: expected a string"))
}

pub(super) fn number(key: &str, value: &Value, min: f32, max: f32) -> Result<f32, String> {
    value
        .as_f64()
        .map(|n| n as f32)
        .filter(|n| (min..=max).contains(n))
        .ok_or_else(|| format!("{key}: expected a number from {min} to {max}"))
}

pub(super) fn named<T: Copy>(key: &str, value: &Value, names: &[(&str, T)]) -> Result<T, String> {
    let name = string(key, value)?;
    names
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, v)| *v)
        .ok_or_else(|| format!("{key}: use {}", list(names.iter().map(|(n, _)| *n))))
}

pub(super) fn list<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    names
        .into_iter()
        .map(|n| format!("\"{n}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn colors(value: &Value) -> Result<HashMap<String, Color>, String> {
    let object = value
        .as_object()
        .ok_or("light and dark: expected an object of colors")?;
    let mut colors = HashMap::new();
    for (name, value) in object {
        if !COLOR_NAMES.contains(&name.replace('_', "-").as_str()) {
            return Err(format!("unknown color \"{name}\""));
        }
        let text = string(name, value)?;
        let color = parse_hex(text)
            .ok_or_else(|| format!("{name}: \"{text}\" is not #rrggbb or #rrggbbaa"))?;
        colors.insert(name.clone(), color);
    }
    Ok(colors)
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

pub(super) fn hex(c: Color) -> String {
    if c.alpha() == 255 {
        format!("#{:02x}{:02x}{:02x}", c.red(), c.green(), c.blue())
    } else {
        format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            c.red(),
            c.green(),
            c.blue(),
            c.alpha()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Catalog, ThemeChoice, resolve};

    fn choice() -> ThemeChoice {
        ThemeChoice {
            theme: "catppuccin".into(),
            mode: 2,
            ..ThemeChoice::default()
        }
    }

    #[test]
    fn colors_parse_and_apply() {
        let file = parse(r##"{"dark": {"accent": "#ff0000", "on_accent": "#00000080"}}"##).unwrap();
        let catalog = Catalog::load(Path::new("no installed themes"));
        let theme = resolve(&choice(), &catalog, &file);
        assert_eq!(theme.dark.accent, Color::from_rgb_u8(255, 0, 0));
        assert_eq!(theme.dark.on_accent.alpha(), 0x80);
        assert_eq!(file.keys(), ["dark"]);
        assert!(parse(r#"{"dark": {"accent": "red"}}"#).is_err());
        assert!(parse(r#"{"dim": {}}"#).is_err());
    }

    #[test]
    fn shapes_parse() {
        let file = parse(
            r#"{"radius": 8, "density": "large", "focus-ring": "glow",
                "font-scale": 1.1, "border-width": 2, "font": "Roboto"}"#,
        )
        .unwrap();
        let catalog = Catalog::load(Path::new("no installed themes"));
        let theme = resolve(&choice(), &catalog, &file);
        assert_eq!(theme.look.radius, 8.0);
        assert_eq!(theme.look.density, 2);
        assert_eq!(theme.look.focus, 1);
        assert_eq!(theme.look.border, 2.0);
        assert_eq!(theme.look.font.as_str(), "Roboto");
        assert!(
            parse(r#"{"style": "winui"}"#).is_err(),
            "the theme is chosen in settings"
        );
        assert!(parse(r#"{"radius": 100}"#).is_err());
        assert!(parse(r#"{"density": "huge"}"#).is_err());
    }
}
