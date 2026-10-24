use serde::Deserialize;

use crate::Raw;
use crate::models::{LibraryItem, MediaProgress};

/// What the realtime connection reports.
#[derive(Debug, Clone)]
pub enum RealtimeEvent {
    /// Socket authenticated (`init`). Sent again after every reconnect.
    Ready { user_id: String },
    /// The server rejected the token (`auth_failed`).
    AuthFailed(String),
    /// Connection lost; the client reconnects on its own.
    Disconnected(String),
    /// `user_item_progress_updated`: progress changed on any device of this user.
    ProgressUpdated {
        session_id: Option<String>,
        progress: MediaProgress,
    },
    /// `item_added`, `item_updated`, `items_added`, `items_updated`.
    ItemsChanged(Vec<Raw<LibraryItem>>),
    /// `item_removed`.
    ItemRemoved { item_id: String, library_id: String },
    /// `library_added`, `library_updated`, `library_removed`: re-read the library list.
    LibrariesChanged,
    /// `user_session_closed`.
    SessionClosed(String),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProgressPayload {
    session_id: Option<String>,
    data: MediaProgress,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitPayload {
    #[serde(default)]
    user_id: String,
}

#[derive(Deserialize)]
struct AuthFailedPayload {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RemovedItem {
    id: String,
    #[serde(default)]
    library_id: String,
}

impl RealtimeEvent {
    /// Maps a Socket.IO event. `None` for events the app does not use or cannot decode.
    pub fn from_socket(name: &str, data: &str) -> Option<Self> {
        let event = match name {
            "init" => Self::Ready {
                user_id: decode::<InitPayload>(data)?.user_id,
            },
            "auth_failed" => Self::AuthFailed(decode::<AuthFailedPayload>(data)?.message),
            "user_item_progress_updated" => {
                let payload: ProgressPayload = decode(data)?;
                Self::ProgressUpdated {
                    session_id: payload.session_id,
                    progress: payload.data,
                }
            }
            "item_added" | "item_updated" => Self::ItemsChanged(vec![decode(data)?]),
            "items_added" | "items_updated" => Self::ItemsChanged(decode(data)?),
            "item_removed" => {
                let item: RemovedItem = decode(data)?;
                Self::ItemRemoved {
                    item_id: item.id,
                    library_id: item.library_id,
                }
            }
            "library_added" | "library_updated" | "library_removed" => Self::LibrariesChanged,
            "user_session_closed" => Self::SessionClosed(decode(data)?),
            _ => return None,
        };
        Some(event)
    }
}

fn decode<T: serde::de::DeserializeOwned>(data: &str) -> Option<T> {
    serde_json::from_str(data).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_progress_and_items() {
        let data = r#"{"id":"p","sessionId":"s","data":{"libraryItemId":"i","currentTime":12.5,"lastUpdate":5}}"#;
        let Some(RealtimeEvent::ProgressUpdated {
            session_id,
            progress,
        }) = RealtimeEvent::from_socket("user_item_progress_updated", data)
        else {
            panic!("no progress event");
        };
        assert_eq!(session_id.as_deref(), Some("s"));
        assert_eq!(progress.current_time, Some(12.5));

        let items = RealtimeEvent::from_socket("items_added", r#"[{"id":"a"},{"id":"b"}]"#);
        assert!(matches!(items, Some(RealtimeEvent::ItemsChanged(v)) if v.len() == 2));
        let removed = RealtimeEvent::from_socket("item_removed", r#"{"id":"a","libraryId":"l"}"#);
        assert!(
            matches!(removed, Some(RealtimeEvent::ItemRemoved { item_id, .. }) if item_id == "a")
        );
        assert!(RealtimeEvent::from_socket("rss_feed_open", "{}").is_none());
    }
}
