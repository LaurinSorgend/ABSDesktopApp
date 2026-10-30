//! One book's metadata, in the order of precedence of ABS: folder names, then audio tags or
//! the ebook, then `desc.txt` and `reader.txt`, `metadata.opf` and `metadata.json`.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use abs_api::models::Chapter;
use serde_json::Value;

use super::names::{FolderData, parse_names};
use super::probe::AudioTags;

/// Largest sidecar or EPUB metadata file read.
const MAX_TEXT_BYTES: u64 = 4 * 1024 * 1024;
/// Largest EPUB cover read.
const MAX_COVER_BYTES: u64 = 32 * 1024 * 1024;

/// A series and the book's place in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesEntry {
    pub name: String,
    pub sequence: Option<String>,
}

/// Everything known about a book, before it is stored.
#[derive(Debug, Clone, Default)]
pub struct BookMetadata {
    pub title: String,
    pub subtitle: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub series: Vec<SeriesEntry>,
    pub genres: Vec<String>,
    pub published_year: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub language: Option<String>,
    pub explicit: bool,
    pub abridged: bool,
    /// From `metadata.json`; replaces those found in the audio files.
    pub chapters: Vec<Chapter>,
}

impl BookMetadata {
    pub fn from_folder(data: FolderData) -> Self {
        Self {
            title: data.title,
            authors: data.authors,
            narrators: data.narrators,
            series: data
                .series
                .map(|name| SeriesEntry {
                    name,
                    sequence: data.sequence,
                })
                .into_iter()
                .collect(),
            published_year: data.published_year,
            asin: data.asin,
            ..Self::default()
        }
    }

    /// Tags of the first audio file.
    pub fn apply_tags(&mut self, tags: &AudioTags) {
        set(
            &mut self.narrators,
            tags.composer.as_deref().map(parse_names),
        );
        replace(
            &mut self.description,
            tags.description.clone().or_else(|| tags.comment.clone()),
        );
        replace(&mut self.publisher, tags.publisher.clone());
        replace(&mut self.published_year, tags.year.clone());
        replace(&mut self.subtitle, tags.subtitle.clone());
        if let Some(title) = tags.album.clone().or_else(|| tags.title.clone()) {
            self.title = title;
        }
        let authors = tags.artist.as_deref().or(tags.album_artist.as_deref());
        set(&mut self.authors, authors.map(parse_names));
        set(&mut self.genres, tags.genre.as_deref().map(parse_genres));
        set(&mut self.series, series_of(tags));
        replace(&mut self.isbn, tags.isbn.clone());
        replace(&mut self.language, tags.language.clone());
        replace(&mut self.asin, tags.asin.clone());
    }

    /// Metadata from an OPF file: inside an EPUB, or a `metadata.opf` next to the book.
    pub fn apply_opf(&mut self, opf: &OpfMetadata) {
        if let Some(title) = &opf.title {
            self.title = title.clone();
        }
        set(&mut self.authors, Some(opf.authors.clone()));
        set(&mut self.narrators, Some(opf.narrators.clone()));
        set(&mut self.genres, Some(opf.genres.clone()));
        set(&mut self.series, Some(opf.series.clone()));
        replace(&mut self.description, opf.description.clone());
        replace(&mut self.publisher, opf.publisher.clone());
        replace(&mut self.published_year, opf.year.clone());
        replace(&mut self.isbn, opf.isbn.clone());
        replace(&mut self.asin, opf.asin.clone());
        replace(&mut self.language, opf.language.clone());
    }

    /// `desc.txt` and `reader.txt` of the book folder.
    pub fn apply_text_files(&mut self, desc: Option<&str>, reader: Option<&str>) {
        let trimmed = desc.map(str::trim).filter(|d| !d.is_empty());
        replace(&mut self.description, trimmed.map(str::to_owned));
        let first_line = reader.and_then(|r| r.lines().next()).map(str::trim);
        let names = first_line.filter(|n| !n.is_empty()).map(parse_names);
        set(&mut self.narrators, names);
    }

