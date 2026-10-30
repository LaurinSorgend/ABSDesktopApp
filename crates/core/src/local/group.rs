//! Which files make up which book. Follows `groupFileItemsIntoLibraryItemDirs` of ABS: the
//! deepest folder with media is a book, and a parent that is a book takes in its subfolders.
//! People pick any folder here, not a managed library, so loose files are treated differently:
//! see [`split_loose`].

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

use crate::ebooks::is_readable;

const AUDIO: [&str; 11] = [
    "m4b", "m4a", "mp4", "mp3", "flac", "ogg", "opus", "oga", "aac", "wav", "aiff",
];
const WHOLE_BOOK_AUDIO: [&str; 3] = ["m4b", "m4a", "mp4"];

static DISC_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(cd|dis[ck])\s*\d{1,3}$").expect("valid regex"));
static PART_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)[\s._-]*(part|pt|disc|disk|cd|vol|volume)?[\s._-]*\d+$").expect("valid regex")
});

/// What a file is to the scanner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Audio,
    Ebook,
    Other,
}

/// The kind of a file, from its extension.
pub fn kind_of(path: &str) -> FileKind {
    match extension(path).as_str() {
        ext if AUDIO.contains(&ext) => FileKind::Audio,
        ext if is_readable(ext) => FileKind::Ebook,
        _ => FileKind::Other,
    }
}

/// One book's files, relative to the chosen folder with `/` as separator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BookGroup {
    /// The book's folder; empty for the chosen folder itself.
    pub dir: String,
    /// The book's name when it is not the whole folder: a file stem or an album.
    pub name: Option<String>,
    pub files: Vec<String>,
}

impl BookGroup {
    /// The path naming this book, for its id and its folder name metadata.
    pub fn rel_path(&self) -> String {
        match (&self.name, self.dir.is_empty()) {
            (Some(name), false) => format!("{}/{name}", self.dir),
            (Some(name), true) => name.clone(),
            (None, _) => self.dir.clone(),
        }
    }
}

/// Groups the files of a chosen folder into books. `album` gives a file's album tag; it is
/// only asked for files where it decides something.
pub fn group_files(files: &[String], album: &dyn Fn(&str) -> Option<String>) -> Vec<BookGroup> {
    let (media, others): (Vec<&String>, Vec<&String>) =
        files.iter().partition(|f| kind_of(f) != FileKind::Other);
    let mut media = media;
    media.sort_by_key(|f| (f.matches('/').count(), f.as_str()));

    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut loose = Vec::new();
    for file in media {
        match book_folder(&folders, file) {
            Some(folder) => folders.entry(folder).or_default().push(file.clone()),
            None => loose.push(file.clone()),
        }
    }

    let mut loose_others = Vec::new();
    for file in others {
        match owning_folder(&folders, file) {
            Some(folder) => folders.get_mut(&folder).expect("found").push(file.clone()),
            None if !file.contains('/') => loose_others.push(file.clone()),
            None => {}
        }
    }

    let mut books = split_loose("", &loose, &loose_others, album, true);
    for (dir, files) in &folders {
        let (media, others): (Vec<String>, Vec<String>) = files
            .iter()
            .cloned()
            .partition(|f| kind_of(f) != FileKind::Other);
        if holds_several_books(&media, album) {
            books.extend(split_loose(dir, &media, &others, album, false));
        } else {
            books.push(BookGroup {
                dir: dir.clone(),
                name: None,
                files: files.clone(),
            });
        }
    }
    books
}

/// The folder a media file's book is in; `None` for a file directly in the chosen folder.
fn book_folder(folders: &BTreeMap<String, Vec<String>>, file: &str) -> Option<String> {
    let (dir, _) = file.rsplit_once('/')?;
    let parts: Vec<&str> = dir.split('/').collect();
    for depth in 1..=parts.len() {
        let prefix = parts[..depth].join("/");
        let is_last = depth == parts.len();
        let disc_is_next = depth + 1 == parts.len() && DISC_FOLDER.is_match(parts[depth]);
        if folders.contains_key(&prefix) || is_last || disc_is_next {
            return Some(prefix);
        }
    }
    None
}

/// The book folder a non-media file lies in, if any.
fn owning_folder(folders: &BTreeMap<String, Vec<String>>, file: &str) -> Option<String> {
    let (dir, _) = file.rsplit_once('/')?;
    let parts: Vec<&str> = dir.split('/').collect();
    (1..=parts.len())
        .map(|depth| parts[..depth].join("/"))
        .find(|prefix| folders.contains_key(prefix))
}

