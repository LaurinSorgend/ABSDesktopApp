//! Messages between the main app and `abs-reader`: one JSON object per line, main to
//! reader on the reader's stdin, reader to main on its stdout.

use std::path::PathBuf;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

/// From the main app to the reader.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ToReader {
    /// Shows a book; replaces the one that is open.
    Open(OpenBook),
    Settings {
        settings: Settings,
    },
    /// Closes the window and ends the process.
    Close,
}

/// A book file and where to start in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenBook {
    pub path: PathBuf,
    /// The file extension foliate-js should see, such as `epub`, `pdf` or `cbz`.
    pub format: String,
    pub title: String,
    /// A CFI for reflowable books, a 1-based page number for PDF and comics.
    pub location: Option<String>,
    pub settings: Settings,
}

/// From the reader to the main app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum FromReader {
    /// The book is open and shown at its start location.
    Ready,
    Relocate(Location),
    Toc {
        items: Vec<TocItem>,
    },
    /// The user changed the settings in the reader window.
    Settings {
        settings: Settings,
    },
    Error {
        message: String,
    },
    /// The window was closed; the process ends after this.
    Closed,
}

/// Where the reader is in the book.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Location {
    /// Point CFI of the start of the visible text, for reflowable books.
    pub cfi: Option<String>,
    /// 1-based page, for PDF and comics.
    pub page: Option<u32>,
    pub pages: Option<u32>,
    /// Fraction of the book read, 0 to 1.
    pub fraction: f64,
    pub chapter: Option<String>,
}

/// A table of contents entry.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TocItem {
    pub label: String,
    pub href: String,
    pub children: Vec<TocItem>,
}

/// How the reader lays out and colours text. The same for every book.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub flow: Flow,
    pub theme: Theme,
    pub font: Font,
    /// Percent of the book's font size.
    pub font_size: u32,
    /// Percent of the font size.
    pub line_height: u32,
    /// Side margin in percent of the page width.
    pub margin: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            flow: Flow::Paginated,
            theme: Theme::Light,
            font: Font::Original,
            font_size: 100,
            line_height: 140,
            margin: 6,
        }
    }
}

impl Settings {
    pub const FONT_SIZE: (u32, u32) = (50, 300);
    pub const LINE_HEIGHT: (u32, u32) = (100, 250);
    pub const MARGIN: (u32, u32) = (0, 20);

    /// The settings with every number inside its range.
    pub fn clamped(mut self) -> Self {
        self.font_size = self.font_size.clamp(Self::FONT_SIZE.0, Self::FONT_SIZE.1);
        self.line_height = self
            .line_height
            .clamp(Self::LINE_HEIGHT.0, Self::LINE_HEIGHT.1);
        self.margin = self.margin.clamp(Self::MARGIN.0, Self::MARGIN.1);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Flow {
    #[default]
    Paginated,
    Scrolled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    #[default]
    Light,
    Dark,
    Sepia,
}

/// `Original` keeps the book's own fonts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Font {
    #[default]
    Original,
    Serif,
    Sans,
}

/// One message as a line without the trailing newline.
pub fn encode(message: &impl Serialize) -> String {
    serde_json::to_string(message).expect("IPC messages always serialize")
}

/// Parses one line.
pub fn decode<T: DeserializeOwned>(line: &str) -> serde_json::Result<T> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_round_trip_as_tagged_json() {
        let open = ToReader::Open(OpenBook {
            path: "book.epub".into(),
            format: "epub".into(),
            title: "Alice".into(),
            location: Some("epubcfi(/6/4!/4/2/1:0)".into()),
            settings: Settings::default(),
        });
        let line = encode(&open);
        assert!(line.starts_with(r#"{"type":"open","path":"book.epub""#));
        assert_eq!(decode::<ToReader>(&line).unwrap(), open);
        assert_eq!(encode(&ToReader::Close), r#"{"type":"close"}"#);
    }

    #[test]
    fn reader_messages_tolerate_missing_fields() {
        let relocate: FromReader = decode(r#"{"type":"relocate","page":3,"pages":24}"#).unwrap();
        let FromReader::Relocate(location) = relocate else {
            panic!("not a relocate");
        };
        assert_eq!((location.page, location.pages), (Some(3), Some(24)));
        assert_eq!(location.fraction, 0.0);
        let settings: FromReader =
            decode(r#"{"type":"settings","settings":{"theme":"sepia"}}"#).unwrap();
        let FromReader::Settings { settings } = settings else {
            panic!("not settings");
        };
        assert_eq!(settings.theme, Theme::Sepia);
        assert_eq!(settings.font_size, 100);
    }

    #[test]
    fn clamps_numbers() {
        let wild = Settings {
            font_size: 1000,
            line_height: 10,
            margin: 50,
            ..Settings::default()
        }
        .clamped();
        assert_eq!(
            (wild.font_size, wild.line_height, wild.margin),
            (300, 100, 20)
        );
    }
}
