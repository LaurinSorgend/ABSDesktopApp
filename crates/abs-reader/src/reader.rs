use abs_reader_ipc::{FromReader, OpenBook, Settings, ToReader};
use anyhow::Result;
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopWindowTarget};
use tao::window::{Window, WindowBuilder};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use crate::assets::Assets;
use crate::ipc::{self, UserEvent};

const PROTOCOL: &str = "abs";

/// The reader window. Hidden until the first book opens; later books reuse it.
pub struct Reader {
    window: Window,
    webview: Option<WebView>,
    assets: Assets,
    /// False when started with a book on the command line, where stdin is a terminal.
    pub exit_on_stdin_close: bool,
}

impl Reader {
    pub fn new(target: &EventLoopWindowTarget<UserEvent>, assets: Assets) -> Result<Self> {
        let window = WindowBuilder::new()
            .with_title("Reader")
            .with_inner_size(LogicalSize::new(900.0, 1000.0))
            .with_visible(false)
            .build(target)?;
        Ok(Self {
            window,
            webview: None,
            assets,
            exit_on_stdin_close: true,
        })
    }

    /// Shows `book`, replacing the open one.
    pub fn open(&mut self, book: OpenBook) -> Result<()> {
        self.window.set_title(&book.title);
        self.assets.set_book(book);
        match &self.webview {
            Some(webview) => webview.load_url(&page_url())?,
            None => self.webview = Some(build_webview(&self.window, self.assets.clone())?),
        }
        self.window.set_visible(true);
        self.window.set_minimized(false);
        self.window.set_focus();
        Ok(())
    }

    fn apply_settings(&self, settings: Settings) {
        let script = format!(
            "window.absReader?.applySettings({})",
            abs_reader_ipc::encode(&settings)
        );
        self.assets.set_settings(settings);
        if let Some(webview) = &self.webview {
            let _ = webview.evaluate_script(&script);
        }
    }

    pub fn handle(&mut self, event: Event<UserEvent>, control_flow: &mut ControlFlow) {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::UserEvent(UserEvent::Message(message)) => self.on_message(message, control_flow),
            Event::UserEvent(UserEvent::StdinClosed) if self.exit_on_stdin_close => {
                finish(control_flow)
            }
            Event::WindowEvent { event, .. } => match event {
                WindowEvent::CloseRequested => finish(control_flow),
                WindowEvent::Resized(_) => self.adjust_memory(),
                _ => {}
            },
            _ => {}
        }
    }

    fn on_message(&mut self, message: ToReader, control_flow: &mut ControlFlow) {
        match message {
            ToReader::Open(book) => {
                if let Err(e) = self.open(book) {
                    ipc::send(&FromReader::Error {
                        message: e.to_string(),
                    });
                }
            }
            ToReader::Settings { settings } => self.apply_settings(settings.clamped()),
            ToReader::Close => finish(control_flow),
        }
    }

    /// Asks WebView2 to give memory back while the window is minimised.
    #[cfg(windows)]
    fn adjust_memory(&self) {
        use wry::{MemoryUsageLevel, WebViewExtWindows};
        let Some(webview) = &self.webview else { return };
        let level = if self.window.is_minimized() {
            MemoryUsageLevel::Low
        } else {
            MemoryUsageLevel::Normal
        };
        let _ = webview.set_memory_usage_level(level);
    }

    #[cfg(not(windows))]
    fn adjust_memory(&self) {}
}

fn finish(control_flow: &mut ControlFlow) {
    ipc::send(&FromReader::Closed);
    *control_flow = ControlFlow::Exit;
}

fn build_webview(window: &Window, assets: Assets) -> Result<WebView> {
    let builder = WebViewBuilder::new()
        .with_custom_protocol(PROTOCOL.into(), move |_id, request| {
            assets.respond(&request)
        })
        .with_ipc_handler(|request| ipc::forward_from_page(request.body()))
        .with_navigation_handler(|url| url.starts_with(&origin()) || url.starts_with("blob:"))
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_devtools(cfg!(debug_assertions))
        .with_url(page_url());
    attach(builder, window)
}

#[cfg(not(target_os = "linux"))]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView> {
    Ok(builder.build(window)?)
}

#[cfg(target_os = "linux")]
fn attach(builder: WebViewBuilder, window: &Window) -> Result<WebView> {
    use anyhow::Context;
    use tao::platform::unix::WindowExtUnix;
    use wry::WebViewBuilderExtUnix;
    let vbox = window.default_vbox().context("tao window has no GTK box")?;
    Ok(builder.build_gtk(vbox)?)
}

fn page_url() -> String {
    format!("{}/index.html", origin())
}

/// Origin wry gives the custom protocol: WebView2 maps it to `http://<name>.localhost`.
fn origin() -> String {
    if cfg!(windows) {
        format!("http://{PROTOCOL}.localhost")
    } else {
        format!("{PROTOCOL}://localhost")
    }
}