/// True for a folder of whole books rather than one book: several M4B/M4A with different
/// names and nothing else to play, or several differently named ebooks without audio.
fn holds_several_books(media: &[String], album: &dyn Fn(&str) -> Option<String>) -> bool {
    let direct: Vec<&String> = media.iter().filter(|f| !is_nested(f, media)).collect();
    let audio: Vec<&&String> = direct
        .iter()
        .filter(|f| kind_of(f) == FileKind::Audio)
        .collect();
    if audio.is_empty() {
        return distinct(direct.iter().map(|f| stem(f))) > 1;
    }
    audio.iter().all(|f| is_whole_book(f)) && distinct(audio.iter().map(|f| book_key(f, album))) > 1
}

/// True when the file is deeper than the shallowest media file of its group.
fn is_nested(file: &str, media: &[String]) -> bool {
    let depth = |f: &str| f.matches('/').count();
    let top = media.iter().map(|f| depth(f)).min().unwrap_or(0);
    depth(file) > top
}

/// Sorts loose files into books:
/// - M4B and M4A are a book each, or parts of one when album or name say so.
/// - Other audio is one book per album. Without albums it all goes together (`whole_folder`).
/// - An ebook joins the audio with the same file stem, else it is a book of its own.
fn split_loose(
    dir: &str,
    media: &[String],
    others: &[String],
    album: &dyn Fn(&str) -> Option<String>,
    whole_folder: bool,
) -> Vec<BookGroup> {
    let mut books: BTreeMap<Option<String>, Vec<String>> = BTreeMap::new();
    let (audio, ebooks): (Vec<&String>, Vec<&String>) =
        media.iter().partition(|f| kind_of(f) == FileKind::Audio);
    let mut stems: BTreeMap<String, Option<String>> = BTreeMap::new();
    for file in audio {
        let key = if is_whole_book(file) {
            Some(book_key(file, album))
        } else {
            album(file).filter(|_| whole_folder)
        };
        stems.insert(stem(file), key.clone());
        books.entry(key).or_default().push(file.clone());
    }
    for file in ebooks {
        let key = match stems.get(&stem(file)) {
            Some(key) => key.clone(),
            None => Some(stem(file)),
        };
        books.entry(key).or_default().push(file.clone());
    }
    let single = books.len() == 1;
    for file in others {
        let key = stems.get(&stem(file)).cloned().or_else(|| {
            books
                .contains_key(&Some(stem(file)))
                .then(|| Some(stem(file)))
        });
        let key = key.or_else(|| single.then(|| books.keys().next().cloned().flatten()));
        if let Some(key) = key {
            books.entry(key).or_default().push(file.clone());
        }
    }
    books
        .into_iter()
        .map(|(name, files)| BookGroup {
            dir: dir.to_owned(),
            name,
            files,
        })
        .collect()
}

fn is_whole_book(file: &str) -> bool {
    WHOLE_BOOK_AUDIO.contains(&extension(file).as_str())
}

/// What parts of one book have in common: the album, else the name without a part number.
fn book_key(file: &str, album: &dyn Fn(&str) -> Option<String>) -> String {
    album(file).unwrap_or_else(|| {
        let name = stem(file);
        let trimmed = PART_SUFFIX.replace(&name, "");
        if trimmed.is_empty() {
            name
        } else {
            trimmed.into_owned()
        }
    })
}

fn distinct(values: impl Iterator<Item = String>) -> usize {
    let mut values: Vec<String> = values.collect();
    values.sort();
    values.dedup();
    values.len()
}

fn extension(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(String::new(), |(_, ext)| ext.to_ascii_lowercase())
}

