//! Complete themes: the ones bundled at build time from the themes repository, and the
//! ones the user installed into the data folder. The format is documented in that
//! repository's README.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use slint::Color;

use super::file::{self, COLOR_NAMES, FOCUS_RINGS};
use super::layout;
use super::{Style, contrast};
use crate::{Colors, Look};

include!(concat!(env!("OUT_DIR"), "/themes.rs"));

/// The theme used when the chosen one is gone.
pub const DEFAULT_THEME: &str = "catppuccin";
/// Folder in the data folder for installed themes.
pub const INSTALLED_DIR: &str = "themes";

/// Who made a theme and where to find it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Meta {
    pub name: String,
    pub version: String,
    pub creator: String,
    pub description: String,
    pub website: String,
    pub source: String,
    pub donation: String,
}

/// One set of colors, with the colors each accent changes.
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub name: String,
    pub dark: bool,
    pub colors: Colors,
    pub accents: Vec<HashMap<String, Color>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThemeDef {
    pub id: String,
    pub meta: Meta,
    pub style: Style,
    pub look: Look,
    pub palettes: Vec<Palette>,
    pub accents: Vec<String>,
    /// Default palettes and accent.
    pub light: usize,
    pub dark: usize,
    pub accent: usize,
    pub bundled: bool,
}

impl ThemeDef {
    pub fn parse(id: &str, text: &str, bundled: bool) -> Result<Self, String> {
        let raw: Map<String, Value> = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let text_of = |key: &str| -> Result<String, String> {
            raw.get(key)
                .map_or(Ok(""), |v| file::string(key, v))
                .map(str::to_owned)
        };
        let meta = Meta {
            name: text_of("name")?,
            version: text_of("version")?,
            creator: text_of("creator")?,
            description: text_of("description")?,
            website: text_of("website")?,
            source: text_of("source")?,
            donation: text_of("donation")?,
        };
        if meta.name.is_empty() {
            return Err("name: missing".into());
        }
        let style_key = text_of("style")?;
        let style = Style::from_key(&style_key).ok_or_else(|| {
            format!(
                "style: \"{style_key}\" is not one of {}",
                file::list(super::STYLES.map(Style::key))
            )
        })?;
        let mut look = default_look();
        style.defaults(&mut look);
        if let Some(value) = raw.get("look") {
            parse_look(&mut look, value)?;
        }
        if let Some(value) = raw.get("layout") {
            layout::parse_layout(&mut look, value)?;
        }
        if let Some(value) = raw.get("widgets") {
            layout::parse_widgets(&mut look, value)?;
        }
        let palettes = raw
            .get("palettes")
            .and_then(Value::as_array)
            .filter(|p| !p.is_empty())
            .ok_or("palettes: expected a list with at least one palette")?
            .iter()
            .map(parse_palette)
            .collect::<Result<Vec<_>, _>>()?;
        let accents = accent_names(&raw, &palettes)?;
        let find = |key: &str, dark: bool| -> Result<usize, String> {
            let name = text_of(key)?;
            match palettes.iter().position(|p| p.name == name) {
                Some(index) => Ok(index),
                None if name.is_empty() => {
                    Ok(palettes.iter().position(|p| p.dark == dark).unwrap_or(0))
                }
                None => Err(format!("{key}: no palette \"{name}\"")),
            }
        };
        let accent_name = text_of("accent")?;
        let accent = accents.iter().position(|a| *a == accent_name).unwrap_or(0);
        Ok(Self {
            id: id.to_owned(),
            meta,
            style,
            look,
            light: find("light-palette", false)?,
            dark: find("dark-palette", true)?,
            palettes,
            accents,
            accent,
            bundled,
        })
    }

    /// A palette by name, or the default one for the mode.
    pub fn palette(&self, name: &str, dark: bool) -> usize {
        self.palettes
            .iter()
            .position(|p| p.name == name)
            .unwrap_or(if dark { self.dark } else { self.light })
    }

