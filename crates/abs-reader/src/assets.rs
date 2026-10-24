use std::borrow::Cow;
use std::path::{Component, Path, PathBuf};

use wry::http::{Request, Response, StatusCode, header};

const INDEX_HTML: &str = include_str!("../web/index.html");
const READER_JS: &str = include_str!("../web/reader.js");
const FOLIATE_PREFIX: &str = "/foliate/";
const CSP: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline' blob:; \
    img-src 'self' blob: data:; font-src 'self' blob: data:; frame-src blob: 'self'; \
    connect-src 'self'; object-src 'none'; base-uri 'none'";

/// What the custom protocol serves: the reader page, foliate-js and one book.
pub struct Assets {
    foliate_dir: PathBuf,
    book_path: PathBuf,
    meta_json: String,
}

impl Assets {
    /// Serves `book_path` with the given format and saved location.
    pub fn new(
        foliate_dir: PathBuf,
        book_path: PathBuf,
        format: &str,
        location: Option<&str>,
    ) -> Self {
        let meta_json = serde_json::json!({ "format": format, "location": location }).to_string();
        Self {
            foliate_dir,
            book_path,
            meta_json,
        }
    }

    /// Answers one custom protocol request.
    pub fn respond(&self, request: &Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
        let path = request.uri().path();
        match path {
            "/" | "/index.html" => html(INDEX_HTML),
            "/reader.js" => ok(READER_JS.as_bytes().to_vec(), "text/javascript"),
            "/meta.json" => ok(self.meta_json.clone().into_bytes(), "application/json"),
            "/book" => self.file(&self.book_path),
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
