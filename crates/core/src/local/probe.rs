//! Reads tags, length and chapters from audio files. Only headers are read, so scanning a
//! long M4B costs little. The tag names are those of `parseTags` in ABS.

use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

use abs_api::models::Chapter;
use lofty::config::ParseOptions;
use lofty::file::AudioFile;
use lofty::flac::FlacFile;
use lofty::ogg::{OpusFile, VorbisFile};
use lofty::picture::PictureType;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::Tag;
use mp4ameta::{FreeformIdent, ReadConfig};

const APPLE_FREEFORM: &str = "com.apple.iTunes";

/// The tags a book's metadata can come from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioTags {
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub album: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub composer: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub publisher: Option<String>,
    pub description: Option<String>,
    pub comment: Option<String>,
    pub series: Option<String>,
    pub series_part: Option<String>,
    pub grouping: Option<String>,
    pub isbn: Option<String>,
    pub asin: Option<String>,
    pub language: Option<String>,
    pub track: Option<u32>,
    pub disc: Option<u32>,
}

/// What an audio file says about itself.
#[derive(Debug, Clone, Default)]
pub struct AudioProbe {
    pub duration: f64,
    pub tags: AudioTags,
    /// Within this file.
    pub chapters: Vec<Chapter>,
    pub has_cover: bool,
}

/// Reads one audio file. The error is a text for the log.
pub fn probe(path: &Path) -> Result<AudioProbe, String> {
    let file = open(path, false)?;
    let duration = file.properties().duration().as_secs_f64();
    let mut tags = file
        .primary_tag()
        .or_else(|| file.first_tag())
        .map(common_tags)
        .unwrap_or_default();
    let has_cover = open(path, true).is_ok_and(|file| file.tags().iter().any(has_picture));
    let chapters = match extension(path).as_str() {
        "m4b" | "m4a" | "mp4" => read_mp4(path, duration, &mut tags),
        "mp3" | "aiff" | "wav" | "aac" => read_id3(path, duration, &mut tags),
        "flac" | "ogg" | "oga" | "opus" => read_vorbis(path, duration, &mut tags),
        _ => Vec::new(),
    };
    Ok(AudioProbe {
        duration,
        tags,
        chapters,
        has_cover,
    })
}

/// The album tag alone, for telling books apart while grouping.
pub fn album_of(path: &Path) -> Option<String> {
    let file = open(path, false).ok()?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    clean(tag.album().as_deref())
}

/// The first picture embedded in an audio file, preferring the front cover.
pub fn embedded_cover(path: &Path) -> Option<Vec<u8>> {
    let file = open(path, true).ok()?;
    let pictures = || file.tags().iter().flat_map(|tag| tag.pictures());
    let picture = pictures()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| pictures().next())?;
    Some(picture.data().to_vec())
}

fn open(path: &Path, with_pictures: bool) -> Result<lofty::file::TaggedFile, String> {
    let options = ParseOptions::new().read_cover_art(with_pictures);
    Probe::open(path)
        .map_err(|e| e.to_string())?
        .options(options)
        .read()
        .map_err(|e| e.to_string())
}

fn has_picture(tag: &Tag) -> bool {
    !tag.pictures().is_empty()
}

fn common_tags(tag: &Tag) -> AudioTags {
    let text = |key| clean(tag.get_string(key));
    AudioTags {
        title: clean(tag.title().as_deref()),
        subtitle: text(ItemKey::TrackSubtitle),
        album: clean(tag.album().as_deref()),
        artist: clean(tag.artist().as_deref()),
        album_artist: text(ItemKey::AlbumArtist),
        composer: text(ItemKey::Composer),
        genre: clean(tag.genre().as_deref()),
        year: tag.date().map(|date| date.year.to_string()),
        publisher: text(ItemKey::Publisher).or_else(|| text(ItemKey::Label)),
        description: text(ItemKey::Description),
        comment: clean(tag.comment().as_deref()),
        grouping: text(ItemKey::ContentGroup),
        language: text(ItemKey::Language),
        track: tag.track(),
        disc: tag.disk(),
        ..AudioTags::default()
    }
}

fn clean(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// Adds custom MP4 tags and returns the chapter list or chapter track.
fn read_mp4(path: &Path, duration: f64, tags: &mut AudioTags) -> Vec<Chapter> {
    let config = ReadConfig {
        read_image_data: false,
        ..ReadConfig::DEFAULT
    };
    let Ok(tag) = mp4ameta::Tag::read_with_path(path, &config) else {
        return Vec::new();
    };
    let freeform = |name: &'static str| {
        let ident = FreeformIdent::new_static(APPLE_FREEFORM, name);
        clean(tag.strings_of(&ident).next())
    };
    tags.series = freeform("SERIES").or_else(|| clean(tag.movement()));
    tags.series_part =
        freeform("SERIES-PART").or_else(|| tag.movement_index().map(|n| n.to_string()));
    tags.isbn = freeform("ISBN");
    tags.asin = freeform("ASIN").or_else(|| freeform("AUDIBLE_ASIN"));
    let list = if tag.chapter_list().is_empty() {
        tag.chapter_track()
    } else {
        tag.chapter_list()
    };
    let starts = list
        .iter()
        .map(|c| (c.start.as_secs_f64(), c.title.clone()))
        .collect();
    from_starts(starts, duration)
}