    /// A `metadata.json` as ABS writes it. Unusable text changes nothing.
    pub fn apply_json(&mut self, text: &str) {
        let Ok(json) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let string = |key: &str| {
            json[key]
                .as_str()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_owned)
        };
        let strings = |key: &str| -> Option<Vec<String>> {
            let values: Vec<String> = json[key]
                .as_array()?
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            Some(values)
        };
        if let Some(title) = string("title") {
            self.title = title;
        }
        replace(&mut self.subtitle, string("subtitle"));
        replace(&mut self.published_year, string("publishedYear"));
        replace(&mut self.publisher, string("publisher"));
        replace(&mut self.description, string("description"));
        replace(&mut self.isbn, string("isbn"));
        replace(&mut self.asin, string("asin"));
        replace(&mut self.language, string("language"));
        set(&mut self.authors, strings("authors"));
        set(&mut self.narrators, strings("narrators"));
        set(&mut self.genres, strings("genres"));
        let series = strings("series").map(|list| list.iter().map(|s| parse_series(s)).collect());
        set(&mut self.series, series);
        self.explicit = json["explicit"].as_bool().unwrap_or(self.explicit);
        self.abridged = json["abridged"].as_bool().unwrap_or(self.abridged);
        self.chapters = json_chapters(&json).unwrap_or(std::mem::take(&mut self.chapters));
    }
}

fn replace(target: &mut Option<String>, value: Option<String>) {
    if value.is_some() {
        *target = value;
    }
}

fn set<T>(target: &mut Vec<T>, value: Option<Vec<T>>) {
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        *target = value;
    }
}

fn parse_genres(text: &str) -> Vec<String> {
    text.split([';', '/', ','])
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_owned)
        .collect()
}

fn series_of(tags: &AudioTags) -> Option<Vec<SeriesEntry>> {
    if let Some(name) = &tags.series {
        return Some(vec![SeriesEntry {
            name: name.clone(),
            sequence: tags.series_part.clone(),
        }]);
    }
    let grouping = tags.grouping.as_deref()?;
    Some(
        grouping
            .split(';')
            .map(|s| parse_series(s.trim()))
            .collect(),
    )
}

/// `Name #1a` as name and sequence.
fn parse_series(text: &str) -> SeriesEntry {
    match text.rsplit_once(" #") {
        Some((name, sequence)) if !sequence.is_empty() && !sequence.contains([' ', '#']) => {
            SeriesEntry {
                name: name.to_owned(),
                sequence: Some(sequence.to_owned()),
            }
        }
        _ => SeriesEntry {
            name: text.to_owned(),
            sequence: None,
        },
    }
}

fn json_chapters(json: &Value) -> Option<Vec<Chapter>> {
    let list = json["chapters"].as_array().filter(|l| !l.is_empty())?;
    Some(
        list.iter()
            .enumerate()
            .map(|(index, chapter)| Chapter {
                id: index as i64,
                start: chapter["start"].as_f64().unwrap_or_default(),
                end: chapter["end"].as_f64().unwrap_or_default(),
                title: chapter["title"].as_str().unwrap_or_default().to_owned(),
            })
            .collect(),
    )
}

/// What an OPF file says about a book.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpfMetadata {
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub narrators: Vec<String>,
    pub genres: Vec<String>,
    pub series: Vec<SeriesEntry>,
    pub description: Option<String>,
    pub publisher: Option<String>,
    pub year: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub language: Option<String>,
    /// The cover's path inside an EPUB, relative to the OPF file.
    pub cover_href: Option<String>,
}

