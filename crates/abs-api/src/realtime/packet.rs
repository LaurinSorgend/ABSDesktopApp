//! Engine.IO v4 / Socket.IO v5 text packets, default namespace, no binary attachments.

use serde::Deserialize;
use serde_json::value::RawValue;

/// A decoded text frame.
#[derive(Debug, Clone, PartialEq)]
pub enum Packet {
    /// Engine.IO handshake; times in milliseconds.
    Open {
        ping_interval: u64,
        ping_timeout: u64,
    },
    Close,
    Ping,
    Pong,
    /// Socket.IO namespace connect acknowledged.
    Connect,
    ConnectError(String),
    Disconnect,
    /// `42["name", data]`; `data` is the raw JSON text, `null` when missing.
    Event {
        name: String,
        data: String,
    },
    /// Anything this client does not handle.
    Unknown(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenPayload {
    ping_interval: u64,
    ping_timeout: u64,
}

impl Packet {
    pub fn parse(frame: &str) -> Self {
        let unknown = || Self::Unknown(frame.to_owned());
        let mut chars = frame.chars();
        match chars.next() {
            Some('0') => serde_json::from_str::<OpenPayload>(chars.as_str())
                .map(|o| Self::Open {
                    ping_interval: o.ping_interval,
                    ping_timeout: o.ping_timeout,
                })
                .unwrap_or_else(|_| unknown()),
            Some('1') => Self::Close,
            Some('2') => Self::Ping,
            Some('3') => Self::Pong,
            Some('4') => parse_socket(chars.as_str()).unwrap_or_else(unknown),
            _ => unknown(),
        }
    }
}

fn parse_socket(body: &str) -> Option<Packet> {
    let mut chars = body.chars();
    let kind = chars.next()?;
    let rest = chars.as_str();
    match kind {
        '0' => Some(Packet::Connect),
        '1' => Some(Packet::Disconnect),
        '2' => parse_event(skip_ack_id(rest)),
        '4' => Some(Packet::ConnectError(rest.to_owned())),
        _ => None,
    }
}

fn skip_ack_id(rest: &str) -> &str {
    rest.trim_start_matches(|c: char| c.is_ascii_digit())
}

fn parse_event(body: &str) -> Option<Packet> {
    let parts: Vec<&RawValue> = serde_json::from_str(body).ok()?;
    let name: String = serde_json::from_str(parts.first()?.get()).ok()?;
    let data = parts.get(1).map_or("null", |d| d.get()).to_owned();
    Some(Packet::Event { name, data })
}

/// Frame that connects to the default namespace.
pub const CONNECT: &str = "40";
/// Reply to a server ping.
pub const PONG: &str = "3";

/// Frame for `socket.emit(name, data)`.
pub fn event(name: &str, data: &impl serde::Serialize) -> String {
    let body = serde_json::json!([name, data]);
    format!("42{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_handshake_and_control() {
        let open = r#"0{"sid":"x","upgrades":[],"pingInterval":25000,"pingTimeout":20000,"maxPayload":1000000}"#;
        assert_eq!(
            Packet::parse(open),
            Packet::Open {
                ping_interval: 25000,
                ping_timeout: 20000
            }
        );
        assert_eq!(Packet::parse("2"), Packet::Ping);
        assert_eq!(Packet::parse(r#"40{"sid":"abc"}"#), Packet::Connect);
        assert_eq!(Packet::parse("41"), Packet::Disconnect);
        assert!(matches!(Packet::parse("6"), Packet::Unknown(_)));
    }

    #[test]
    fn parses_events_with_raw_data() {
        let frame = r#"42["user_item_progress_updated",{"id":"p","data":{"currentTime":3}}]"#;
        let Packet::Event { name, data } = Packet::parse(frame) else {
            panic!("not an event");
        };
        assert_eq!(name, "user_item_progress_updated");
        assert_eq!(data, r#"{"id":"p","data":{"currentTime":3}}"#);
        assert_eq!(
            Packet::parse(r#"4212["pong"]"#),
            Packet::Event {
                name: "pong".into(),
                data: "null".into()
            }
        );
    }

    #[test]
    fn encodes_events() {
        assert_eq!(event("auth", &"tok"), r#"42["auth","tok"]"#);
    }
}
