//! Endpoint contract tests. Run with `cargo test -p abs-api --features integration`.
#![cfg(feature = "integration")]

mod common;

use abs_api::models::{DeviceInfo, PlayRequest, SessionSync, ShelfContent};
use abs_api::{AbsClient, Error, Feature, ItemFilter, ItemsQuery};
use reqwest::{StatusCode, header};

async fn audiobooks_id(client: &AbsClient) -> String {
    let libraries = client.libraries().await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Audiobooks").unwrap();
    library.id.clone()
}

async fn item_titled(client: &AbsClient, library_id: &str, prefix: &str) -> String {
    let page = client
        .library_items(library_id, &ItemsQuery::default())
        .await
        .unwrap();
    let item = page.results.iter().find(|i| {
        let title = i.media.metadata.title.as_deref().unwrap_or_default();
        title.starts_with(prefix)
    });
    item.unwrap().id.clone()
}

#[tokio::test]
async fn library_items_page_sort_and_filter() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        let library = audiobooks_id(&client).await;
        let query = ItemsQuery {
            limit: 1,
            page: 1,
            sort: Some("media.metadata.title".into()),
            ..ItemsQuery::default()
        };
        let page = client.library_items(&library, &query).await.unwrap();
        assert_eq!((page.total, page.results.len()), (2, 1), "{server}");
        assert!(page.results[0].json().starts_with('{'));
        let authors = client.library_authors(&library).await.unwrap().authors;
        let filter = ItemFilter::new("authors", authors[0].id.clone());
        let query = ItemsQuery {
            filter: Some(filter),
            ..ItemsQuery::default()
        };
        let filtered = client.library_items(&library, &query).await.unwrap();
        assert_eq!(filtered.total, 2, "{server}");
    }
}

#[tokio::test]
async fn browse_endpoints() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        let library = audiobooks_id(&client).await;
        let series = client.library_series(&library, 10, 0).await.unwrap();
        assert_eq!(series.results[0].books.len(), 2, "{server}");
        let collections = client.library_collections(&library, 10, 0).await.unwrap();
        assert_eq!(collections.results[0].name, "Fixture favourites");
        let found = client.search(&library, "marathon", 5).await.unwrap();
        assert_eq!(found.book.len(), 1, "{server}");
        let shelves = client.personalized(&library, 10).await.unwrap();
        let has_books = shelves
            .iter()
            .any(|s| matches!(&s.content, ShelfContent::Books(b) if !b.is_empty()));
        assert!(has_books, "{server}");
    }
}

#[tokio::test]
async fn expanded_item_and_cover() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        let library = audiobooks_id(&client).await;
        let id = item_titled(&client, &library, "The Happy Prince Marathon").await;
        let item = client.item(&id).await.unwrap();
        assert_eq!(item.media.chapters.len(), 80, "{server}");
        let cover = client
            .cover(&id, 200)
            .await
            .unwrap()
            .expect("item has a cover");
        assert_eq!(&cover[..4], b"RIFF", "{server}: cover is WebP");
    }
}

#[tokio::test]
async fn playback_session_round_trip() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        let library = audiobooks_id(&client).await;
        let id = item_titled(&client, &library, "The Happy Prince and").await;
        let session = client.play(&id, &play_request()).await.unwrap();
        assert_eq!(session.audio_tracks.len(), 5, "{server}");
        check_track_range(&client, &session.audio_tracks[1].content_url).await;
        let sync = SessionSync {
            current_time: 1500.0,
            time_listened: 5.0,
            duration: session.duration,
        };
        client.sync_session(&session.id, &sync).await.unwrap();
        let progress = client.item_progress(&id).await.unwrap().unwrap();
        assert_eq!(progress.current_time, Some(1500.0), "{server}");
        client
            .close_session(&session.id, Some(&sync))
            .await
            .unwrap();
        let err = client.sync_session(&session.id, &sync).await.unwrap_err();
        assert_eq!(err.status(), Some(StatusCode::NOT_FOUND), "{server}: {err}");
    }
}

#[tokio::test]
async fn bookmarks_create_update_delete() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        let library = audiobooks_id(&client).await;
        let id = item_titled(&client, &library, "The Happy Prince Marathon").await;
        let created = client.create_bookmark(&id, 77.5, "Contract").await.unwrap();
        assert_eq!(created.time, 77.5);
        let updated = client.update_bookmark(&id, 77.5, "Renamed").await.unwrap();
        assert_eq!(updated.title, "Renamed");
        let me = client.me().await.unwrap();
        assert!(me.bookmarks.iter().any(|b| b.time == 77.5), "{server}");
        client.delete_bookmark(&id, 77.5).await.unwrap();
        let me = client.me().await.unwrap();
        assert!(!me.bookmarks.iter().any(|b| b.time == 77.5), "{server}");
    }
}

#[tokio::test]
async fn all_progress_is_version_gated() {
    for server in common::servers() {
        let (client, _) = common::listener(&server).await;
        match client.all_progress().await {
            Ok(_) => assert!(client.supports(Feature::MeProgress), "{server}"),
            Err(Error::Unsupported(Feature::MeProgress)) => {
                let raw = client.url("api/me/progress").unwrap();
                let status = reqwest::Client::new()
                    .get(raw)
                    .bearer_auth(client.access_token().unwrap())
                    .send()
                    .await
                    .unwrap()
                    .status();
                assert_eq!(status, StatusCode::NOT_FOUND, "{server}: gate too strict");
            }
            Err(e) => panic!("{server}: {e}"),
        }
    }
}

fn play_request() -> PlayRequest {
    PlayRequest {
        device_info: DeviceInfo {
            device_id: "contract-tests".into(),
            client_name: "abs-desktop".into(),
            client_version: env!("CARGO_PKG_VERSION").into(),
            manufacturer: None,
            model: None,
        },
        media_player: "libmpv".into(),
        supported_mime_types: vec!["audio/mpeg".into(), "audio/mp4".into()],
        force_direct_play: true,
        force_transcode: false,
    }
}

async fn check_track_range(client: &AbsClient, content_url: &str) {
    let response = reqwest::Client::new()
        .get(client.resolve(content_url).unwrap())
        .bearer_auth(client.access_token().unwrap())
        .header(header::RANGE, "bytes=0-1023")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.bytes().await.unwrap().len(), 1024);
}
