//! Theme files: installing and removing themes, exporting the current one, editing
//! `theme.json`, and opening a theme's links.

use std::path::{Path, PathBuf};

use super::App;
use crate::settings::THEME;
use crate::theme;

/// What a new `theme.json` starts with.
const STARTER: &str = "{\n  \"light\": {},\n  \"dark\": {}\n}\n";

impl App {
    /// Installs a theme file someone shared and switches to it.
    pub fn import_theme(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Theme", &["json"])
            .pick_file()
        else {
            return;
        };
        match theme::install(&path, &self.themes_dir) {
            Ok(id) => {
                self.reload_themes();
                let _ = self.db.store().settings().set(THEME, &id);
                self.reload_choices();
            }
            Err(e) => self.show_error(e),
        }
        self.apply_theme();
    }

    /// Removes the chosen theme if the user installed it. A bundled theme it replaced
    /// comes back; otherwise the default theme is used.
    pub fn remove_theme(&mut self) {
        let index = self.catalog.find(&self.choices.theme.theme);
        let def = &self.catalog.themes[index];
        if def.bundled {
            return;
        }
        let path = self.themes_dir.join(format!("{}.json", def.id));
        if let Err(e) = std::fs::remove_file(path) {
            self.show_error(e.to_string());
        }
        self.reload_themes();
        self.apply_theme();
    }

    /// Saves the current theme, with the user's changes, as a file to share.
    pub fn export_theme(&mut self) {
        let Some(resolved) = &self.theme else {
            return;
        };
        let text = theme::export(&self.catalog, resolved);
        let id = &self.catalog.themes[resolved.theme].id;
        let picked = rfd::FileDialog::new()
            .add_filter("Theme", &["json"])
            .set_file_name(format!("{id}-custom.json"))
            .save_file();
        if let Some(path) = picked
            && let Err(e) = std::fs::write(&path, text)
        {
            self.show_error(e.to_string());
        }
    }

    /// Creates `theme.json` if there is none, then opens it in the default editor.
    pub fn edit_theme_file(&mut self) {
        if !self.theme_file.exists()
            && let Err(e) = std::fs::write(&self.theme_file, STARTER)
        {
            self.show_error(e.to_string());
            return;
        }
        if let Err(e) = open(self.theme_file.as_os_str()) {
            self.show_error(e.to_string());
        }
        self.apply_theme();
    }

    /// Moves `theme.json` aside, so only the theme and the settings apply.
    pub fn stop_theme_file(&mut self) {
        if let Err(e) = std::fs::rename(&self.theme_file, backup_path(&self.theme_file)) {
            self.show_error(e.to_string());
        }
        self.apply_theme();
    }

    /// Opens a theme's website, source or donation page in the browser.
    pub fn open_link(&mut self, url: &str) {
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return;
        }
        if let Err(e) = open_url(url) {
            self.show_error(e.to_string());
        }
    }
}

/// Opens a link in the browser. On Windows this avoids `cmd`, which would split a
/// link at `&`.
fn open_url(url: &str) -> std::io::Result<()> {
    if cfg!(windows) {
        std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()
            .map(drop)
    } else {
        open(url.as_ref())
    }
}

fn backup_path(path: &Path) -> PathBuf {
    path.with_extension("json.bak")
}

/// Opens a file with the program the system uses for it.
fn open(target: &std::ffi::OsStr) -> std::io::Result<()> {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(target);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(target);
        c
    };
    command.spawn().map(drop)
}
