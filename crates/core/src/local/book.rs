//! Turns one group of files into what is stored for a book. The item JSON has the shape of
//! an ABS library item, so the catalog, the item page and the works read it unchanged.

use std::path::{Path, PathBuf};

use abs_api::Raw;
use abs_api::models::{Chapter, LibraryItem};
use serde_json::{Value, json};

use super::group::{BookGroup, FileKind, kind_of};
use super::metadata::{BookMetadata, parse_opf, read_epub, read_text_file};
use super::names::{folder_data, title_ignore_prefix};
use super::order::{AudioFacts, chapters, natural, smart_order};
use super::probe::{AudioProbe, probe};
use crate::db::{ItemRow, LocalBook, LocalTrack};

const IMAGES: [&str; 4] = ["png", "jpg", "jpeg", "webp"];

/// A file found in a chosen folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundFile {
    /// Relative to the chosen folder, with `/` as separator.
    pub rel_path: String,
    pub size: u64,
    /// Milliseconds since the Unix epoch.
    pub modified: i64,
}

/// A book ready to store.
#[derive(Debug, Clone)]
pub struct BuiltBook {
    pub item: ItemRow,
    pub book: LocalBook,
    pub tracks: Vec<LocalTrack>,
}

/// What identifies a group while scanning.
pub struct BookIds<'a> {
    pub item_id: &'a str,
    pub library_id: &'a str,
    pub folder_name: &'a str,
    pub signature: &'a str,
    pub fingerprint: &'a str,
    pub added_at: i64,
}

/// Reads a group's files. `None` when nothing in it can be played or read.
pub fn build_book(
    root: &Path,
    group: &BookGroup,
    files: &[FoundFile],
    ids: &BookIds<'_>,
) -> Option<BuiltBook> {
    let rel_path = group.rel_path();
    let folder = folder_data(if rel_path.is_empty() {
        ids.folder_name
    } else {
        &rel_path
    });
    let folder_title = folder.title.clone();
    let mut metadata = BookMetadata::from_folder(folder);
    let (facts, probes) = read_audio(root, group);
    let ebook = pick_ebook(group).map(|rel| root.join(rel));
    if facts.is_empty() && ebook.is_none() {
        return None;
    }
    let mut ordered = facts;
    smart_order(&mut ordered, &folder_title);
    if let Some((_, first)) = ordered
        .first()
        .and_then(|f| probes.iter().find(|p| p.0 == f.rel_path))
    {
        metadata.apply_tags(&first.tags);
    } else if let Some(opf) = ebook
        .as_deref()
        .filter(|path| is_epub(path))
        .and_then(read_epub)
    {
        metadata.apply_opf(&opf);
    }
    apply_sidecars(&mut metadata, root, group);

    let book_chapters = if metadata.chapters.is_empty() {
        chapters(&ordered, &folder_title)
    } else {
        std::mem::take(&mut metadata.chapters)
    };
    let tracks = tracks_of(root, &ordered, files);
    let duration: f64 = ordered.iter().map(|f| f.duration).sum();
    let cover = pick_cover(root, group, &ordered, &probes, ebook.as_deref());
    let updated_at = files
        .iter()
        .filter(|f| group.files.contains(&f.rel_path))
        .map(|f| f.modified)
        .max()
        .unwrap_or(ids.added_at);
    let dir = root.join(&group.dir);
    let view = ItemView {
        ids,
        rel_path: &rel_path,
        dir: &dir,
        metadata: &metadata,
        chapters: &book_chapters,
        track_count: ordered.len(),
        duration,
        has_cover: cover.is_some(),
        ebook: ebook.as_deref(),
        updated_at,
    };
    let raw: Raw<LibraryItem> = serde_json::from_str(&item_json(&view).to_string()).ok()?;
    Some(BuiltBook {
        item: ItemRow::from_api(&raw),
        book: LocalBook {
            item_id: ids.item_id.to_owned(),
            dir,
            ebook_path: ebook,
            cover_path: cover,
            signature: ids.signature.to_owned(),
            fingerprint: ids.fingerprint.to_owned(),
        },
        tracks,
    })
}

/// Probes the audio files in name order. A file that cannot be read is left out.
fn read_audio(root: &Path, group: &BookGroup) -> (Vec<AudioFacts>, Vec<(String, AudioProbe)>) {
    let mut facts = Vec::new();
    let mut probes = Vec::new();
    for rel in group.files.iter().filter(|f| kind_of(f) == FileKind::Audio) {
        match probe(&root.join(rel)) {
            Ok(found) if found.duration > 0.0 => {
                facts.push(AudioFacts {
                    rel_path: rel.clone(),
                    duration: found.duration,
                    title_tag: found.tags.title.clone(),
                    track_tag: found.tags.track,
                    disc_tag: found.tags.disc,
                    chapters: found.chapters.clone(),
                });
                probes.push((rel.clone(), found));
            }
            Ok(_) => eprintln!("local folder: {rel} has no length, skipped"),
            Err(e) => eprintln!("local folder: {rel}: {e}"),
        }
    }
    (facts, probes)
}

fn is_epub(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("epub"))
}

/// EPUB if there is one, else the first other ebook by name.
fn pick_ebook(group: &BookGroup) -> Option<&String> {
    let mut ebooks: Vec<&String> = group
        .files
        .iter()
        .filter(|f| kind_of(f) == FileKind::Ebook)
        .collect();
    ebooks.sort_by(|a, b| natural(a, b));
    ebooks
        .iter()
        .find(|f| is_epub(Path::new(f.as_str())))
        .or(ebooks.first())
        .copied()
}

