//! Play order and chapters. Follows `runSmartTrackOrder` and `getBookChaptersFromAudioFiles` of ABS.

use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::sync::LazyLock;

use abs_api::models::Chapter;
use regex::Regex;

/// Embedded chapters shorter than this are neighbours' chapters copied into a file.
const MIN_CHAPTER_SECONDS: f64 = 0.1;

static DISC_IN_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:disc|cd) ?(\d\d?)\b").expect("valid regex"));
static DISC_FOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:cd|dis[ck])\s*(\d{1,3})$").expect("valid regex"));
static NUMBER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{1,4}").expect("valid regex"));

/// What ordering and chapters need to know about one audio file.
#[derive(Debug, Clone, Default)]
pub struct AudioFacts {
    /// Relative to the chosen folder, with `/` as separator.
    pub rel_path: String,
    pub duration: f64,
    pub title_tag: Option<String>,
    pub track_tag: Option<u32>,
    pub disc_tag: Option<u32>,
    /// Within this file.
    pub chapters: Vec<Chapter>,
}

/// Puts files in play order, by disc and track number from tags or file names.
pub fn smart_order(files: &mut [AudioFacts], book_title: &str) {
    files.sort_by(|a, b| natural(&a.rel_path, &b.rel_path));
    let from_names: Vec<(Option<u32>, Option<u32>)> = files
        .iter()
        .map(|f| numbers_from_name(&f.rel_path, book_title))
        .collect();
    let discs_in_tags: Vec<u32> = files.iter().filter_map(|f| f.disc_tag).collect();
    let discs_in_names: Vec<u32> = from_names.iter().filter_map(|n| n.0).collect();
    let count = files.len();
    let disc_key = |index: usize, file: &AudioFacts| {
        if discs_in_tags.len() == count && is_sequential(&discs_in_tags) {
            file.disc_tag
        } else if discs_in_names.len() == count && is_sequential(&discs_in_names) {
            from_names[index].0
        } else {
            None
        }
    };
    let distinct =
        |values: Vec<Option<u32>>| values.into_iter().flatten().collect::<BTreeSet<_>>().len();
    let names_win = distinct(from_names.iter().map(|n| n.1).collect())
        > distinct(files.iter().map(|f| f.track_tag).collect());
    let mut keyed: Vec<(u32, u32, AudioFacts)> = files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let track = if names_win {
                from_names[index].1
            } else {
                file.track_tag
            };
            (
                disc_key(index, file).unwrap_or(0),
                track.unwrap_or(0),
                file.clone(),
            )
        })
        .collect();
    keyed.sort_by_key(|(disc, track, _)| (*disc, *track));
    for (slot, (_, _, file)) in files.iter_mut().zip(keyed) {
        *slot = file;
    }
}

/// True when sorted values have no gap larger than one.
fn is_sequential(values: &[u32]) -> bool {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted.windows(2).all(|pair| pair[1] - pair[0] <= 1)
}

/// Disc and track from a path: a disc in the name or folder, then the first number left
/// after the book title is taken out of the name.
fn numbers_from_name(rel_path: &str, book_title: &str) -> (Option<u32>, Option<u32>) {
    let (dir, name) = rel_path.rsplit_once('/').unwrap_or(("", rel_path));
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let mut rest = if book_title.is_empty() {
        stem.to_owned()
    } else {
        stem.replacen(book_title, "", 1)
    };
    let mut disc = DISC_IN_NAME
        .captures(&rest)
        .and_then(|found| found[1].parse().ok());
    rest = DISC_IN_NAME.replace(&rest, "").into_owned();
    let folder = dir.rsplit('/').next().unwrap_or(dir);
    if let Some(found) = DISC_FOLDER.captures(folder) {
        disc = found[1].parse().ok().or(disc);
    }
    let track = NUMBER
        .find(&rest)
        .and_then(|found| found.as_str().parse().ok());
    (disc, track)
}