/// Adds `TXXX` tags and returns the `CHAP` frames in time order.
fn read_id3(path: &Path, duration: f64, tags: &mut AudioTags) -> Vec<Chapter> {
    let Ok(tag) = id3::Tag::read_from_path(path) else {
        return Vec::new();
    };
    let custom = |name: &str| {
        tag.extended_texts()
            .find(|t| t.description.eq_ignore_ascii_case(name))
            .and_then(|t| clean(Some(&t.value)))
    };
    tags.series = custom("series");
    tags.series_part = custom("series-part");
    tags.isbn = custom("isbn");
    tags.asin = custom("asin").or_else(|| custom("audible_asin"));
    let mut frames: Vec<&id3::frame::Chapter> = tag.chapters().collect();
    frames.sort_by_key(|c| c.start_time);
    frames
        .into_iter()
        .enumerate()
        .map(|(index, chapter)| Chapter {
            id: index as i64,
            start: f64::from(chapter.start_time) / 1000.0,
            end: (f64::from(chapter.end_time) / 1000.0).min(duration),
            title: chapter_title(chapter).unwrap_or_else(|| format!("Chapter {}", index + 1)),
        })
        .collect()
}

fn chapter_title(chapter: &id3::frame::Chapter) -> Option<String> {
    chapter
        .frames
        .iter()
        .find(|frame| frame.id() == "TIT2")
        .and_then(|frame| clean(frame.content().text()))
}

/// Adds custom Vorbis comments and returns the `CHAPTERxxx` chapters.
fn read_vorbis(path: &Path, duration: f64, tags: &mut AudioTags) -> Vec<Chapter> {
    let Some(comments) = vorbis_comments(path) else {
        return Vec::new();
    };
    let custom = |name: &str| clean(comments.get(name).map(String::as_str));
    tags.series = custom("SERIES");
    tags.series_part = custom("SERIES-PART");
    tags.isbn = custom("ISBN");
    tags.asin = custom("ASIN");
    let mut starts = Vec::new();
    for number in 0..1000 {
        let key = format!("CHAPTER{number:03}");
        let Some(start) = comments.get(&key).and_then(|t| parse_time(t)) else {
            if number == 0 {
                continue;
            }
            break;
        };
        let title = comments
            .get(&format!("{key}NAME"))
            .map_or_else(|| format!("Chapter {number}"), String::clone);
        starts.push((start, title));
    }
    from_starts(starts, duration)
}

/// All Vorbis comments of a file, by upper case key; the first of a repeated key wins.
fn vorbis_comments(path: &Path) -> Option<BTreeMap<String, String>> {
    let mut file = File::open(path).ok()?;
    let options = ParseOptions::new().read_cover_art(false);
    let mut comments = BTreeMap::new();
    let mut add = |key: &str, value: &str| {
        comments
            .entry(key.to_ascii_uppercase())
            .or_insert_with(|| value.to_owned());
    };
    match extension(path).as_str() {
        "flac" => {
            let flac = FlacFile::read_from(&mut file, options).ok()?;
            flac.vorbis_comments()?.items().for_each(|(k, v)| add(k, v));
        }
        "opus" => {
            let opus = OpusFile::read_from(&mut file, options).ok()?;
            opus.vorbis_comments().items().for_each(|(k, v)| add(k, v));
        }
        _ => {
            let vorbis = VorbisFile::read_from(&mut file, options).ok()?;
            vorbis
                .vorbis_comments()
                .items()
                .for_each(|(k, v)| add(k, v));
        }
    }
    Some(comments)
}

/// `HH:MM:SS.mmm` as seconds.
fn parse_time(text: &str) -> Option<f64> {
    let mut seconds = 0.0;
    for part in text.split(':') {
        seconds = seconds * 60.0 + part.parse::<f64>().ok()?;
    }
    Some(seconds)
}

/// Chapters from start times: each ends where the next starts, the last with the file.
fn from_starts(starts: Vec<(f64, String)>, duration: f64) -> Vec<Chapter> {
    let ends: Vec<f64> = starts
        .iter()
        .skip(1)
        .map(|(start, _)| *start)
        .chain([duration])
        .collect();
    starts
        .into_iter()
        .zip(ends)
        .enumerate()
        .map(|(index, ((start, title), end))| Chapter {
            id: index as i64,
            start,
            end,
            title,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vorbis_times_are_seconds() {
        assert_eq!(parse_time("01:02:03.500"), Some(3723.5));
        assert_eq!(parse_time("00:10.0"), Some(10.0));
        assert_eq!(parse_time("soon"), None);
    }

    #[test]
    fn a_chapter_ends_where_the_next_starts() {
        let chapters = from_starts(vec![(0.0, "A".into()), (5.0, "B".into())], 9.0);
        assert_eq!(chapters[0].end, 5.0);
        assert_eq!(chapters[1].end, 9.0);
        assert_eq!(chapters[1].id, 1);
        assert!(from_starts(Vec::new(), 9.0).is_empty());
    }

    #[test]
    fn a_file_that_is_no_audio_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.mp3");
        std::fs::write(&path, b"no audio here").unwrap();
        assert!(probe(&path).is_err());
    }
}
