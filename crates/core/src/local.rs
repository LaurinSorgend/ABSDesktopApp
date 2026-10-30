//! Books in folders on this computer, recognised by the rules of the ABS scanner.

mod group;
mod metadata;
mod names;
mod order;
mod probe;

pub use group::{BookGroup, FileKind, group_files, kind_of};
pub use metadata::{
    BookMetadata, OpfMetadata, SeriesEntry, epub_cover, parse_opf, read_epub, read_text_file,
};
pub use names::{FolderData, folder_data, parse_names, title_ignore_prefix};
pub use order::{AudioFacts, chapters, smart_order};
pub use probe::{AudioProbe, AudioTags, album_of, embedded_cover, probe};

#[cfg(all(test, feature = "integration"))]
mod fixture_tests {
    use std::path::{Path, PathBuf};

    use super::*;

    fn media(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/media")
            .join(rel)
    }

    #[test]
    fn reads_an_m4b_with_chapters() {
        let found = probe(&media(
            "audiobooks/Oscar Wilde/The Happy Prince Marathon/The Happy Prince Marathon.m4b",
        ))
        .unwrap();
        assert!(
            (found.duration - 40.0 * 3600.0).abs() < 60.0,
            "{}",
            found.duration
        );
        assert_eq!(found.chapters.len(), 80);
        assert_eq!(found.chapters[1].title, "Chapter 2");
        assert!((found.chapters[1].start - 1800.0).abs() < 1.0);
        assert!((found.chapters[1].end - 3600.0).abs() < 1.0);
    }

    #[test]
    fn reads_an_mp3() {
        let found = probe(&media(
            "audiobooks/Oscar Wilde/The Happy Prince and Other Tales/thehappyprinceandothertales_01_wilde_64kb.mp3",
        ))
        .unwrap();
        assert!(found.duration > 60.0, "{}", found.duration);
    }

    #[test]
    fn reads_an_epub() {
        let path = media("ebooks/Lewis Carroll/Alice EPUB/Alice's Adventures in Wonderland.epub");
        let opf = read_epub(&path).unwrap();
        assert_eq!(
            opf.title.as_deref(),
            Some("Alice's Adventures in Wonderland")
        );
        assert_eq!(opf.authors, ["Lewis Carroll"]);
        assert!(epub_cover(&path).is_some_and(|cover| cover.len() > 1000));
    }
}
