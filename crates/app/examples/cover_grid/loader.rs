use std::sync::Arc;

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{Value, json};
use slint::{Rgba8Pixel, SharedPixelBuffer};
use tokio::sync::{Semaphore, mpsc};

const COVER_WIDTH: &str = "320";
const PARALLEL_DOWNLOADS: usize = 6;
const PAGE_SIZE: usize = 100;

/// One library item as the grid needs it.
pub struct GridItem {
    pub id: String,
    pub title: String,
}

/// Server connection shared by the item list and the cover loader.
#[derive(Clone)]
pub struct Server {
    client: Client,
    base: String,
    token: String,
}

/// A decoded cover ready to be turned into a `slint::Image` on the UI thread.
pub struct LoadedCover {
    pub index: usize,
    pub pixels: SharedPixelBuffer<Rgba8Pixel>,
}

impl Server {
    /// Logs in and keeps the access token.
    pub async fn login(base: &str, username: &str, password: &str) -> Result<Self> {
        let client = Client::new();
        let body: Value = client
            .post(format!("{base}/login"))
            .header("x-return-tokens", "true")
            .json(&json!({ "username": username, "password": password }))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = body["user"]["accessToken"]
            .as_str()
            .context("no access token")?
            .to_owned();
        Ok(Self {
            client,
            base: base.to_owned(),
            token,
        })
    }

    /// Fetches every item of the named library, page by page.
    pub async fn library_items(&self, library_name: &str) -> Result<Vec<GridItem>> {
        let library_id = self.library_id(library_name).await?;
        let mut items = Vec::new();
        for page in 0.. {
            let batch = self.items_page(&library_id, page).await?;
            let done = batch.len() < PAGE_SIZE;
            items.extend(batch);
            if done {
                break;
            }
        }
        Ok(items)
    }

    async fn library_id(&self, name: &str) -> Result<String> {
        let body = self.get_json("/api/libraries", &[]).await?;
        body["libraries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|lib| lib["name"] == name)
            .and_then(|lib| lib["id"].as_str())
            .map(str::to_owned)
            .with_context(|| format!("library {name} not found"))
    }

    async fn items_page(&self, library_id: &str, page: usize) -> Result<Vec<GridItem>> {
        let query = [
            ("minified", "1".to_owned()),
            ("limit", PAGE_SIZE.to_string()),
            ("page", page.to_string()),
        ];
        let body = self
            .get_json(&format!("/api/libraries/{library_id}/items"), &query)
            .await?;
        let results = body["results"].as_array().cloned().unwrap_or_default();
        Ok(results
            .iter()
            .map(|item| GridItem {
                id: item["id"].as_str().unwrap_or_default().to_owned(),
                title: item["media"]["metadata"]["title"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            })
            .collect())
    }

    async fn get_json(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        let resp = self
            .client
            .get(format!("{}{path}", self.base))
            .query(query)
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(resp.error_for_status()?.json().await?)
    }

    async fn cover_bytes(&self, item_id: &str) -> Result<Vec<u8>> {
        let resp = self
            .client
            .get(format!("{}/api/items/{item_id}/cover", self.base))
            .query(&[("width", COVER_WIDTH), ("format", "webp")])
            .bearer_auth(&self.token)
            .send()
            .await?;
        Ok(resp.error_for_status()?.bytes().await?.to_vec())
    }
}

/// Starts the cover worker; send `(index, item_id)` requests into the returned channel.
pub fn spawn_cover_loader(
    runtime: &tokio::runtime::Handle,
    server: Server,
    on_loaded: impl Fn(LoadedCover) + Send + Sync + 'static,
) -> mpsc::UnboundedSender<(usize, String)> {
    let (tx, mut rx) = mpsc::unbounded_channel::<(usize, String)>();
    let on_loaded = Arc::new(on_loaded);
    let limit = Arc::new(Semaphore::new(PARALLEL_DOWNLOADS));
    runtime.spawn(async move {
        while let Some((index, item_id)) = rx.recv().await {
            let permit = limit
                .clone()
                .acquire_owned()
                .await
                .expect("semaphore closed");
            let (server, on_loaded) = (server.clone(), on_loaded.clone());
            tokio::spawn(async move {
                match load_cover(&server, index, &item_id).await {
                    Ok(cover) => on_loaded(cover),
                    Err(err) => eprintln!("cover {item_id}: {err:#}"),
                }
                drop(permit);
            });
        }
    });
    tx
}

async fn load_cover(server: &Server, index: usize, item_id: &str) -> Result<LoadedCover> {
    let bytes = server.cover_bytes(item_id).await?;
    let pixels = tokio::task::spawn_blocking(move || decode(&bytes)).await??;
    Ok(LoadedCover { index, pixels })
}

fn decode(bytes: &[u8]) -> Result<SharedPixelBuffer<Rgba8Pixel>> {
    let rgba = image::load_from_memory(bytes)?.into_rgba8();
    Ok(SharedPixelBuffer::clone_from_slice(
        rgba.as_raw(),
        rgba.width(),
        rgba.height(),
    ))
}