    pub fn accent_index(&self, name: &str) -> usize {
        self.accents
            .iter()
            .position(|a| a == name)
            .unwrap_or(self.accent)
    }

    /// A palette's colors with an accent applied. Colors that followed the palette's own
    /// accent follow the new one, unless the accent sets them itself.
    pub fn colors(&self, palette: usize, accent: usize) -> Colors {
        let palette = &self.palettes[palette.min(self.palettes.len() - 1)];
        let mut colors = palette.colors.clone();
        let Some(changes) = palette.accents.get(accent) else {
            return colors;
        };
        if let Some(new) = changes.get("accent").copied() {
            let old = colors.accent;
            let (base, text) = (colors.base, colors.text);
            let follows_on_accent = colors.on_accent == readable_on(old, base, text);
            let follows_on_action = colors.on_action == readable_on(colors.action, base, text);
            for slot in [&mut colors.action, &mut colors.glow1] {
                if *slot == old {
                    *slot = new;
                }
            }
            colors.accent = new;
            if follows_on_accent {
                colors.on_accent = readable_on(new, base, text);
            }
            if follows_on_action {
                colors.on_action = readable_on(colors.action, base, text);
            }
        }
        file::apply_colors(&mut colors, changes);
        colors
    }

    /// The theme file, in the same layout as the files in the themes repository.
    pub fn to_json(&self) -> String {
        let q = |s: &str| Value::String(s.to_owned()).to_string();
        let meta = &self.meta;
        let mut out = String::from("{\n");
        for (key, value) in [
            ("name", &meta.name),
            ("version", &meta.version),
            ("creator", &meta.creator),
            ("description", &meta.description),
            ("website", &meta.website),
            ("source", &meta.source),
            ("donation", &meta.donation),
        ] {
            out += &format!("  \"{key}\": {},\n", q(value));
        }
        out += &format!("  \"style\": {},\n", q(self.style.key()));
        out += &format!(
            "  \"light-palette\": {},\n",
            q(&self.palettes[self.light].name)
        );
        out += &format!(
            "  \"dark-palette\": {},\n",
            q(&self.palettes[self.dark].name)
        );
        if let Some(accent) = self.accents.get(self.accent) {
            out += &format!("  \"accent\": {},\n", q(accent));
        }
        out += &format!("  \"look\": {},\n", look_json(&self.look));
        out += &format!("  \"layout\": {},\n", layout::layout_json(&self.look));
        out += &format!("  \"widgets\": {},\n", layout::widgets_json(&self.look));
        let palettes: Vec<String> = self
            .palettes
            .iter()
            .map(|p| palette_json(p, &self.accents))
            .collect();
        out += &format!("  \"palettes\": [\n{}\n  ]\n}}\n", palettes.join(",\n"));
        out
    }
}

/// Bundled themes, then the installed ones; an installed theme replaces a bundled one
/// with the same id. Files that do not parse are skipped and reported.
#[derive(Debug, Default)]
pub struct Catalog {
    pub themes: Vec<ThemeDef>,
    pub errors: Vec<String>,
}

