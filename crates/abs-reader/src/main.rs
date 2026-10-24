//! Ebook reader helper process: a tao window with a wry webview running foliate-js.
//!
//! `abs-reader <book file> [saved CFI]`. Prints one JSON message per line on stdout.

mod assets;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop};
use tao::window::{Window, WindowBuilder};
use wry::{WebView, WebViewBuilder};

use assets::Assets;

const PROTOCOL: &str = "abs";

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let book = PathBuf::from(
        args.next()
            .context("usage: abs-reader <book file> [saved CFI]")?,
    );
    let location = args.next();
    let format = book
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("epub")
        .to_lowercase();
    let assets = Assets::new(foliate_dir(), book.clone(), &format, location.as_deref());

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title(window_title(&book))
        .build(&event_loop)?;
    let _webview = build_webview(&window, assets)?;
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        if let Event::WindowEvent {
            event: WindowEvent::CloseRequested,
            ..
        } = event
        {
            println!(r#"{{"type":"closed"}}"#);
            *control_flow = ControlFlow::Exit;
        }
    });
}

fn build_webview(window: &Window, assets: Assets) -> Result<WebView> {
    let builder = WebViewBuilder::new()
        .with_custom_protocol(PROTOCOL.into(), move |_id, request| {
            assets.respond(&request)
        })
        .with_ipc_handler(|request| println!("{}", request.body()))
        .with_navigation_handler(|url| url.starts_with(&start_origin()) || url.starts_with("blob:"))
        .with_devtools(cfg!(debug_assertions))
        .with_url(format!("{}/index.html", start_origin()));
    attach(builder, window)
}

#[cfg(not(target_os = "linux"))]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView> {
    Ok(builder.build(window)?)
}

#[cfg(target_os = "linux")]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView> {
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;
    let vbox = window.default_vbox().context("tao window has no GTK box")?;
    Ok(builder.build_gtk(vbox)?)
}

/// Origin wry gives the custom protocol: WebView2 maps it to `http://<name>.localhost`.
fn start_origin() -> String {
    if cfg!(windows) {
        format!("http://{PROTOCOL}.localhost")
    } else {
        format!("{PROTOCOL}://localhost")
    }
}

fn foliate_dir() -> PathBuf {
    std::env::var_os("ABS_FOLIATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/foliate-js"))
}

fn window_title(book: &Path) -> String {
    book.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Reader")
        .to_owned()
}
