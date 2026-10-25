//! Widget sets: how controls, navigation and panels are drawn. A theme names one of
//! them; colors, fonts and shapes come from the theme file.

use crate::{Look, ThemeStyle};

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

    /// The layout and widgets a theme of this style gets when its file does not say.
    pub fn defaults(self, look: &mut Look) {
        use Style::*;
        look.style = self.widgets();
        look.sidebar_width = if self == Winui { 240.0 } else { 184.0 };
        look.player_width = 320.0;
        look.logo = matches!(self, Audiobookshelf | Winui);
        look.nav_item = match self {
            Material | Glass => 1,
            Winui => 2,
            _ => 0,
        };
        look.nav_inset = match self {
            Material => 12.0,
            Glass => 8.0,
            Winui => 4.0,
            _ => 0.0,
        };
        look.text_field = match self {
            Material => 1,
            Winui => 2,
            _ => 0,
        };
        look.segmented = match self {
            Material => 1,
            Glass => 2,
            Winui => 3,
            _ => 0,
        };
        look.slider = match self {
            Catppuccin => 0,
            Material => 1,
            Audiobookshelf => 2,
            Glass => 3,
            Winui => 4,
        };
        look.cover_hover = match self {
            Material => 1,
            Audiobookshelf => 2,
            _ => 0,
        };
        look.cover_zoom = self != Audiobookshelf;
        look.shelf = self == Audiobookshelf;
        look.play_button = if self == Material { 2 } else { 0 };
        look.play_shadow = matches!(self, Glass | Audiobookshelf);
        look.sheen = self == Glass;
        look.control_edge = self == Winui;
        look.button_weight = if self == Winui { 400 } else { 500 };
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