/// Parses OPF XML. `None` when it is not XML.
pub fn parse_opf(xml: &str) -> Option<OpfMetadata> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).ok()?;
    let mut opf = OpfMetadata::default();
    let metadata = doc
        .descendants()
        .find(|n| n.tag_name().name() == "metadata")?;
    let elements = || metadata.children().filter(|n| n.is_element());
    let refined = |id: &str, property: &str| {
        let target = format!("#{id}");
        elements().find_map(|n| {
            (n.attribute("refines") == Some(target.as_str())
                && n.attribute("property") == Some(property))
            .then(|| node_text(n))
            .flatten()
        })
    };
    for node in elements() {
        let text = node_text(node);
        match node.tag_name().name() {
            "title" if opf.title.is_none() => opf.title = text,
            "creator" | "contributor" => {
                let id_role = node.attribute("id").and_then(|id| refined(id, "role"));
                let role = id_role.or_else(|| local_attribute(node, "role").map(str::to_owned));
                match (node.tag_name().name(), role.as_deref(), text) {
                    (_, Some("nrt" | "narrator"), Some(name)) => opf.narrators.push(name),
                    ("creator", None | Some("aut"), Some(name)) => opf.authors.push(name),
                    _ => {}
                }
            }
            "subject" => opf.genres.extend(text),
            "description" => opf.description = text,
            "publisher" => opf.publisher = text,
            "language" => opf.language = text,
            "date" => opf.year = text.as_deref().and_then(leading_year),
            "identifier" => read_identifier(node, text, &mut opf),
            _ => {}
        }
    }
    read_series(&elements().collect::<Vec<_>>(), &refined, &mut opf);
    opf.cover_href = cover_href(&doc, &elements().collect::<Vec<_>>());
    Some(opf)
}

fn node_text(node: roxmltree::Node<'_, '_>) -> Option<String> {
    let text: String = node
        .descendants()
        .filter(|n| n.is_text())
        .filter_map(|n| n.text())
        .collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// An attribute by its name without namespace, as `opf:role` and plain `role` both occur.
fn local_attribute<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn leading_year(text: &str) -> Option<String> {
    let year = text.get(..4)?;
    year.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| year.to_owned())
}

fn read_identifier(node: roxmltree::Node<'_, '_>, text: Option<String>, opf: &mut OpfMetadata) {
    let Some(text) = text else {
        return;
    };
    let scheme = local_attribute(node, "scheme")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let lower = text.to_ascii_lowercase();
    if scheme == "isbn" {
        opf.isbn = Some(text);
    } else if let Some(isbn) = lower.strip_prefix("urn:isbn:") {
        opf.isbn = Some(isbn.to_uppercase());
    } else if scheme == "asin" || scheme == "mobi-asin" || scheme == "amazon" {
        opf.asin = Some(text);
    }
}

/// `calibre:series` and its index, or an EPUB 3 collection with its position.
fn read_series(
    elements: &[roxmltree::Node<'_, '_>],
    refined: &dyn Fn(&str, &str) -> Option<String>,
    opf: &mut OpfMetadata,
) {
    let meta_named = |name: &str| {
        elements.iter().find_map(|n| {
            (n.attribute("name") == Some(name))
                .then(|| n.attribute("content").map(str::to_owned))
                .flatten()
        })
    };
    if let Some(name) = meta_named("calibre:series") {
        let sequence = meta_named("calibre:series_index")
            .map(|s| s.parse::<f64>().map_or(s, |n| n.to_string()));
        opf.series.push(SeriesEntry { name, sequence });
        return;
    }
    for node in elements {
        if node.attribute("property") == Some("belongs-to-collection")
            && let Some(name) = node_text(*node)
        {
            let sequence = node
                .attribute("id")
                .and_then(|id| refined(id, "group-position"));
            opf.series.push(SeriesEntry { name, sequence });
        }
    }
}

