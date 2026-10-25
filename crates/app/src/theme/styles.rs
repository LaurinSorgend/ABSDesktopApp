//! Widget sets: how controls, navigation and panels are drawn. A theme names one of
//! them; colors, fonts and shapes come from the theme file.

use crate::ThemeStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    #[default]
    Catppuccin,
    Audiobookshelf,
    Material,
    Winui,
    Glass,
}

pub const STYLES: [Style; 5] = [
    Style::Catppuccin,
    Style::Audiobookshelf,
    Style::Material,
    Style::Winui,
    Style::Glass,
];

impl Style {
    /// The name in theme files.
    pub fn key(self) -> &'static str {
        match self {
            Style::Catppuccin => "catppuccin",
            Style::Audiobookshelf => "audiobookshelf",
            Style::Material => "material",
            Style::Winui => "winui",
            Style::Glass => "glass",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        STYLES.iter().copied().find(|s| s.key() == key)
    }

    pub fn widgets(self) -> ThemeStyle {
        match self {
            Style::Catppuccin => ThemeStyle::Catppuccin,
            Style::Audiobookshelf => ThemeStyle::Audiobookshelf,
            Style::Material => ThemeStyle::Material,
            Style::Winui => ThemeStyle::Winui,
            Style::Glass => ThemeStyle::Glass,
        }
    }
}
