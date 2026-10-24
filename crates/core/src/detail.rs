//! What the item page shows, read from minified or expanded item JSON.

use abs_api::Raw;
use abs_api::models::{Chapter, LibraryItem};

use crate::db::ItemRow;

/// Item fields for the detail page.
#[derive(Debug, Clone, Default)]
pub struct ItemDetail {
    pub title: String,
    pub subtitle: Option<String>,
    pub author: Option<String>,
    pub narrator: Option<String>,
    pub series: Option<String>,
    pub published_year: Option<String>,
    pub genres: Vec<String>,
    /// Plain text; HTML from the server is reduced to paragraphs.
    pub description: Option<String>,
    pub duration: Option<f64>,
    /// Empty for minified JSON; the expanded item has them.
    pub chapters: Vec<Chapter>,
    pub has_audio: bool,
}

impl ItemDetail {
    /// Parses stored or freshly loaded item JSON.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let item: Raw<LibraryItem> = serde_json::from_str(json)?;
        let row = ItemRow::from_api(&item);
        let media = &item.media;
        let meta = &media.metadata;
        Ok(Self {
            title: row.title,
            subtitle: meta.subtitle.clone().filter(|s| !s.is_empty()),
            author: row.author,
            narrator: row.narrator,
            series: row.series,
            published_year: meta.published_year.clone(),
            genres: meta.genres.clone(),
            description: meta.description.as_deref().map(plain_text),
            duration: media.duration,
            chapters: media.chapters.clone(),
            has_audio: media.duration.unwrap_or_default() > 0.0
                || media.num_tracks.unwrap_or_default() > 0
                || !media.tracks.is_empty(),
        })
    }
}

/// Strips tags, turns block ends into line breaks and decodes the common entities.
pub fn plain_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>') else {
            rest = &rest[start..];
            break;
        };
        let tag = rest[start + 1..start + end].trim().to_ascii_lowercase();
        if is_break(&tag) {
            out.push('\n');
        }
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    let text = decode_entities(&out);
    collapse_blank_lines(&text)
}

fn is_break(tag: &str) -> bool {
    let name = tag.trim_start_matches('/').trim_end_matches('/').trim();
    let name = name.split_whitespace().next().unwrap_or_default();
    name == "br"
        || (tag.starts_with('/') && matches!(name, "p" | "div" | "li" | "h1" | "h2" | "h3"))
}

fn decode_entities(text: &str) -> String {
    text.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

fn collapse_blank_lines(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let mut out = Vec::new();
    for line in lines {
        if line.is_empty() && out.last().is_none_or(|l: &&str| l.is_empty()) {
            continue;
        }
        out.push(line);
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/api/2.37.2/{name}.json",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn reads_expanded_item() {
        let detail = ItemDetail::from_json(&fixture("item_multi_file")).unwrap();
        assert_eq!(detail.author.as_deref(), Some("Oscar Wilde"));
        assert!(!detail.chapters.is_empty());
        assert!(detail.has_audio);
    }

    #[test]
    fn html_becomes_paragraphs() {
        let html = "<p>One &amp; two</p><p>Three<br/>four</p>\n\n\n<b>five</b>";
        assert_eq!(plain_text(html), "One & two\nThree\nfour\n\nfive");
        assert_eq!(plain_text("a < b"), "a < b");
    }
}
