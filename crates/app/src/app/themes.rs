//! Applies the theme choice and `theme.json`, and opens the file for editing.

use slint::{ComponentHandle, ModelRc, VecModel};

use super::App;
use crate::theme::{self, ACCENT_NAMES, Overrides, flavor};
use crate::{Strings, Theme, Ui};

impl App {
    /// Recomputes the colors from the settings and the theme file.
    pub fn apply_theme(&mut self) {
        let choice = self.choices.theme;
        let loaded = theme::load_overrides(&self.theme_file);
        let overrides = loaded.clone().unwrap_or_default();
        let (light, dark) = theme::colors(&choice, &overrides);
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let colors = window.global::<Theme>();
        colors.set_light_colors(light);
        colors.set_dark_colors(dark);
        colors.set_mode(choice.mode as i32);
        let shown = if colors.get_dark() {
            choice.dark
        } else {
            choice.light
        };
        let swatches: Vec<slint::Color> = flavor(shown)
            .accents
            .iter()
            .map(|c| slint::Color::from_rgb_u8((c >> 16) as u8, (c >> 8) as u8, *c as u8))
            .collect();
        let names: Vec<slint::SharedString> = ACCENT_NAMES.iter().map(|n| (*n).into()).collect();
        let ui = window.global::<Ui>();
        ui.set_accent_swatches(ModelRc::new(VecModel::from(swatches)));
        ui.set_accent_names(ModelRc::new(VecModel::from(names)));
        ui.set_theme_mode(choice.mode as i32);
        ui.set_light_flavor(choice.light as i32);
        ui.set_dark_flavor(choice.dark as i32);
        ui.set_accent_index(choice.accent as i32);
        ui.set_theme_file(self.theme_file.display().to_string().into());
        ui.set_theme_note(theme_note(&loaded, &window.global::<Strings>()));
    }

    /// Writes a starting theme file if there is none, then opens it in the default editor.
    pub fn edit_theme_file(&mut self) {
        if !self.theme_file.exists() {
            let (light, dark) = theme::colors(&self.choices.theme, &Overrides::default());
            if let Err(e) = std::fs::write(&self.theme_file, theme::template(&light, &dark)) {
                self.show_error(e.to_string());
                return;
            }
        }
        if let Err(e) = open_in_editor(&self.theme_file) {
            self.show_error(e.to_string());
        }
        self.apply_theme();
    }
}

fn theme_note(loaded: &Result<Overrides, String>, strings: &Strings<'_>) -> slint::SharedString {
    match loaded {
        Err(message) => strings.invoke_theme_file_error(message.into()),
        Ok(o) if !o.light.is_empty() || !o.dark.is_empty() => strings.invoke_custom_colors_active(),
        Ok(_) => "".into(),
    }
}

fn open_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    command.spawn().map(drop)
}
