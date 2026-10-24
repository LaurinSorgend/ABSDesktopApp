//! Decodes the captured server responses in `fixtures/api/<version>/`.

use std::path::PathBuf;

use abs_api::Raw;
use abs_api::models::*;
use serde::de::DeserializeOwned;

const VERSIONS: [&str; 2] = ["2.26.3", "2.37.2"];

fn fixture<T: DeserializeOwned>(version: &str, name: &str) -> T {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/api")
        .join(version)
        .join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path:?}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{version}/{name}: {e}"))
}

fn each_version(check: impl Fn(&str)) {
    VERSIONS.iter().for_each(|v| check(v));
}

#[test]
fn status_and_users() {
    each_version(|v| {
        let status: Status = fixture(v, "status");
        assert_eq!(status.server_version, v);
        let login: LoginResponse = fixture(v, "login");
        assert_eq!(login.user.username, "admin");
        assert!(login.user_default_library_id.is_some());
        let authorize: LoginResponse = fixture(v, "authorize");
        assert!(authorize.user.permissions.download);
        let me: User = fixture(v, "me");
        assert_eq!(me.user_type, "root");
    });
}

#[test]
fn libraries_and_items() {
    each_version(|v| {
        let libraries: Libraries = fixture(v, "libraries");
        assert_eq!(libraries.libraries.len(), 3);
        assert!(libraries.libraries[0].json().contains("\"settings\""));
        let items: Page<Raw<LibraryItem>> = fixture(v, "library_items");
        assert_eq!(items.total, 2);
        let first = &items.results[0].media.metadata;
        assert_eq!(first.author_name.as_deref(), Some("Oscar Wilde"));
        assert!(first.series_name.as_deref().unwrap().starts_with("Fixture series"));
    });
}

#[test]
fn expanded_items() {
    each_version(|v| {
        let multi: Raw<LibraryItem> = fixture(v, "item_multi_file");
        assert_eq!(multi.media.tracks.len(), 5);
        assert_eq!(multi.media.metadata.series[0].sequence.as_deref(), Some("2"));
        assert_eq!(multi.media.metadata.authors[0].name, "Oscar Wilde");
        let single: Raw<LibraryItem> = fixture(v, "item_single_file");
        assert_eq!(single.media.chapters.len(), 80);
        assert_eq!(single.media.metadata.asin.as_deref(), Some("B0FIXTURE1"));
    });
}

#[test]
fn browse_lists() {
    each_version(|v| {
        let series: Page<Series> = fixture(v, "library_series");
        assert_eq!(series.results[0].books.len(), 2);
        let authors: Authors = fixture(v, "library_authors");
        assert_eq!(authors.authors[0].name, "Oscar Wilde");
        let collections: Page<Collection> = fixture(v, "library_collections");
        assert_eq!(collections.results[0].books.len(), 2);
        let search: SearchResults = fixture(v, "library_search");
        assert!(!search.book.is_empty());
    });
}

#[test]
fn personalized_shelves() {
    each_version(|v| {
        let shelves: Vec<Shelf> = fixture(v, "library_personalized");
        let kinds: Vec<_> = shelves
            .iter()
            .map(|s| match &s.content {
                ShelfContent::Books(_) => "books",
                ShelfContent::Series(_) => "series",
                ShelfContent::Authors(_) => "authors",
                ShelfContent::Other { .. } => "other",
            })
            .collect();
        assert!(kinds.contains(&"books"), "{v}: {kinds:?}");
        assert!(kinds.contains(&"series"), "{v}: {kinds:?}");
        assert!(kinds.contains(&"authors"), "{v}: {kinds:?}");
    });
}

#[test]
fn playback_and_progress() {
    each_version(|v| {
        let session: PlaybackSession = fixture(v, "play");
        assert_eq!(session.audio_tracks.len(), 5);
        let second = &session.audio_tracks[1];
        assert!(second.start_offset > 1440.0);
        assert!(second.content_url.starts_with("/api/items/"));
        let progress: MediaProgress = fixture(v, "item_progress");
        assert_eq!(progress.current_time, Some(30.0));
        let bookmark: Bookmark = fixture(v, "bookmark_create");
        assert_eq!(bookmark.time, 42.0);
    });
    let list: ProgressList = fixture("2.37.2", "me_progress");
    assert!(!list.media_progress.is_empty());
}