fn stem(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.rsplit_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(files: &[&str]) -> Vec<BookGroup> {
        let files: Vec<String> = files.iter().map(|f| (*f).to_owned()).collect();
        group_files(&files, &|_| None)
    }

    fn book(dir: &str, name: Option<&str>, files: &[&str]) -> BookGroup {
        BookGroup {
            dir: dir.to_owned(),
            name: name.map(str::to_owned),
            files: files.iter().map(|f| (*f).to_owned()).collect(),
        }
    }

    #[test]
    fn a_folder_of_mp3s_is_one_book() {
        assert_eq!(
            groups(&[
                "Author/Dune/1.mp3",
                "Author/Dune/2.mp3",
                "Author/Dune/cover.jpg"
            ]),
            [book(
                "Author/Dune",
                None,
                &[
                    "Author/Dune/1.mp3",
                    "Author/Dune/2.mp3",
                    "Author/Dune/cover.jpg"
                ]
            )]
        );
    }

    #[test]
    fn disc_folders_belong_to_the_book() {
        assert_eq!(
            groups(&["Dune/CD1/1.mp3", "Dune/Disc 2/1.mp3"]),
            [book("Dune", None, &["Dune/CD1/1.mp3", "Dune/Disc 2/1.mp3"])]
        );
    }

    #[test]
    fn a_parent_with_media_takes_in_its_subfolders() {
        assert_eq!(
            groups(&["Dune/1.mp3", "Dune/extra/2.mp3"]),
            [book("Dune", None, &["Dune/1.mp3", "Dune/extra/2.mp3"])]
        );
    }

    #[test]
    fn loose_mp3s_in_the_chosen_folder_are_one_book() {
        assert_eq!(
            groups(&["01.mp3", "02.mp3", "cover.jpg"]),
            [book("", None, &["01.mp3", "02.mp3", "cover.jpg"])]
        );
    }

    #[test]
    fn loose_mp3s_split_by_album() {
        let files: Vec<String> = ["a1.mp3", "a2.mp3", "b1.mp3"].map(str::to_owned).to_vec();
        let found = group_files(&files, &|f| Some(f[..1].to_owned()));
        assert_eq!(
            found,
            [
                book("", Some("a"), &["a1.mp3", "a2.mp3"]),
                book("", Some("b"), &["b1.mp3"])
            ]
        );
    }

    #[test]
    fn m4bs_in_one_folder_are_separate_books() {
        assert_eq!(
            groups(&["Author/Dune.m4b", "Author/Emma.m4b"]),
            [
                book("Author", Some("Dune"), &["Author/Dune.m4b"]),
                book("Author", Some("Emma"), &["Author/Emma.m4b"])
            ]
        );
    }

    #[test]
    fn m4b_parts_stay_one_book() {
        assert_eq!(
            groups(&["Dune/Dune Part 1.m4b", "Dune/Dune Part 2.m4b"]),
            [book(
                "Dune",
                None,
                &["Dune/Dune Part 1.m4b", "Dune/Dune Part 2.m4b"]
            )]
        );
    }

    #[test]
    fn m4bs_with_the_same_album_are_one_book() {
        let files: Vec<String> = ["Dune/x.m4b", "Dune/y.m4b"].map(str::to_owned).to_vec();
        let found = group_files(&files, &|_| Some("Dune".to_owned()));
        assert_eq!(found, [book("Dune", None, &["Dune/x.m4b", "Dune/y.m4b"])]);
    }

    #[test]
    fn audio_and_ebook_with_one_stem_are_one_book() {
        assert_eq!(
            groups(&["Dune.m4b", "Dune.epub", "Emma.pdf"]),
            [
                book("", Some("Dune"), &["Dune.m4b", "Dune.epub"]),
                book("", Some("Emma"), &["Emma.pdf"])
            ]
        );
    }

    #[test]
    fn a_folder_with_a_differently_named_ebook_stays_one_book() {
        assert_eq!(
            groups(&["Dune/book.m4b", "Dune/novel.epub"]),
            [book("Dune", None, &["Dune/book.m4b", "Dune/novel.epub"])]
        );
    }

    #[test]
    fn ebooks_with_different_names_are_separate_books() {
        assert_eq!(
            groups(&["Author/Dune.epub", "Author/Dune.pdf", "Author/Emma.epub"]),
            [
                book(
                    "Author",
                    Some("Dune"),
                    &["Author/Dune.epub", "Author/Dune.pdf"]
                ),
                book("Author", Some("Emma"), &["Author/Emma.epub"])
            ]
        );
    }

    #[test]
    fn a_folder_without_media_is_no_book() {
        assert!(groups(&["Notes/readme.txt", "picture.png"]).is_empty());
    }

    #[test]
    fn rel_path_names_the_book() {
        assert_eq!(book("A/B", None, &[]).rel_path(), "A/B");
        assert_eq!(book("A", Some("Dune"), &[]).rel_path(), "A/Dune");
        assert_eq!(book("", Some("Dune"), &[]).rel_path(), "Dune");
        assert_eq!(book("", None, &[]).rel_path(), "");
    }
}
