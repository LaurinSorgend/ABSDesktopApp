use anyhow::{Context, Result};
use reqwest::blocking::Client;
use serde_json::{Value, json};

const SUPPORTED_MIME_TYPES: &[&str] = &[
    "audio/mpeg",
    "audio/mp4",
    "audio/x-m4b",
    "audio/aac",
    "audio/flac",
    "audio/ogg",
    "audio/opus",
    "audio/webm",
    "audio/wav",
];

/// One audio track of a playback session, with its place on the book's timeline.
pub struct Track {
    pub url: String,
    pub start_offset: f64,
    pub duration: f64,
}

/// A started ABS playback session.
pub struct Session {
    pub id: String,
    pub tracks: Vec<Track>,
}

/// Logged-in connection to the test server.
pub struct Server {
    client: Client,
    pub base: String,
    pub token: String,
}

impl Server {
    /// Logs in with username and password.
    pub fn login(base: &str, username: &str, password: &str) -> Result<Self> {
        let client = Client::new();
        let body: Value = client
            .post(format!("{base}/login"))
            .header("x-return-tokens", "true")
            .json(&json!({ "username": username, "password": password }))
            .send()?
            .error_for_status()?
            .json()?;
        let token = body["user"]["accessToken"].as_str().context("no access token")?.to_owned();
        Ok(Self { client, base: base.to_owned(), token })
    }

    /// Finds an item by exact title in any library.
    pub fn find_item(&self, title: &str) -> Result<String> {
        let libraries = self.get("/api/libraries")?;
        for library in libraries["libraries"].as_array().into_iter().flatten() {
            let id = library["id"].as_str().unwrap_or_default();
            let items = self.get(&format!("/api/libraries/{id}/items?minified=1&limit=200"))?;
            let found = items["results"].as_array().into_iter().flatten().find(|i| i["media"]["metadata"]["title"] == title);
            if let Some(item) = found {
                return Ok(item["id"].as_str().unwrap_or_default().to_owned());
            }
        }
        anyhow::bail!("item {title} not found")
    }

    /// Starts a direct-play session the way the desktop app will.
    pub fn start_session(&self, item_id: &str) -> Result<Session> {
        let body = json!({
            "deviceInfo": { "deviceId": "mpv-spike", "clientName": "ABS Desktop spike", "clientVersion": "0.0.0" },
            "mediaPlayer": "libmpv",
            "forceDirectPlay": false,
            "supportedMimeTypes": SUPPORTED_MIME_TYPES,
        });
        let session: Value =
            self.client.post(format!("{}/api/items/{item_id}/play", self.base)).bearer_auth(&self.token).json(&body).send()?.error_for_status()?.json()?;
        let tracks = session["audioTracks"].as_array().context("no audioTracks")?.iter().map(|t| self.track(t)).collect();
        Ok(Session { id: session["id"].as_str().unwrap_or_default().to_owned(), tracks })
    }

    /// Closes a session so the server doesn't keep it open.
    pub fn close_session(&self, id: &str) -> Result<()> {
        self.client.post(format!("{}/api/session/{id}/close", self.base)).bearer_auth(&self.token).send()?.error_for_status()?;
        Ok(())
    }

    fn track(&self, track: &Value) -> Track {
        Track {
            url: format!("{}{}", self.base, track["contentUrl"].as_str().unwrap_or_default()),
            start_offset: track["startOffset"].as_f64().unwrap_or_default(),
            duration: track["duration"].as_f64().unwrap_or_default(),
        }
    }

    fn get(&self, path: &str) -> Result<Value> {
        Ok(self.client.get(format!("{}{path}", self.base)).bearer_auth(&self.token).send()?.error_for_status()?.json()?)
    }
}

/// Maps a position on the book timeline to `(track index, offset in track)`.
pub fn locate(tracks: &[Track], global: f64) -> (usize, f64) {
    let index = tracks.iter().rposition(|t| t.start_offset <= global).unwrap_or(0);
    (index, global - tracks[index].start_offset)
}
