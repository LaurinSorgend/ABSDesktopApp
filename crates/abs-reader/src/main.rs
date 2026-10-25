//! Ebook reader helper process: a tao window with a wry webview running foliate-js.
//!
//! The main app starts it without arguments and talks to it in JSON lines over stdin
//! and stdout (`abs-reader-ipc`). For testing, `abs-reader <book file> [location]`
//! opens a book directly.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod ipc;
mod reader;

use std::path::{Path, PathBuf};

use abs_reader_ipc::{OpenBook, Settings};
use anyhow::Result;
use tao::event_loop::EventLoopBuilder;

use assets::Assets;
use ipc::UserEvent;
use reader::Reader;

fn main() -> Result<()> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let from_args = book_from_args();
    let mut reader = Reader::new(&event_loop, Assets::new(foliate_dir()))?;
    reader.exit_on_stdin_close = from_args.is_none();
    if let Some(book) = from_args {
        reader.open(book)?;
    }
    ipc::spawn_stdin_reader(event_loop.create_proxy());
    event_loop.run(move |event, _, control_flow| reader.handle(event, control_flow));
}

fn book_from_args() -> Option<OpenBook> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(args.next()?);
    let location = args.next().map(|l| l.to_string_lossy().into_owned());
    Some(OpenBook {
        format: format_of(&path),
        title: title_of(&path),
        path,
        location,
        settings: Settings::default(),
    })
}

/// Where foliate-js is: `ABS_FOLIATE_DIR`, then `foliate-js` next to the executable,
/// then the submodule in the source tree.
fn foliate_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("ABS_FOLIATE_DIR") {
        return dir.into();
    }
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("foliate-js")));
    match beside_exe {
        Some(dir) if dir.is_dir() => dir,
        _ => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/foliate-js"),
    }
}

fn format_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("epub")
        .to_lowercase()
}

fn title_of(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Reader")
        .to_owned()
}
