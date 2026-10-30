//! Starting the engine against a temporary folder and waiting for its events.
#![allow(dead_code, reason = "each test binary uses a different part")]

use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use abs_core::credentials::Credentials;
use abs_core::engine::{Engine, EngineConfig, Event};
use tokio::io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional};
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Runtime;
use tokio::sync::watch;

const WAIT: Duration = Duration::from_secs(30);
/// Time for clients to notice that the proxy cut their pooled connections.
const CONNECTIONS_CLOSE: Duration = Duration::from_millis(300);
const BAD_GATEWAY: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";

pub fn start(db_path: &std::path::Path, credentials: &Credentials) -> (Engine, Receiver<Event>) {
    let (sender, events) = channel();
    let config = EngineConfig {
        db_path: db_path.to_owned(),
        cover_root: db_path.with_file_name("covers"),
        download_root: db_path.with_file_name("downloads"),
        credentials: credentials.clone(),
    };
    let engine = Engine::start(config, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    (engine, events)
}

/// A TCP proxy in front of a server that can act like a reverse proxy whose backend is down.
pub struct FlakyProxy {
    pub url: String,
    broken: watch::Sender<bool>,
    _runtime: Runtime,
}

impl FlakyProxy {
    pub fn new(server: &str) -> Self {
        let target = server.trim_start_matches("http://").trim_end_matches('/');
        let target = target.to_owned();
        let runtime = Runtime::new().unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let (broken, watcher) = watch::channel(false);
        runtime.spawn(async move {
            while let Ok((client, _)) = listener.accept().await {
                tokio::spawn(serve(client, target.clone(), watcher.clone()));
            }
        });
        Self {
            url,
            broken,
            _runtime: runtime,
        }
    }

    /// While broken, every request is answered with 502 and open connections are cut.
    pub fn set_broken(&self, broken: bool) {
        self.broken.send_replace(broken);
        std::thread::sleep(CONNECTIONS_CLOSE);
    }
}

async fn serve(mut client: TcpStream, target: String, mut broken: watch::Receiver<bool>) {
    if *broken.borrow_and_update() {
        let mut request = [0; 4096];
        let _ = client.read(&mut request).await;
        let _ = client.write_all(BAD_GATEWAY).await;
        let _ = client.shutdown().await;
        return;
    }
    let Ok(mut server) = TcpStream::connect(&target).await else {
        return;
    };
    tokio::select! {
        _ = copy_bidirectional(&mut client, &mut server) => {}
        _ = broken.changed() => {}
    }
}

/// Waits for the first event `pick` accepts and returns what it extracted.
pub fn wait_for<T>(events: &Receiver<Event>, mut pick: impl FnMut(&Event) -> Option<T>) -> T {
    loop {
        let event = events.recv_timeout(WAIT).expect("engine event");
        if let Some(value) = pick(&event) {
            return value;
        }
        assert!(
            !matches!(
                event,
                Event::AddAccountFailed { .. }
                    | Event::SyncFailed { .. }
                    | Event::RequestFailed { .. }
            ),
            "{event:?}"
        );
    }
}
