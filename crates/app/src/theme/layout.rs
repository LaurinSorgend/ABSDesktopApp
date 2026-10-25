//! The `layout` and `widgets` blocks of a theme file: where navigation and the player
//! sit, and how single controls are drawn. The theme's style sets the defaults.

use serde_json::Value;

use super::file;
use crate::Look;

pub const NAV: [&str; 3] = ["left", "right", "top"];
pub const PLAYER: [&str; 4] = ["bottom", "top", "left", "right"];
pub const NAV_ITEM: [&str; 3] = ["bar", "pill", "indicator"];
pub const TEXT_FIELD: [&str; 3] = ["outlined", "filled", "underlined"];
pub const SEGMENTED: [&str; 4] = ["joined", "check", "track", "fluent"];
pub const SLIDER: [&str; 5] = ["square", "dot", "hover-dot", "knob", "ring"];
pub const COVER_HOVER: [&str; 4] = ["ring", "tint", "dim", "none"];
pub const PLAY_BUTTON: [&str; 4] = ["auto", "circle", "soft", "rounded"];

/// Applies a `layout` block.
pub fn parse_layout(look: &mut Look, value: &Value) -> Result<(), String> {
    for (key, value) in object("layout", value)? {
        match key.as_str() {
            "nav" => look.nav = choice(key, value, &NAV)?,
            "player" => look.player = choice(key, value, &PLAYER)?,
            "sidebar-width" => look.sidebar_width = file::number(key, value, 140.0, 360.0)?,
            "player-width" => look.player_width = file::number(key, value, 260.0, 480.0)?,
            "logo" => look.logo = flag(key, value)?,
            other => return Err(format!("layout: unknown key \"{other}\"")),
        }
    }
    Ok(())
}

/// Applies a `widgets` block.
pub fn parse_widgets(look: &mut Look, value: &Value) -> Result<(), String> {
    for (key, value) in object("widgets", value)? {
        match key.as_str() {
            "nav-item" => look.nav_item = choice(key, value, &NAV_ITEM)?,
            "nav-inset" => look.nav_inset = file::number(key, value, 0.0, 24.0)?,
            "text-field" => look.text_field = choice(key, value, &TEXT_FIELD)?,
            "segmented" => look.segmented = choice(key, value, &SEGMENTED)?,
            "slider" => look.slider = choice(key, value, &SLIDER)?,
            "cover-hover" => look.cover_hover = choice(key, value, &COVER_HOVER)?,
            "cover-zoom" => look.cover_zoom = flag(key, value)?,
            "shelf" => look.shelf = flag(key, value)?,
            "play-button" => look.play_button = choice(key, value, &PLAY_BUTTON)?,
            "play-shadow" => look.play_shadow = flag(key, value)?,
            "sheen" => look.sheen = flag(key, value)?,
            "control-edge" => look.control_edge = flag(key, value)?,
            "button-weight" => look.button_weight = file::number(key, value, 100.0, 900.0)? as i32,
            other => return Err(format!("widgets: unknown key \"{other}\"")),
        }
    }
    Ok(())
}

pub fn layout_json(look: &Look) -> String {
    object_json(&[
        ("nav", name(&NAV, look.nav)),
        ("player", name(&PLAYER, look.player)),
        ("sidebar-width", look.sidebar_width.to_string()),
        ("player-width", look.player_width.to_string()),
        ("logo", look.logo.to_string()),
    ])
}

pub fn widgets_json(look: &Look) -> String {
    object_json(&[
        ("nav-item", name(&NAV_ITEM, look.nav_item)),
        ("nav-inset", look.nav_inset.to_string()),
        ("text-field", name(&TEXT_FIELD, look.text_field)),
        ("segmented", name(&SEGMENTED, look.segmented)),
        ("slider", name(&SLIDER, look.slider)),
        ("cover-hover", name(&COVER_HOVER, look.cover_hover)),
        ("cover-zoom", look.cover_zoom.to_string()),
        ("shelf", look.shelf.to_string()),
        ("play-button", name(&PLAY_BUTTON, look.play_button)),
        ("play-shadow", look.play_shadow.to_string()),
        ("sheen", look.sheen.to_string()),
        ("control-edge", look.control_edge.to_string()),
        ("button-weight", look.button_weight.to_string()),
    ])
}

