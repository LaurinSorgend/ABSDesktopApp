//! Realtime contract tests. Run with `cargo test -p abs-api --features integration`.
#![cfg(feature = "integration")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use abs_api::models::{DeviceInfo, PlayRequest, SessionSync};
use abs_api::{ItemsQuery, Realtime, RealtimeEvent};

const WAIT: Duration = Duration::from_secs(20);

async fn next_matching(
    realtime: &mut Realtime,
    mut pick: impl FnMut(&RealtimeEvent) -> bool,
) -> RealtimeEvent {
    tokio::time::timeout(WAIT, async {
        loop {
            let event = realtime.next().await.expect("realtime stopped");
            if pick(&event) {
                return event;
            }
        }
    })
    .await
    .expect("no matching realtime event")
}

#[tokio::test]
async fn progress_from_another_device_arrives() {
    for server in common::servers() {
        let (listener, _) = common::listener(&server).await;
        let listener = Arc::new(listener);
        let mut realtime = Realtime::spawn(listener.clone());
        let ready =
            next_matching(&mut realtime, |e| matches!(e, RealtimeEvent::Ready { .. })).await;
        let RealtimeEvent::Ready { user_id } = ready else {
            unreachable!()
        };
        assert_eq!(user_id, listener.me().await.unwrap().id, "{server}");

        let (phone, _) = common::listener(&server).await;
        let item_id = some_item(&phone).await;
        let session = phone.play(&item_id, &play_request()).await.unwrap();
        let sync = SessionSync {
            current_time: 321.0,
            time_listened: 3.0,
            duration: session.duration,
        };
        phone.sync_session(&session.id, &sync).await.unwrap();
        let event = next_matching(&mut realtime, |e| {
            matches!(e, RealtimeEvent::ProgressUpdated { progress, .. } if progress.library_item_id == item_id)
        })
        .await;
        let RealtimeEvent::ProgressUpdated { progress, .. } = event else {
            unreachable!()
        };
        assert_eq!(progress.current_time, Some(321.0), "{server}");
        phone.close_session(&session.id, None).await.unwrap();
    }
}

#[tokio::test]
async fn reauth_after_refresh_keeps_socket() {
    for server in common::servers() {
        let (client, _) = common::client(&server).await;
        client.login("leaver", "leaver-pass").await.unwrap();
        let client = Arc::new(client);
        let mut realtime = Realtime::spawn(client.clone());
        next_matching(&mut realtime, |e| matches!(e, RealtimeEvent::Ready { .. })).await;
        client.refresh_now().await.unwrap();
        let again = next_matching(&mut realtime, |e| {
            matches!(
                e,
                RealtimeEvent::Ready { .. } | RealtimeEvent::Disconnected(_)
            )
        })
        .await;
        assert!(
            matches!(again, RealtimeEvent::Ready { .. }),
            "{server}: {again:?}"
        );
    }
}

async fn some_item(client: &abs_api::AbsClient) -> String {
    let libraries = client.libraries().await.unwrap();
    let library = libraries.iter().find(|l| l.name == "Audiobooks").unwrap();
    let page = client
        .library_items(&library.id, &ItemsQuery::default())
        .await
        .unwrap();
    page.results[0].id.clone()
}

fn play_request() -> PlayRequest {
    PlayRequest {
        device_info: DeviceInfo {
            device_id: "contract-phone".into(),
            client_name: "contract-tests".into(),
            client_version: "0".into(),
            manufacturer: None,
            model: None,
        },
        media_player: "test".into(),
        supported_mime_types: vec!["audio/mpeg".into(), "audio/mp4".into()],
        force_direct_play: true,
        force_transcode: false,
    }
}