fn cover_href(
    doc: &roxmltree::Document<'_>,
    elements: &[roxmltree::Node<'_, '_>],
) -> Option<String> {
    let items = || doc.descendants().filter(|n| n.tag_name().name() == "item");
    let by_property = items().find(|n| {
        n.attribute("properties")
            .is_some_and(|p| p.split_whitespace().any(|v| v == "cover-image"))
    });
    let cover_id = elements.iter().find_map(|n| {
        (n.attribute("name") == Some("cover"))
            .then(|| n.attribute("content"))
            .flatten()
    });
    by_property
        .or_else(|| items().find(|n| cover_id.is_some() && n.attribute("id") == cover_id))
        .and_then(|n| n.attribute("href"))
        .map(str::to_owned)
}

/// The OPF metadata of an EPUB.
pub fn read_epub(path: &Path) -> Option<OpfMetadata> {
    let mut zip = open_epub(path)?;
    let (_, xml) = read_opf(&mut zip)?;
    parse_opf(&xml)
}

/// The cover image inside an EPUB.
pub fn epub_cover(path: &Path) -> Option<Vec<u8>> {
    let mut zip = open_epub(path)?;
    let (opf_path, xml) = read_opf(&mut zip)?;
    let href = parse_opf(&xml)?.cover_href?;
    let dir = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);
    let name = if dir.is_empty() {
        href
    } else {
        format!("{dir}/{href}")
    };
    read_entry(&mut zip, &name, MAX_COVER_BYTES)
}

fn open_epub(path: &Path) -> Option<zip::ZipArchive<File>> {
    zip::ZipArchive::new(File::open(path).ok()?).ok()
}

/// The path of the OPF file and its text, found through `container.xml`.
fn read_opf(zip: &mut zip::ZipArchive<File>) -> Option<(String, String)> {
    let container = read_entry(zip, "META-INF/container.xml", MAX_TEXT_BYTES)?;
    let container = String::from_utf8(container).ok()?;
    let doc = roxmltree::Document::parse(&container).ok()?;
    let path = doc
        .descendants()
        .find(|n| n.tag_name().name() == "rootfile")?
        .attribute("full-path")?
        .to_owned();
    let xml = String::from_utf8(read_entry(zip, &path, MAX_TEXT_BYTES)?).ok()?;
    Some((path, xml))
}

