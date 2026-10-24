//! Global item time versus (track, offset), and chapter navigation.

/// One audio file of an item. `start_offset` is its start in global item time.
#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub url: String,
    pub start_offset: f64,
    pub duration: f64,
}

/// A chapter in global item time, from the server JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub start: f64,
    pub end: f64,
    pub title: String,
}

/// Seconds into a chapter after which "previous" restarts it instead of going back.
pub const RESTART_THRESHOLD: f64 = 3.0;

/// Total length of all tracks.
pub fn total_duration(tracks: &[Track]) -> f64 {
    tracks.last().map_or(0.0, |t| t.start_offset + t.duration)
}

/// The track holding `global` and the offset inside it. Clamps to the item.
pub fn locate(tracks: &[Track], global: f64) -> (usize, f64) {
    let global = global.max(0.0);
    let index = tracks
        .iter()
        .rposition(|t| t.start_offset <= global)
        .unwrap_or(0);
    let Some(track) = tracks.get(index) else {
        return (0, 0.0);
    };
    (
        index,
        (global - track.start_offset).clamp(0.0, track.duration),
    )
}

/// Global time of an offset inside a track.
pub fn global_time(tracks: &[Track], index: usize, offset: f64) -> f64 {
    tracks.get(index).map_or(0.0, |t| t.start_offset) + offset
}

/// Index of the chapter at `position`.
pub fn chapter_at(chapters: &[Chapter], position: f64) -> Option<usize> {
    chapters
        .iter()
        .rposition(|c| c.start <= position + f64::EPSILON)
}

/// Start of the next chapter, or `None` in the last one.
pub fn next_chapter_start(chapters: &[Chapter], position: f64) -> Option<f64> {
    let next = chapter_at(chapters, position).map_or(0, |i| i + 1);
    chapters.get(next).map(|c| c.start)
}

/// Where "previous chapter" goes: the start of this chapter if more than
/// [`RESTART_THRESHOLD`] seconds in, else the start of the one before.
pub fn previous_chapter_start(chapters: &[Chapter], position: f64) -> Option<f64> {
    let current = chapter_at(chapters, position)?;
    let chapter = &chapters[current];
    if position - chapter.start > RESTART_THRESHOLD || current == 0 {
        return Some(chapter.start);
    }
    Some(chapters[current - 1].start)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracks() -> Vec<Track> {
        [(0.0, 100.0), (100.0, 50.0), (150.0, 25.0)]
            .iter()
            .map(|&(start_offset, duration)| Track {
                url: String::new(),
                start_offset,
                duration,
            })
            .collect()
    }

    fn chapters() -> Vec<Chapter> {
        [(0.0, 60.0), (60.0, 130.0), (130.0, 175.0)]
            .iter()
            .map(|&(start, end)| Chapter {
                start,
                end,
                title: String::new(),
            })
            .collect()
    }

    #[test]
    fn maps_global_time_to_tracks() {
        let t = tracks();
        assert_eq!(total_duration(&t), 175.0);
        assert_eq!(locate(&t, 0.0), (0, 0.0));
        assert_eq!(locate(&t, 120.0), (1, 20.0));
        assert_eq!(locate(&t, 150.0), (2, 0.0));
        assert_eq!(locate(&t, 999.0), (2, 25.0));
        assert_eq!(locate(&t, -5.0), (0, 0.0));
        assert_eq!(global_time(&t, 1, 20.0), 120.0);
        assert_eq!(locate(&[], 10.0), (0, 0.0));
    }

    #[test]
    fn chapter_navigation() {
        let c = chapters();
        assert_eq!(chapter_at(&c, 59.0), Some(0));
        assert_eq!(chapter_at(&c, 60.0), Some(1));
        assert_eq!(next_chapter_start(&c, 10.0), Some(60.0));
        assert_eq!(next_chapter_start(&c, 140.0), None);
        assert_eq!(previous_chapter_start(&c, 70.0), Some(60.0));
        assert_eq!(previous_chapter_start(&c, 61.0), Some(0.0));
        assert_eq!(previous_chapter_start(&c, 1.0), Some(0.0));
        assert_eq!(previous_chapter_start(&[], 1.0), None);
    }
}
