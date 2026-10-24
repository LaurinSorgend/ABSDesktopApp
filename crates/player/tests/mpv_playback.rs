//! libmpv against the Docker server, silent output.
//! Run with `cargo test -p abs-player --features integration`.
#![cfg(feature = "integration")]

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use abs_api::models::{DeviceInfo, PlayRequest};
use abs_api::{AbsClient, ItemsQuery, http_client};
use abs_player::timeline::{Track, total_duration};
use abs_player::{Media, MpvConfig, MpvPlayer, Player, PlayerEvent};

const WAIT: Duration = Duration::from_secs(30);

/// Tracks and auth header of a fresh session on the item whose title starts with `prefix`.
/// Each test logs in as a different user, since starting a session closes the user's other one.
async fn media(prefix: &str, user: &str) -> Media {
    let server =
        std::env::var("ABS_TEST_SERVER").unwrap_or_else(|_| "http://localhost:13379".into());
    let client = AbsClient::connect(http_client().unwrap(), &server)
        .await
        .unwrap();
    client.login(user, &format!("{user}-pass")).await.unwrap();
    let libraries = client.libraries().await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Audiobooks").unwrap();
    let items = client
        .library_items(&library.id, &ItemsQuery::default())
        .await
        .unwrap();
    let item = items
        .results
        .iter()
        .find(|i| {
            i.media
                .metadata
                .title
                .as_deref()
                .unwrap_or_default()
                .starts_with(prefix)
        })
        .unwrap();
    let session = client.play(&item.id, &play_request()).await.unwrap();
    client.close_session(&session.id, None).await.unwrap();
    let tracks = session
        .audio_tracks
        .iter()
        .map(|t| Track {
            url: client.resolve(&t.content_url).unwrap().to_string(),
            start_offset: t.start_offset,
            duration: t.duration,
        })
        .collect();
    let header = format!("Authorization: Bearer {}", client.access_token().unwrap());
    Media {
        tracks,
        http_headers: vec![header],
    }
}

fn player() -> (MpvPlayer, Receiver<PlayerEvent>) {
    let (sender, events) = channel();
    let config = MpvConfig {
        audio_output: Some("null".into()),
        ..MpvConfig::default()
    };
    let player = MpvPlayer::new(&config, move |e| {
        let _ = sender.send(e);
    })
    .unwrap();
    (player, events)
}

/// Waits for a position event within two seconds of `target`.
fn wait_position(events: &Receiver<PlayerEvent>, target: f64) -> f64 {
    loop {
        match events.recv_timeout(WAIT).expect("player event") {
            PlayerEvent::Position(p) if (p - target).abs() < 2.0 => return p,
            PlayerEvent::Error(e) => panic!("player error: {e}"),
            _ => {}
        }
    }
}

fn wait_ended(events: &Receiver<PlayerEvent>) {
    loop {
        match events.recv_timeout(WAIT).expect("player event") {
            PlayerEvent::Ended => return,
            PlayerEvent::Error(e) => panic!("player error: {e}"),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn multi_file_book_maps_global_time() {
    let media = media("The Happy Prince and", "listener").await;
    let total = total_duration(&media.tracks);
    let (player, events) = player();
    player.load(media, 1500.0, true).unwrap();
    wait_position(&events, 1500.0);
    player.seek(3100.0).unwrap();
    wait_position(&events, 3100.0);
    player.seek(3200.0).unwrap();
    wait_position(&events, 3200.0);
    player.set_speed(2.0).unwrap();
    player.seek(total - 1.0).unwrap();
    wait_ended(&events);
}

#[tokio::test(flavor = "multi_thread")]
async fn long_single_file_starts_deep_inside() {
    let media = media("The Happy Prince Marathon", "admin").await;
    let (player, events) = player();
    player.load(media, 72_000.0, true).unwrap();
    wait_position(&events, 72_000.0);
    player.pause().unwrap();
    assert!(player.is_paused() || events.recv_timeout(WAIT).is_ok());
    assert!((player.position() - 72_000.0).abs() < 3.0);
}

fn play_request() -> PlayRequest {
    PlayRequest {
        device_info: DeviceInfo {
            device_id: "player-tests".into(),
            client_name: "contract-tests".into(),
            client_version: "0".into(),
            manufacturer: None,
            model: None,
        },
        media_player: "libmpv".into(),
        supported_mime_types: vec!["audio/mpeg".into(), "audio/mp4".into()],
        force_direct_play: true,
        force_transcode: false,
    }
}