/// A JSON object with one key per line, indented to sit inside a theme file.
pub(super) fn object_json(entries: &[(&str, String)]) -> String {
    let lines: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("    \"{k}\": {v}"))
        .collect();
    format!("{{\n{}\n  }}", lines.join(",\n"))
}

fn object<'a>(block: &str, value: &'a Value) -> Result<&'a serde_json::Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("{block}: expected an object"))
}

fn flag(key: &str, value: &Value) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("{key}: expected true or false"))
}

fn choice(key: &str, value: &Value, names: &[&str]) -> Result<i32, String> {
    let given = file::string(key, value)?;
    names
        .iter()
        .position(|n| *n == given)
        .map(|i| i as i32)
        .ok_or_else(|| format!("{key}: use {}", file::list(names.iter().copied())))
}

fn name(names: &[&str], index: i32) -> String {
    let index = (index.max(0) as usize).min(names.len() - 1);
    format!("\"{}\"", names[index])
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::theme::{Catalog, Style, ThemeChoice, ThemeDef, file, resolve};

    #[test]
    fn styles_set_defaults_that_blocks_override() {
        let text = r##"{
            "name": "Top", "style": "material",
            "layout": {"nav": "top", "player": "right", "player-width": 300, "logo": true},
            "widgets": {"slider": "ring", "shelf": true, "text-field": "underlined"},
            "palettes": [{"name": "Day", "mode": "light", "colors": {
                "base": "#ffffff", "mantle": "#f0f0f0", "crust": "#e0e0e0",
                "surface0": "#dddddd", "surface1": "#cccccc", "surface2": "#bbbbbb",
                "overlay0": "#999999", "overlay2": "#777777", "text": "#111111",
                "subtext": "#333333", "accent": "#0055aa", "red": "#aa0000",
                "yellow": "#aa7700", "green": "#00aa00", "shelf": "#8b5a2b"}}]
        }"##;
        let def = ThemeDef::parse("top", text, false).unwrap();
        let look = &def.look;
        assert_eq!((look.nav, look.player, look.player_width), (2, 3, 300.0));
        assert!(look.logo && look.shelf);
        assert_eq!((look.slider, look.text_field), (4, 2));
        assert_eq!(look.segmented, 1, "material's check marks stay");
        assert_eq!(def.palettes[0].colors.shelf.red(), 0x8b);
        assert_eq!(ThemeDef::parse("top", &def.to_json(), false).unwrap(), def);

        let mut winui = crate::Look::default();
        Style::Winui.defaults(&mut winui);
        assert_eq!(
            (winui.nav_item, winui.slider, winui.sidebar_width),
            (2, 4, 240.0)
        );
        assert!(winui.control_edge && !winui.shelf);

        let bad = |block: &str| {
            let mut look = crate::Look::default();
            parse_layout(&mut look, &serde_json::from_str(block).unwrap()).is_err()
        };
        assert!(bad(r#"{"nav": "bottom"}"#));
        assert!(bad(r#"{"sidebar-width": 20}"#));
        assert!(bad(r#"{"dock": "left"}"#));
    }

    #[test]
    fn local_file_moves_the_navigation() {
        let local =
            file::parse(r#"{"layout": {"nav": "right"}, "widgets": {"sheen": true}}"#).unwrap();
        let choice = ThemeChoice {
            theme: "catppuccin".into(),
            ..ThemeChoice::default()
        };
        let theme = resolve(
            &choice,
            &Catalog::load(Path::new("no installed themes")),
            &local,
        );
        assert_eq!(theme.look.nav, 1);
        assert!(theme.look.sheen);
        assert!(file::parse(r#"{"widgets": {"slider": "huge"}}"#).is_err());
    }
}