fn read_entry(zip: &mut zip::ZipArchive<File>, name: &str, limit: u64) -> Option<Vec<u8>> {
    let entry = zip.by_name(name).ok()?;
    let mut bytes = Vec::new();
    entry.take(limit).read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// A sidecar's text, cut off at the size limit.
pub fn read_text_file(path: &Path) -> Option<String> {
    let mut bytes = Vec::new();
    File::open(path)
        .ok()?
        .take(MAX_TEXT_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPF: &str = r##"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" xmlns:opf="http://www.idpf.org/2007/opf">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Dune</dc:title>
    <dc:creator opf:role="aut">Frank Herbert</dc:creator>
    <dc:creator id="n1">Scott Brick</dc:creator>
    <meta refines="#n1" property="role">nrt</meta>
    <dc:subject>Science Fiction</dc:subject>
    <dc:description>Spice.</dc:description>
    <dc:publisher>Chilton</dc:publisher>
    <dc:date>1965-08-01</dc:date>
    <dc:language>en</dc:language>
    <dc:identifier opf:scheme="ISBN">9780441013593</dc:identifier>
    <dc:identifier opf:scheme="ASIN">B002V1OF70</dc:identifier>
    <meta name="calibre:series" content="Dune Saga"/>
    <meta name="calibre:series_index" content="1.0"/>
    <meta name="cover" content="img"/>
  </metadata>
  <manifest><item id="img" href="images/c.jpg" media-type="image/jpeg"/></manifest>
</package>"##;

    #[test]
    fn reads_an_opf_file() {
        let opf = parse_opf(OPF).unwrap();
        assert_eq!(opf.title.as_deref(), Some("Dune"));
        assert_eq!(opf.authors, ["Frank Herbert"]);
        assert_eq!(opf.narrators, ["Scott Brick"]);
        assert_eq!(opf.genres, ["Science Fiction"]);
        assert_eq!(opf.year.as_deref(), Some("1965"));
        assert_eq!(opf.isbn.as_deref(), Some("9780441013593"));
        assert_eq!(opf.asin.as_deref(), Some("B002V1OF70"));
        assert_eq!(
            opf.series,
            [SeriesEntry {
                name: "Dune Saga".into(),
                sequence: Some("1".into())
            }]
        );
        assert_eq!(opf.cover_href.as_deref(), Some("images/c.jpg"));
        assert!(parse_opf("not xml").is_none());
    }

    #[test]
    fn epub3_collections_and_cover_property() {
        let opf = parse_opf(
            r##"<package><metadata xmlns:dc="d">
                <meta property="belongs-to-collection" id="c1">Saga</meta>
                <meta refines="#c1" property="group-position">2</meta>
               </metadata>
               <manifest><item properties="nav cover-image" href="c.png"/></manifest></package>"##,
        )
        .unwrap();
        assert_eq!(opf.series[0].name, "Saga");
        assert_eq!(opf.series[0].sequence.as_deref(), Some("2"));
        assert_eq!(opf.cover_href.as_deref(), Some("c.png"));
    }

    fn from_folder() -> BookMetadata {
        BookMetadata::from_folder(super::super::folder_data("Folder Author/Folder Title"))
    }

    #[test]
    fn tags_replace_folder_names() {
        let mut book = from_folder();
        book.apply_tags(&AudioTags::default());
        assert_eq!(book.title, "Folder Title");
        assert_eq!(book.authors, ["Folder Author"]);
        book.apply_tags(&AudioTags {
            title: Some("Track 1".into()),
            album: Some("Album Title".into()),
            album_artist: Some("Album Artist".into()),
            composer: Some("Reader One & Reader Two".into()),
            genre: Some("Fantasy; Epic".into()),
            comment: Some("From the comment".into()),
            grouping: Some("Saga #2; Other".into()),
            ..AudioTags::default()
        });
        assert_eq!(book.title, "Album Title");
        assert_eq!(book.authors, ["Album Artist"]);
        assert_eq!(book.narrators, ["Reader One", "Reader Two"]);
        assert_eq!(book.genres, ["Fantasy", "Epic"]);
        assert_eq!(book.description.as_deref(), Some("From the comment"));
        assert_eq!(book.series[0].sequence.as_deref(), Some("2"));
        assert_eq!(book.series[1].name, "Other");
    }

    #[test]
    fn later_sources_win() {
        let mut book = from_folder();
        book.apply_tags(&AudioTags {
            album: Some("Album".into()),
            ..AudioTags::default()
        });
        book.apply_text_files(Some(" Told plainly. \n"), Some("Reader\nsecond line"));
        assert_eq!(book.description.as_deref(), Some("Told plainly."));
        assert_eq!(book.narrators, ["Reader"]);
        book.apply_opf(&parse_opf(OPF).unwrap());
        assert_eq!(book.title, "Dune");
        assert_eq!(book.description.as_deref(), Some("Spice."));
        book.apply_json(
            r#"{"title":"From JSON","authors":["J"],"series":["S #3"],"abridged":true,
                "narrators":[],"chapters":[{"start":0,"end":5,"title":"One"}]}"#,
        );
        assert_eq!(book.title, "From JSON");
        assert_eq!(book.authors, ["J"]);
        assert_eq!(
            book.narrators,
            ["Scott Brick"],
            "an empty list changes nothing"
        );
        assert_eq!(book.series[0].sequence.as_deref(), Some("3"));
        assert!(book.abridged);
        assert_eq!(book.chapters[0].title, "One");
        book.apply_json("not json");
        assert_eq!(book.title, "From JSON");
    }

    #[test]
    fn series_text_takes_a_sequence_without_spaces() {
        assert_eq!(parse_series("Name #1a").sequence.as_deref(), Some("1a"));
        assert_eq!(parse_series("Name #1 a").sequence, None);
        assert_eq!(parse_series("Name #1 a").name, "Name #1 a");
    }
}
