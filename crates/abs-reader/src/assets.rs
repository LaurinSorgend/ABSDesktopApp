use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

use abs_reader_ipc::{OpenBook, Settings};
use wry::http::{Request, Response, StatusCode, header};

const INDEX_HTML: &str = include_str!("../web/index.html");
const READER_JS: &str = include_str!("../web/reader.js");
const READER_CSS: &str = include_str!("../web/reader.css");
const FOLIATE_PREFIX: &str = "/foliate/";
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' blob:; \
    img-src 'self' blob: data:; font-src 'self' blob: data:; frame-src blob: 'self'; \
    worker-src 'self' blob:; connect-src 'self' blob: data:; media-src 'self' blob:; \
    object-src 'none'; base-uri 'none'; form-action 'none'";

/// What the custom protocol serves: the reader page, foliate-js and the open book.
/// Clones share the book, so the window can swap it.
#[derive(Clone)]
pub struct Assets {
    foliate_dir: PathBuf,
    book: Arc<Mutex<Option<OpenBook>>>,
}

impl Assets {
    pub fn new(foliate_dir: PathBuf) -> Self {
        Self {
            foliate_dir,
            book: Arc::default(),
        }
    }

    /// The book `/book` and `/meta.json` answer with from now on.
    pub fn set_book(&self, book: OpenBook) {
        *self.book.lock().unwrap_or_else(|p| p.into_inner()) = Some(book);
    }

    /// Keeps new settings with the book, so a reload of the page uses them.
    pub fn set_settings(&self, settings: Settings) {
        if let Some(book) = self.book.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            book.settings = settings;
        }
    }

    fn with_book<R>(&self, f: impl FnOnce(&OpenBook) -> R) -> Option<R> {
        self.book
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .map(f)
    }

    /// Answers one custom protocol request.
    pub fn respond(&self, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        let path = request.uri().path();
        match path {
            "/" | "/index.html" => html(INDEX_HTML),
            "/reader.js" => ok(READER_JS.as_bytes().to_vec(), "text/javascript"),
            "/reader.css" => ok(READER_CSS.as_bytes().to_vec(), "text/css"),
            "/meta.json" => match self.with_book(abs_reader_ipc::encode) {
                Some(json) => ok(json.into_bytes(), "application/json"),
                None => status(StatusCode::NOT_FOUND),
            },
            "/book" => match self.with_book(|b| b.path.clone()) {
                Some(path) => self.file(&path),
                None => status(StatusCode::NOT_FOUND),
            },
            _ => match path.strip_prefix(FOLIATE_PREFIX).and_then(safe_relative) {
                Some(relative) => self.file(&self.foliate_dir.join(relative)),
                None => status(StatusCode::NOT_FOUND),
            },
        }
    }

    fn file(&self, path: &Path) -> Response<Cow<'static, [u8]>> {
        match std::fs::read(path) {
            Ok(bytes) => ok(bytes, mime_for(path)),
            Err(_) => status(StatusCode::NOT_FOUND),
        }
    }
}

fn safe_relative(path: &str) -> Option<PathBuf> {
    let relative = PathBuf::from(path);
    relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        .then_some(relative)
}

fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("js" | "mjs") => "text/javascript",
        Some("css") => "text/css",
        Some("json") => "application/json",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("html") => "text/html",
        _ => "application/octet-stream",
    }
}

fn html(body: &'static str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html")
        .header(header::CONTENT_SECURITY_POLICY, CSP)
        .body(Cow::Borrowed(body.as_bytes()))
        .expect("static response")
}

fn ok(body: Vec<u8>, content_type: &str) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(Cow::Owned(body))
        .expect("static response")
}

fn status(code: StatusCode) -> Response<Cow<'static, [u8]>> {
    Response::builder()
        .status(code)
        .body(Cow::Borrowed(&[][..]))
        .expect("static response")
}

#[cfg(test)]
mod tests {
    use super::safe_relative;

    #[test]
    fn rejects_traversal() {
        assert!(safe_relative("view.js").is_some());
        assert!(safe_relative("vendor/zip.js").is_some());
        assert!(safe_relative("../secret").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
    }
}
