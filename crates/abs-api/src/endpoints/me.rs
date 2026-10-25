use reqwest::StatusCode;
use serde_json::json;

use crate::client::AbsClient;
use crate::error::{Error, Result};
use crate::models::{Bookmark, EbookLocation, MediaProgress, ProgressList, User};
use crate::version::Feature;

impl AbsClient {
    /// `GET /api/me`.
    pub async fn me(&self) -> Result<User> {
        self.send_json(self.http().get(self.url("api/me")?)).await
    }

    /// `GET /api/me/progress`. Older servers return [`Error::Unsupported`];
    /// use the `media_progress` of the login or `/api/me` user there.
    pub async fn all_progress(&self) -> Result<Vec<MediaProgress>> {
        if !self.supports(Feature::MeProgress) {
            return Err(Error::Unsupported(Feature::MeProgress));
        }
        let list: ProgressList = self
            .send_json(self.http().get(self.url("api/me/progress")?))
            .await?;
        Ok(list.media_progress)
    }

    /// `GET /api/me/progress/:id`. `None` when the item was never started.
    pub async fn item_progress(&self, item_id: &str) -> Result<Option<MediaProgress>> {
        let request = self
            .http()
            .get(self.url(&format!("api/me/progress/{item_id}"))?);
        match self.send_json(request).await {
            Err(e) if e.status() == Some(StatusCode::NOT_FOUND) => Ok(None),
            result => result.map(Some),
        }
    }

    /// `PATCH /api/me/progress/:id`: marks the item finished or not finished.
    pub async fn set_finished(&self, item_id: &str, finished: bool) -> Result<()> {
        let request = self
            .http()
            .patch(self.url(&format!("api/me/progress/{item_id}"))?)
            .json(&json!({ "isFinished": finished }));
        self.send(request).await.map(drop)
    }

    /// `PATCH /api/me/progress/:id` with a listening position, outside any session.
    pub async fn set_progress(
        &self,
        item_id: &str,
        current_time: f64,
        duration: f64,
    ) -> Result<()> {
        let progress = if duration > 0.0 {
            (current_time / duration).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let request = self
            .http()
            .patch(self.url(&format!("api/me/progress/{item_id}"))?)
            .json(
                &json!({ "currentTime": current_time, "duration": duration, "progress": progress }),
            );
        self.send(request).await.map(drop)
    }

    /// `PATCH /api/me/progress/:id` with the reading position, as the web reader sends it.
    pub async fn set_ebook_progress(
        &self,
        item_id: &str,
        location: &EbookLocation,
        progress: f64,
    ) -> Result<()> {
        let request = self
            .http()
            .patch(self.url(&format!("api/me/progress/{item_id}"))?)
            .json(&json!({ "ebookLocation": location, "ebookProgress": progress.clamp(0.0, 1.0) }));
        self.send(request).await.map(drop)
    }

    /// `POST /api/me/item/:id/bookmark`.
    pub async fn create_bookmark(&self, item_id: &str, time: f64, title: &str) -> Result<Bookmark> {
        let request = self
            .http()
            .post(self.bookmark_url(item_id)?)
            .json(&json!({ "time": time, "title": title }));
        self.send_json(request).await
    }

    /// `PATCH /api/me/item/:id/bookmark`: renames the bookmark at `time`.
    pub async fn update_bookmark(&self, item_id: &str, time: f64, title: &str) -> Result<Bookmark> {
        let request = self
            .http()
            .patch(self.bookmark_url(item_id)?)
            .json(&json!({ "time": time, "title": title }));
        self.send_json(request).await
    }

    /// `DELETE /api/me/item/:id/bookmark/:time`.
    pub async fn delete_bookmark(&self, item_id: &str, time: f64) -> Result<()> {
        let url = self.url(&format!("api/me/item/{item_id}/bookmark/{time}"))?;
        self.send(self.http().delete(url)).await.map(drop)
    }

    fn bookmark_url(&self, item_id: &str) -> Result<reqwest::Url> {
        self.url(&format!("api/me/item/{item_id}/bookmark"))
    }
}
