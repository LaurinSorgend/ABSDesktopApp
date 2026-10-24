use reqwest::header::RANGE;
use reqwest::{Response, StatusCode};

use crate::Raw;
use crate::client::AbsClient;
use crate::error::Result;
use crate::models::{LibraryItem, PlayRequest, PlaybackSession, SessionSync};

impl AbsClient {
    /// `GET /api/items/:id?expanded=1`.
    pub async fn item(&self, item_id: &str) -> Result<Raw<LibraryItem>> {
        let url = self.url(&format!("api/items/{item_id}"))?;
        self.send_json(self.http().get(url).query(&[("expanded", "1")]))
            .await
    }

    /// `GET /api/items/:id/cover` as WebP, `width` pixels wide. `None` when the item has no cover.
    pub async fn cover(&self, item_id: &str, width: u32) -> Result<Option<Vec<u8>>> {
        let url = self.url(&format!("api/items/{item_id}/cover"))?;
        let query = [("width", width.to_string()), ("format", "webp".to_owned())];
        match self.send(self.http().get(url).query(&query)).await {
            Ok(response) => Ok(Some(response.bytes().await?.to_vec())),
            Err(e) if e.status() == Some(StatusCode::NOT_FOUND) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// `GET /api/items/:id/file/:ino/download`, from byte `offset` on. The answer is
    /// `206 Partial Content` when the server honours the range, else the whole file.
    pub async fn download_file(&self, item_id: &str, ino: &str, offset: u64) -> Result<Response> {
        let url = self.url(&format!("api/items/{item_id}/file/{ino}/download"))?;
        let request = match offset {
            0 => self.http().get(url),
            _ => self
                .http()
                .get(url)
                .header(RANGE, format!("bytes={offset}-")),
        };
        self.send(request).await
    }

    /// `POST /api/items/:id/play`: starts a playback session.
    pub async fn play(&self, item_id: &str, request: &PlayRequest) -> Result<PlaybackSession> {
        let url = self.url(&format!("api/items/{item_id}/play"))?;
        self.send_json(self.http().post(url).json(request)).await
    }

    /// `POST /api/session/:id/sync`. A 404 means the server forgot the session.
    pub async fn sync_session(&self, session_id: &str, sync: &SessionSync) -> Result<()> {
        let url = self.url(&format!("api/session/{session_id}/sync"))?;
        self.send(self.http().post(url).json(sync)).await.map(drop)
    }

    /// `POST /api/session/:id/close`, with a last sync if given.
    pub async fn close_session(&self, session_id: &str, sync: Option<&SessionSync>) -> Result<()> {
        let url = self.url(&format!("api/session/{session_id}/close"))?;
        let request = match sync {
            Some(sync) => self.http().post(url).json(sync),
            None => self.http().post(url),
        };
        self.send(request).await.map(drop)
    }
}