fn apply_sidecars(metadata: &mut BookMetadata, root: &Path, group: &BookGroup) {
    let named = |name: &str| {
        group
            .files
            .iter()
            .find(|f| file_name(f).eq_ignore_ascii_case(name))
            .and_then(|f| read_text_file(&root.join(f)))
    };
    metadata.apply_text_files(named("desc.txt").as_deref(), named("reader.txt").as_deref());
    let opf = named("metadata.opf").or_else(|| {
        group
            .files
            .iter()
            .find(|f| has_extension(f, &["opf"]))
            .and_then(|f| read_text_file(&root.join(f)))
    });
    if let Some(opf) = opf.as_deref().and_then(parse_opf) {
        metadata.apply_opf(&opf);
    }
    if let Some(json) = named("metadata.json") {
        metadata.apply_json(&json);
    }
}

fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit('/').next().unwrap_or(rel_path)
}

fn has_extension(rel_path: &str, extensions: &[&str]) -> bool {
    file_name(rel_path)
        .rsplit_once('.')
        .is_some_and(|(_, ext)| extensions.iter().any(|e| ext.eq_ignore_ascii_case(e)))
}

fn tracks_of(root: &Path, ordered: &[AudioFacts], files: &[FoundFile]) -> Vec<LocalTrack> {
    let mut offset = 0.0;
    ordered
        .iter()
        .map(|fact| {
            let track = LocalTrack {
                path: root.join(&fact.rel_path),
                size: files
                    .iter()
                    .find(|f| f.rel_path == fact.rel_path)
                    .map_or(0, |f| f.size),
                duration: fact.duration,
                start_offset: offset,
            };
            offset += fact.duration;
            track
        })
        .collect()
}

/// `cover.*`, else the first image, else a picture in the audio files or the EPUB.
fn pick_cover(
    root: &Path,
    group: &BookGroup,
    ordered: &[AudioFacts],
    probes: &[(String, AudioProbe)],
    ebook: Option<&Path>,
) -> Option<PathBuf> {
    let mut images: Vec<&String> = group
        .files
        .iter()
        .filter(|f| has_extension(f, &IMAGES))
        .collect();
    images.sort_by(|a, b| natural(a, b));
    let image = images
        .iter()
        .find(|f| {
            file_name(f)
                .rsplit_once('.')
                .is_some_and(|(stem, _)| stem.eq_ignore_ascii_case("cover"))
        })
        .or(images.first());
    if let Some(image) = image {
        return Some(root.join(image));
    }
    let with_picture = ordered.iter().find(|f| {
        probes
            .iter()
            .any(|(rel, p)| *rel == f.rel_path && p.has_cover)
    });
    if let Some(file) = with_picture {
        return Some(root.join(&file.rel_path));
    }
    ebook
        .filter(|path| is_epub(path) && super::metadata::epub_cover(path).is_some())
        .map(Path::to_path_buf)
}

/// The fields of the item JSON.
struct ItemView<'a> {
    ids: &'a BookIds<'a>,
    rel_path: &'a str,
    dir: &'a Path,
    metadata: &'a BookMetadata,
    chapters: &'a [Chapter],
    track_count: usize,
    duration: f64,
    has_cover: bool,
    ebook: Option<&'a Path>,
    updated_at: i64,
}

fn item_json(view: &ItemView<'_>) -> Value {
    let meta = view.metadata;
    let chapters: Vec<Value> = view
        .chapters
        .iter()
        .map(|c| json!({ "id": c.id, "start": c.start, "end": c.end, "title": c.title }))
        .collect();
    let authors: Vec<Value> = meta
        .authors
        .iter()
        .map(|name| json!({ "id": "", "name": name }))
        .collect();
    let series: Vec<Value> = meta
        .series
        .iter()
        .map(|s| json!({ "id": "", "name": s.name, "sequence": s.sequence }))
        .collect();
    json!({
        "id": view.ids.item_id,
        "libraryId": view.ids.library_id,
        "mediaType": "book",
        "path": view.dir.to_string_lossy(),
        "relPath": view.rel_path,
        "addedAt": view.ids.added_at,
        "updatedAt": view.updated_at,
        "media": {
            "metadata": {
                "title": meta.title,
                "titleIgnorePrefix": title_ignore_prefix(&meta.title),
                "subtitle": meta.subtitle,
                "authors": authors,
                "narrators": meta.narrators,
                "series": series,
                "genres": meta.genres,
                "publishedYear": meta.published_year,
                "publisher": meta.publisher,
                "description": meta.description,
                "isbn": meta.isbn,
                "asin": meta.asin,
                "language": meta.language,
                "explicit": meta.explicit,
                "abridged": meta.abridged,
            },
            "coverPath": view.has_cover.then_some("local"),
            "duration": view.duration,
            "numTracks": view.track_count,
            "numChapters": view.chapters.len(),
            "chapters": chapters,
            "ebookFile": view.ebook.map(ebook_json),
        },
    })
}

fn ebook_json(path: &Path) -> Value {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let format = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    json!({
        "ebookFormat": format,
        "metadata": {
            "filename": name,
            "ext": format!(".{format}"),
            "path": path.to_string_lossy(),
        },
    })
}