/// Compares names with runs of digits as numbers, ignoring case.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.to_lowercase(), b.to_lowercase());
    let (mut a, mut b) = (a.as_str(), b.as_str());
    while !a.is_empty() && !b.is_empty() {
        let digits_a = a.bytes().take_while(u8::is_ascii_digit).count();
        let digits_b = b.bytes().take_while(u8::is_ascii_digit).count();
        let order = if digits_a > 0 && digits_b > 0 {
            let number = |text: &str| text.trim_start_matches('0').to_owned();
            let (number_a, number_b) = (number(&a[..digits_a]), number(&b[..digits_b]));
            let order = number_a
                .len()
                .cmp(&number_b.len())
                .then(number_a.cmp(&number_b));
            a = &a[digits_a..];
            b = &b[digits_b..];
            order
        } else {
            let (char_a, char_b) = (first_char(a), first_char(b));
            a = &a[char_a.len_utf8()..];
            b = &b[char_b.len_utf8()..];
            char_a.cmp(&char_b)
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

fn first_char(text: &str) -> char {
    text.chars().next().unwrap_or_default()
}

/// A book's chapters from files in play order: embedded ones if the first file has any, else
/// one per file, named by title tag when each file has its own, else by file name.
pub fn chapters(files: &[AudioFacts], book_title: &str) -> Vec<Chapter> {
    let Some(first) = files.first() else {
        return Vec::new();
    };
    if !first.chapters.is_empty() {
        let shared = files.len() == 1 || same_chapters(first, &files[1]);
        return if shared {
            first.chapters.clone()
        } else {
            embedded_of_all(files)
        };
    }
    if files.len() == 1 {
        return Vec::new();
    }
    let unique_titles: BTreeSet<&str> = files
        .iter()
        .filter_map(|f| f.title_tag.as_deref())
        .filter(|title| !title.is_empty() && *title != book_title)
        .collect();
    let use_titles = unique_titles.len() == files.len();
    let mut start = 0.0;
    let mut result = Vec::new();
    for file in files.iter().filter(|f| f.duration > 0.0) {
        let title = match &file.title_tag {
            Some(title) if use_titles => title.clone(),
            _ => file_stem(&file.rel_path).to_owned(),
        };
        result.push(Chapter {
            id: result.len() as i64,
            start,
            end: start + file.duration,
            title,
        });
        start += file.duration;
    }
    result
}

fn same_chapters(a: &AudioFacts, b: &AudioFacts) -> bool {
    a.chapters.len() == b.chapters.len()
        && a.chapters
            .iter()
            .zip(&b.chapters)
            .all(|(x, y)| x.title == y.title && x.start == y.start)
}

fn embedded_of_all(files: &[AudioFacts]) -> Vec<Chapter> {
    let mut start = 0.0;
    let mut result = Vec::new();
    for file in files.iter().filter(|f| f.duration > 0.0) {
        for chapter in &file.chapters {
            if chapter.end - chapter.start >= MIN_CHAPTER_SECONDS {
                result.push(Chapter {
                    id: result.len() as i64,
                    start: chapter.start + start,
                    end: chapter.end + start,
                    title: chapter.title.clone(),
                });
            }
        }
        start += file.duration;
    }
    result
}

fn file_stem(rel_path: &str) -> &str {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    name.rsplit_once('.').map_or(name, |(stem, _)| stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(rel_path: &str) -> AudioFacts {
        AudioFacts {
            rel_path: rel_path.into(),
            duration: 10.0,
            ..AudioFacts::default()
        }
    }

    fn ordered(mut files: Vec<AudioFacts>, title: &str) -> Vec<String> {
        smart_order(&mut files, title);
        files.into_iter().map(|f| f.rel_path).collect()
    }

    #[test]
    fn numbers_in_names_order_tracks() {
        assert_eq!(
            ordered(
                vec![file("Part 10.mp3"), file("Part 2.mp3"), file("Part 1.mp3")],
                ""
            ),
            ["Part 1.mp3", "Part 2.mp3", "Part 10.mp3"]
        );
    }

    #[test]
    fn a_number_in_the_title_is_not_the_track() {
        assert_eq!(
            ordered(vec![file("1984 02.mp3"), file("1984 01.mp3")], "1984"),
            ["1984 01.mp3", "1984 02.mp3"]
        );
    }

    #[test]
    fn disc_folders_come_before_tracks() {
        assert_eq!(
            ordered(
                vec![
                    file("CD2/01.mp3"),
                    file("CD1/02.mp3"),
                    file("CD1/01.mp3"),
                    file("CD2/02.mp3")
                ],
                ""
            ),
            ["CD1/01.mp3", "CD1/02.mp3", "CD2/01.mp3", "CD2/02.mp3"]
        );
    }

    #[test]
    fn tags_win_when_they_tell_more_than_names() {
        let tagged = |name: &str, track| AudioFacts {
            track_tag: Some(track),
            ..file(name)
        };
        assert_eq!(
            ordered(vec![tagged("a.mp3", 2), tagged("b.mp3", 1)], ""),
            ["b.mp3", "a.mp3"]
        );
    }

    #[test]
    fn names_without_numbers_sort_naturally() {
        assert_eq!(
            ordered(vec![file("b.mp3"), file("A.mp3")], ""),
            ["A.mp3", "b.mp3"]
        );
    }

    #[test]
    fn natural_order_compares_numbers() {
        assert_eq!(natural("a2", "a10"), Ordering::Less);
        assert_eq!(natural("A02", "a2"), Ordering::Equal);
        assert_eq!(natural("a", "ab"), Ordering::Less);
        assert_eq!(natural("ä2", "ä10"), Ordering::Less);
    }

    fn chapter(id: i64, start: f64, end: f64, title: &str) -> Chapter {
        Chapter {
            id,
            start,
            end,
            title: title.into(),
        }
    }

    fn spans(chapters: &[Chapter]) -> Vec<(f64, f64, &str)> {
        chapters
            .iter()
            .map(|c| (c.start, c.end, c.title.as_str()))
            .collect()
    }

    #[test]
    fn one_file_uses_its_chapters() {
        let m4b = AudioFacts {
            chapters: vec![chapter(0, 0.0, 10.0, "One")],
            ..file("a.m4b")
        };
        assert_eq!(spans(&chapters(&[m4b], "")), [(0.0, 10.0, "One")]);
        assert!(chapters(&[file("a.mp3")], "").is_empty());
    }

    #[test]
    fn files_without_chapters_become_chapters() {
        assert_eq!(
            spans(&chapters(&[file("One.mp3"), file("x/Two.mp3")], "")),
            [(0.0, 10.0, "One"), (10.0, 20.0, "Two")]
        );
    }

    #[test]
    fn unique_title_tags_name_the_chapters() {
        let titled = |name: &str, title: &str| AudioFacts {
            title_tag: Some(title.into()),
            ..file(name)
        };
        assert_eq!(
            spans(&chapters(
                &[titled("1.mp3", "Start"), titled("2.mp3", "End")],
                "Book"
            )),
            [(0.0, 10.0, "Start"), (10.0, 20.0, "End")]
        );
        assert_eq!(
            spans(&chapters(
                &[titled("1.mp3", "Book"), titled("2.mp3", "Book")],
                "Book"
            )),
            [(0.0, 10.0, "1"), (10.0, 20.0, "2")]
        );
    }

    #[test]
    fn embedded_chapters_add_up_and_drop_empty_ones() {
        let with = |name: &str, list| AudioFacts {
            chapters: list,
            ..file(name)
        };
        let files = [
            with("1.m4b", vec![chapter(0, 0.0, 10.0, "One")]),
            with(
                "2.m4b",
                vec![chapter(0, 0.0, 0.05, "Copy"), chapter(1, 0.0, 10.0, "Two")],
            ),
        ];
        assert_eq!(
            spans(&chapters(&files, "")),
            [(0.0, 10.0, "One"), (10.0, 20.0, "Two")]
        );
    }

    #[test]
    fn the_same_chapters_in_every_file_count_once() {
        let list = vec![chapter(0, 0.0, 10.0, "One")];
        let files = [
            AudioFacts {
                chapters: list.clone(),
                ..file("1.m4b")
            },
            AudioFacts {
                chapters: list,
                ..file("2.m4b")
            },
        ];
        assert_eq!(chapters(&files, "").len(), 1);
    }
}
