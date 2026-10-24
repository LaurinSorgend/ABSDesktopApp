//! One WebSocket session: handshake, auth, then events until the connection drops.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

use super::events::RealtimeEvent;
use super::packet::{self, Packet};
use crate::AbsClient;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

type Socket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Why a session ended without a transport error.
#[derive(Debug, PartialEq, Eq)]
pub enum Ended {
    AuthFailed,
    ReceiverGone,
}

/// Runs one connection. `ready` is set once the server accepted the token.
pub async fn session(
    client: &AbsClient,
    events: &mpsc::Sender<RealtimeEvent>,
    ready: &mut bool,
) -> Result<Ended, String> {
    let url = socket_url(client.base_url())?;
    let (mut socket, _) = timeout(
        HANDSHAKE_TIMEOUT,
        tokio_tungstenite::connect_async(url.as_str()),
    )
    .await
    .map_err(|_| "connect timed out".to_owned())?
    .map_err(|e| e.to_string())?;
    let idle = handshake(&mut socket).await?;
    let mut token_changes = client.token_changes();
    let token = client.access_token().ok_or("logged out")?;
    send(&mut socket, packet::event("auth", &token)).await?;
    loop {
        tokio::select! {
            frame = timeout(idle, socket.next()) => {
                let text = read_text(frame)?;
                if let Some(ended) = on_frame(&mut socket, &text, events, ready).await? {
                    return Ok(ended);
                }
            }
            changed = token_changes.changed() => {
                changed.map_err(|_| "client dropped".to_owned())?;
                let token = client.access_token().ok_or("logged out")?;
                send(&mut socket, packet::event("auth", &token)).await?;
            }
        }
    }
}

/// Waits for the Engine.IO open packet and the namespace connect. Returns the idle limit.
async fn handshake(socket: &mut Socket) -> Result<Duration, String> {
    let mut idle = None;
    loop {
        let frame = timeout(HANDSHAKE_TIMEOUT, socket.next()).await;
        match Packet::parse(&read_text(frame)?) {
            Packet::Open {
                ping_interval,
                ping_timeout,
            } => {
                idle = Some(Duration::from_millis(ping_interval + ping_timeout));
                send(socket, packet::CONNECT.to_owned()).await?;
            }
            Packet::Connect => return idle.ok_or_else(|| "connect before open".to_owned()),
            Packet::ConnectError(message) => return Err(format!("namespace refused: {message}")),
            Packet::Ping => send(socket, packet::PONG.to_owned()).await?,
            _ => {}
        }
    }
}

async fn on_frame(
    socket: &mut Socket,
    text: &str,
    events: &mpsc::Sender<RealtimeEvent>,
    ready: &mut bool,
) -> Result<Option<Ended>, String> {
    match Packet::parse(text) {
        Packet::Ping => send(socket, packet::PONG.to_owned()).await?,
        Packet::Close | Packet::Disconnect => return Err("server closed the socket".into()),
        Packet::Event { name, data } => {
            let Some(event) = RealtimeEvent::from_socket(&name, &data) else {
                return Ok(None);
            };
            let auth_failed = matches!(event, RealtimeEvent::AuthFailed(_));
            *ready |= matches!(event, RealtimeEvent::Ready { .. });
            if events.send(event).await.is_err() {
                return Ok(Some(Ended::ReceiverGone));
            }
            if auth_failed {
                return Ok(Some(Ended::AuthFailed));
            }
        }
        _ => {}
    }
    Ok(None)
}

type Frame = Result<
    Option<Result<Message, tokio_tungstenite::tungstenite::Error>>,
    tokio::time::error::Elapsed,
>;

/// The text of the next frame; ping timeouts, errors and close frames end the session.
fn read_text(frame: Frame) -> Result<String, String> {
    match frame {
        Err(_) => Err("no ping from server".into()),
        Ok(None) => Err("connection closed".into()),
        Ok(Some(Err(e))) => Err(e.to_string()),
        Ok(Some(Ok(Message::Text(text)))) => Ok(text.as_str().to_owned()),
        Ok(Some(Ok(Message::Close(_)))) => Err("connection closed".into()),
        Ok(Some(Ok(_))) => Ok(String::new()),
    }
}

async fn send(socket: &mut Socket, text: String) -> Result<(), String> {
    socket
        .send(Message::text(text))
        .await
        .map_err(|e| e.to_string())
}

/// `ws(s)://host/<base path>/socket.io/?EIO=4&transport=websocket`.
pub fn socket_url(base: &Url) -> Result<Url, String> {
    let mut url = base.join("socket.io/").map_err(|e| e.to_string())?;
    let scheme = if base.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    url.set_scheme(scheme)
        .map_err(|()| "cannot switch to a WebSocket URL".to_owned())?;
    url.set_query(Some("EIO=4&transport=websocket"));
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_socket_urls() {
        let base = Url::parse("https://host.example/abs/").unwrap();
        assert_eq!(
            socket_url(&base).unwrap().as_str(),
            "wss://host.example/abs/socket.io/?EIO=4&transport=websocket"
        );
        let base = Url::parse("http://localhost:13378/").unwrap();
        assert!(
            socket_url(&base)
                .unwrap()
                .as_str()
                .starts_with("ws://localhost:13378/socket.io/")
        );
    }
}
