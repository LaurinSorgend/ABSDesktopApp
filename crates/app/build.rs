//! Compiles the Slint UI, bundles the themes from the themes repository and, for
//! Windows targets, embeds the app icon in the exe.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The themes repository. `ABS_THEMES_URL` points elsewhere, `ABS_THEMES_DIR` at a
/// checkout to use as it is (as CI does).
const THEMES_URL: &str = "git@github.com:LaurinSorgend/abs-desktop-themes.git";

fn main() {
    slint_build::compile("ui/app.slint").expect("compile the Slint UI");
    bundle_themes();
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
}

fn embed_windows_icon() {
    const ICON: &str = "../../packaging/icons/abs-desktop.ico";
    println!("cargo:rerun-if-changed={ICON}");
    winresource::WindowsResource::new()
        .set_icon(ICON)
        .compile()
        .expect("embed the Windows icon");
}

/// Copies `themes/*.json` of the repository into `OUT_DIR` and writes `themes.rs`,
/// which lists them for `include_str!`.
fn bundle_themes() {
    println!("cargo:rerun-if-env-changed=ABS_THEMES_DIR");
    println!("cargo:rerun-if-env-changed=ABS_THEMES_URL");
    // Packaging sets this to a new value so a release always has the latest themes.
    println!("cargo:rerun-if-env-changed=ABS_THEMES_FETCH");
    let repo = match std::env::var_os("ABS_THEMES_DIR") {
        Some(dir) => {
            let dir = PathBuf::from(dir);
            println!("cargo:rerun-if-changed={}", dir.join("themes").display());
            dir
        }
        None => fetch_themes(),
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let target = out.join("themes");
    let _ = std::fs::remove_dir_all(&target);
    std::fs::create_dir_all(&target).expect("create the themes folder");
    let mut files: Vec<PathBuf> = std::fs::read_dir(repo.join("themes"))
        .unwrap_or_else(|e| panic!("read {}: {e}", repo.join("themes").display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no themes in {}", repo.display());
    let mut list = String::from("/// Themes from the themes repository: id and file contents.\n");
    list += "pub const BUNDLED: &[(&str, &str)] = &[\n";
    for file in &files {
        let id = file.file_stem().expect("file name").to_string_lossy();
        let text = std::fs::read_to_string(file).expect("read a theme");
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", file.display()));
        let copy = target.join(format!("{id}.json"));
        std::fs::write(&copy, text).expect("copy a theme");
        list += &format!(
            "    ({id:?}, include_str!({:?})),\n",
            copy.display().to_string()
        );
    }
    list += "];\n";
    std::fs::write(out.join("themes.rs"), list).expect("write themes.rs");
}

/// Clones the repository next to the build output, or brings the clone up to date.
/// Without a network the last clone is used; without one either the build stops.
fn fetch_themes() -> PathBuf {
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    // OUT_DIR is <target>/<profile>/build/<package>/out; the clone is shared per profile.
    let cache = out
        .ancestors()
        .nth(3)
        .unwrap_or(&out)
        .join("abs-desktop-themes");
    let url = std::env::var("ABS_THEMES_URL").unwrap_or_else(|_| THEMES_URL.to_owned());
    let result = if cache.join(".git").exists() {
        git(&cache, &["fetch", "--depth", "1", "origin"])
            .and_then(|()| git(&cache, &["reset", "--hard", "FETCH_HEAD"]))
    } else {
        let _ = std::fs::remove_dir_all(&cache);
        let parent = cache.parent().expect("cache parent");
        std::fs::create_dir_all(parent).expect("create the cache folder");
        git(
            parent,
            &["clone", "--depth", "1", &url, &cache.to_string_lossy()],
        )
    };
    match result {
        Ok(()) => {}
        Err(e) if cache.join("themes").exists() => {
            println!("cargo:warning=themes not updated, using the last download: {e}");
        }
        Err(e) => panic!(
            "could not download the themes from {url}: {e}\n\
             Set ABS_THEMES_DIR to a checkout of the themes repository to build without it."
        ),
    }
    cache
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}