impl Catalog {
    pub fn load(installed: &Path) -> Self {
        let mut catalog = Self::default();
        for (id, text) in BUNDLED {
            match ThemeDef::parse(id, text, true) {
                Ok(theme) => catalog.themes.push(theme),
                Err(e) => catalog.errors.push(format!("{id}: {e}")),
            }
        }
        let mut files: Vec<PathBuf> = std::fs::read_dir(installed)
            .map(|dir| dir.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        files.sort();
        for path in files
            .iter()
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
        {
            let id = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let parsed = std::fs::read_to_string(path)
                .map_err(|e| e.to_string())
                .and_then(|text| ThemeDef::parse(&id, &text, false));
            match parsed {
                Ok(theme) => match catalog.themes.iter().position(|t| t.id == id) {
                    Some(index) => catalog.themes[index] = theme,
                    None => catalog.themes.push(theme),
                },
                Err(e) => catalog.errors.push(format!("{}: {e}", path.display())),
            }
        }
        catalog
    }

    /// The theme with this id, else the default theme, else the first.
    pub fn find(&self, id: &str) -> usize {
        let at = |id: &str| self.themes.iter().position(|t| t.id == id);
        at(id).or_else(|| at(DEFAULT_THEME)).unwrap_or(0)
    }
}

/// Checks a theme file and copies it into the installed folder. Returns its id.
pub fn install(from: &Path, installed: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(from).map_err(|e| e.to_string())?;
    let name = ThemeDef::parse("", &text, false)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .meta
        .name;
    let id = slug(&name);
    std::fs::create_dir_all(installed).map_err(|e| e.to_string())?;
    std::fs::write(installed.join(format!("{id}.json")), text).map_err(|e| e.to_string())?;
    Ok(id)
}

/// An id from a theme name: lowercase letters and digits joined by dashes.
pub fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    if words.is_empty() {
        "theme".into()
    } else {
        words.join("-")
    }
}

/// The look a theme starts from before its `look` block.
fn default_look() -> Look {
    Look {
        font: "Ronzino".into(),
        mono: "Fira Code".into(),
        font_scale: 1.0,
        heading_weight: 700,
        border: 1.0,
        density: 1,
        control_height: 36.0,
        panel_shadow: 18.0,
        ..Look::default()
    }
}

fn parse_look(look: &mut Look, value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("look: expected an object")?;
    for (key, value) in object {
        let length = |max: f32| file::number(key, value, 0.0, max);
        let flag = || {
            value
                .as_bool()
                .ok_or_else(|| format!("{key}: expected true or false"))
        };
        match key.as_str() {
            "font" => look.font = file::string(key, value)?.into(),
            "mono-font" => look.mono = file::string(key, value)?.into(),
            "heading-weight" => {
                look.heading_weight = file::number(key, value, 100.0, 900.0)? as i32
            }
            "radius" => look.radius = length(32.0)?,
            "cover-radius" => look.cover_radius = length(32.0)?,
            "card-radius" => look.card_radius = length(32.0)?,
            "panel-radius" => look.panel_radius = length(32.0)?,
            "pill" => look.pill = flag()?,
            "round-icons" => look.round_icons = flag()?,
            "border-width" => look.border = length(4.0)?,
            "focus-ring" => look.focus = file::named(key, value, &FOCUS_RINGS)? as i32,
            "control-height" => look.control_height = file::number(key, value, 24.0, 56.0)?,
            "cover-shadow" => look.cover_shadow = length(48.0)?,
            "panel-shadow" => look.panel_shadow = length(64.0)?,
            "floating" => look.floating = flag()?,
            "content-card" => look.content_card = flag()?,
            "backdrop" => look.backdrop = flag()?,
            other => return Err(format!("look: unknown key \"{other}\"")),
        }
    }
    Ok(())
}

/// The colors every palette has to set; the others follow from them.
const CORE: [&str; 14] = [
    "base", "mantle", "crust", "surface0", "surface1", "surface2", "overlay0", "overlay2", "text",
    "subtext", "accent", "red", "yellow", "green",
];

fn parse_palette(value: &Value) -> Result<Palette, String> {
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .ok_or("palettes: every palette needs a name")?
        .to_owned();
    let at = |e: String| format!("palette \"{name}\": {e}");
    let dark = match value.get("mode").and_then(Value::as_str) {
        Some("light") => false,
        Some("dark") => true,
        _ => return Err(at("mode: use \"light\" or \"dark\"".into())),
    };
    let given = file::colors(value.get("colors").unwrap_or(&Value::Null)).map_err(at)?;
    let core = |key: &str| {
        given
            .get(key)
            .copied()
            .ok_or_else(|| at(format!("colors: \"{key}\" is missing")))
    };
    for key in CORE {
        core(key)?;
    }
    let mut colors = derived(&|key| core(key).unwrap_or_default(), dark);
    file::apply_colors(&mut colors, &given);
    let accents = match value.get("accents") {
        None => Vec::new(),
        Some(list) => list
            .as_array()
            .ok_or_else(|| at("accents: expected a list".into()))?
            .iter()
            .map(|a| file::colors(a.get("colors").unwrap_or(&Value::Null)).map_err(at))
            .collect::<Result<_, _>>()?,
    };
    Ok(Palette {
        name,
        dark,
        colors,
        accents,
    })
}

/// The accent names, which every palette has to list in the same order.
fn accent_names(raw: &Map<String, Value>, palettes: &[Palette]) -> Result<Vec<String>, String> {
    let names_of = |index: usize| -> Vec<String> {
        raw["palettes"][index]
            .get("accents")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|a| {
                        a.get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let first = names_of(0);
    for (index, palette) in palettes.iter().enumerate().skip(1) {
        if names_of(index) != first {
            return Err(format!(
                "palette \"{}\": accents differ from the first palette's",
                palette.name
            ));
        }
    }
    Ok(first)
}

/// All colors from the core ones, as a palette without the optional colors gets them.
fn derived(core: &dyn Fn(&str) -> Color, dark: bool) -> Colors {
    let (base, text, accent) = (core("base"), core("text"), core("accent"));
    let on_accent = readable_on(accent, base, text);
    Colors {
        base,
        mantle: core("mantle"),
        crust: core("crust"),
        surface0: core("surface0"),
        surface1: core("surface1"),
        surface2: core("surface2"),
        overlay0: core("overlay0"),
        overlay2: core("overlay2"),
        text,
        subtext: core("subtext"),
        accent,
        on_accent,
        red: core("red"),
        yellow: core("yellow"),
        green: core("green"),
        action: accent,
        on_action: on_accent,
        accent_soft: core("surface0"),
        on_accent_soft: text,
        control: base,
        control_hover: core("surface0"),
        control_press: core("surface1"),
        stroke: core("overlay0"),
        layer: base,
        card: base,
        popup: core("mantle"),
        sidebar: core("mantle"),
        bar: core("mantle"),
        shadow: Color::from_argb_u8(0x40, 0, 0, 0),
        glow1: accent,
        glow2: core("overlay2"),
        glow3: core("surface2"),
        shelf: if dark {
            Color::from_rgb_u8(0x4a, 0x4a, 0x4a)
        } else {
            Color::from_rgb_u8(0xcf, 0xcf, 0xcf)
        },
    }
}

/// Base or text, whichever reads better on `fill`.
fn readable_on(fill: Color, base: Color, text: Color) -> Color {
    if contrast(fill, base) >= contrast(fill, text) {
        base
    } else {
        text
    }
}

fn look_json(look: &Look) -> String {
    let number = |n: f32| n.to_string();
    let entries = [
        ("font", Value::String(look.font.to_string()).to_string()),
        (
            "mono-font",
            Value::String(look.mono.to_string()).to_string(),
        ),
        ("heading-weight", look.heading_weight.to_string()),
        ("radius", number(look.radius)),
        ("cover-radius", number(look.cover_radius)),
        ("card-radius", number(look.card_radius)),
        ("panel-radius", number(look.panel_radius)),
        ("pill", look.pill.to_string()),
        ("round-icons", look.round_icons.to_string()),
        ("border-width", number(look.border)),
        (
            "focus-ring",
            format!("\"{}\"", FOCUS_RINGS[look.focus.clamp(0, 2) as usize].0),
        ),
        ("control-height", number(look.control_height)),
        ("cover-shadow", number(look.cover_shadow)),
        ("panel-shadow", number(look.panel_shadow)),
        ("floating", look.floating.to_string()),
        ("content-card", look.content_card.to_string()),
        ("backdrop", look.backdrop.to_string()),
    ];
    layout::object_json(&entries)
}

fn palette_json(palette: &Palette, names: &[String]) -> String {
    let mut colors = palette.colors.clone();
    let lines: Vec<String> = COLOR_NAMES
        .iter()
        .map(|name| {
            format!(
                "        \"{name}\": \"{}\"",
                file::hex(file::get(&mut colors, name))
            )
        })
        .collect();
    let accents: Vec<String> = palette
        .accents
        .iter()
        .zip(names)
        .map(|(changes, name)| {
            let fields: Vec<String> = COLOR_NAMES
                .iter()
                .filter_map(|n| {
                    changes
                        .get(*n)
                        .or_else(|| changes.get(&n.replace('-', "_")))
                        .map(|c| format!("\"{n}\": \"{}\"", file::hex(*c)))
                })
                .collect();
            format!(
                "        {{\"name\": {}, \"colors\": {{{}}}}}",
                Value::String(name.clone()),
                fields.join(", ")
            )
        })
        .collect();
    format!(
        "    {{\n      \"name\": {},\n      \"mode\": \"{}\",\n      \"colors\": {{\n{}\n      }},\n      \"accents\": [\n{}\n      ]\n    }}",
        Value::String(palette.name.clone()),
        if palette.dark { "dark" } else { "light" },
        lines.join(",\n"),
        accents.join(",\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_themes_parse_and_round_trip() {
        assert!(!BUNDLED.is_empty(), "no themes were bundled");
        for (id, text) in BUNDLED {
            let theme = ThemeDef::parse(id, text, true).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert!(!theme.meta.version.is_empty(), "{id}: no version");
            let again = ThemeDef::parse(id, &theme.to_json(), true).unwrap();
            assert_eq!(again, theme, "{id} changes when written and read again");
            for palette in 0..theme.palettes.len() {
                for accent in 0..theme.accents.len().max(1) {
                    let c = theme.colors(palette, accent);
                    let ratio = contrast(c.text, c.base);
                    assert!(ratio >= 7.0, "{id}: text on base is {ratio:.1}:1");
                }
            }
        }
    }

    #[test]
    fn minimal_theme_fills_in_colors() {
        let text = r##"{
            "name": "Plain", "version": "1", "style": "material",
            "palettes": [{"name": "Day", "mode": "light", "colors": {
                "base": "#ffffff", "mantle": "#f0f0f0", "crust": "#e0e0e0",
                "surface0": "#dddddd", "surface1": "#cccccc", "surface2": "#bbbbbb",
                "overlay0": "#999999", "overlay2": "#777777", "text": "#111111",
                "subtext": "#333333", "accent": "#0055aa", "red": "#aa0000",
                "yellow": "#aa7700", "green": "#00aa00"},
                "accents": [{"name": "Blue", "colors": {}}, {"name": "Red", "colors": {"accent": "#aa0000"}}]}]
        }"##;
        let theme = ThemeDef::parse("plain", text, false).unwrap();
        assert_eq!(theme.style, Style::Material);
        assert_eq!((theme.light, theme.dark), (0, 0));
        let blue = theme.colors(0, 0);
        assert_eq!(blue.action, blue.accent);
        assert_eq!(blue.on_accent, blue.base, "white text on dark blue");
        let red = theme.colors(0, 1);
        assert_eq!(red.accent, Color::from_rgb_u8(0xaa, 0, 0));
        assert_eq!(
            red.action, red.accent,
            "the action color follows the accent"
        );
        assert!(
            ThemeDef::parse(
                "x",
                r#"{"name": "X", "style": "aqua", "palettes": []}"#,
                false
            )
            .is_err()
        );
        assert_eq!(slug("WinUI 3 (custom)"), "winui-3-custom");
    }
}
