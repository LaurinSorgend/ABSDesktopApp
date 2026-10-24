//! Phase 0 smoke test against a running Audiobookshelf server.
//!
//! `cargo run -p abs-api --example smoke -- [url] [username] [password]`
//! Defaults match the Docker test server from `docker/`.

use anyhow::{Context, Result, bail};
use reqwest::{Client, Response, StatusCode, header};
use serde_json::{Value, json};

struct Tokens {
    access: String,
    refresh: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let base = args
        .next()
        .unwrap_or_else(|| "http://localhost:13379".into());
    let username = args.next().unwrap_or_else(|| "admin".into());
    let password = args.next().unwrap_or_else(|| "admin-pass".into());
    let client = Client::new();

    print_status(&client, &base).await?;
    let tokens = login(&client, &base, &username, &password).await?;
    let tokens = refresh(&client, &base, &tokens).await?;
    let items = list_items(&client, &base, &tokens.access).await?;
    let first = items.first().context("library has no items")?;
    let item_id = first["id"].as_str().context("item without id")?;
    download_cover(&client, &base, &tokens.access, item_id).await?;
    check_range_support(&client, &base, &tokens.access, item_id).await?;
    Ok(())
}

async fn print_status(client: &Client, base: &str) -> Result<()> {
    let status: Value = client
        .get(format!("{base}/status"))
        .send()
        .await?
        .json()
        .await?;
    println!("server version: {}", status["serverVersion"]);
    Ok(())
}

async fn login(client: &Client, base: &str, username: &str, password: &str) -> Result<Tokens> {
    let resp = client
        .post(format!("{base}/login"))
        .header("x-return-tokens", "true")
        .json(&json!({ "username": username, "password": password }))
        .send()
        .await?;
    let tokens = tokens_from(ok(resp).await?).await?;
    println!("login ok, refresh token returned in body");
    Ok(tokens)
}

async fn refresh(client: &Client, base: &str, tokens: &Tokens) -> Result<Tokens> {
    let resp = client
        .post(format!("{base}/auth/refresh"))
        .header("x-refresh-token", &tokens.refresh)
        .send()
        .await?;
    let fresh = tokens_from(ok(resp).await?).await?;
    println!(
        "refresh ok, refresh token rotated: {}",
        fresh.refresh != tokens.refresh
    );
    Ok(fresh)
}

async fn tokens_from(resp: Response) -> Result<Tokens> {
    let body: Value = resp.json().await?;
    let user = &body["user"];
    let field = |name: &str| {
        user[name]
            .as_str()
            .map(str::to_owned)
            .with_context(|| format!("missing user.{name}"))
    };
    Ok(Tokens {
        access: field("accessToken")?,
        refresh: field("refreshToken")?,
    })
}

async fn list_items(client: &Client, base: &str, token: &str) -> Result<Vec<Value>> {
    let libraries: Value = ok(client
        .get(format!("{base}/api/libraries"))
        .bearer_auth(token)
        .send()
        .await?)
    .await?
    .json()
    .await?;
    let libraries = libraries["libraries"]
        .as_array()
        .context("no libraries array")?;
    println!(
        "libraries: {}",
        libraries
            .iter()
            .map(|l| l["name"].to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let library_id = libraries
        .first()
        .and_then(|l| l["id"].as_str())
        .context("no library")?;
    let page: Value = ok(client
        .get(format!("{base}/api/libraries/{library_id}/items"))
        .query(&[("minified", "1"), ("limit", "20"), ("page", "0")])
        .bearer_auth(token)
        .send()
        .await?)
    .await?
    .json()
    .await?;
    let results = page["results"].as_array().cloned().unwrap_or_default();
    println!("first page: {} of {} items", results.len(), page["total"]);
    for item in &results {
        println!("  {} ({})", item["media"]["metadata"]["title"], item["id"]);
    }
    Ok(results)
}

async fn download_cover(client: &Client, base: &str, token: &str, item_id: &str) -> Result<()> {
    let resp = client
        .get(format!("{base}/api/items/{item_id}/cover"))
        .query(&[("width", "400"), ("format", "webp")])
        .bearer_auth(token)
        .send()
        .await?;
    if resp.status() == StatusCode::NOT_FOUND {
        println!("cover: item has no cover");
        return Ok(());
    }
    let resp = ok(resp).await?;
    let content_type = resp.headers().get(header::CONTENT_TYPE).cloned();
    let bytes = resp.bytes().await?;
    println!(
        "cover: {} bytes, content-type {content_type:?}",
        bytes.len()
    );
    Ok(())
}

async fn check_range_support(
    client: &Client,
    base: &str,
    token: &str,
    item_id: &str,
) -> Result<()> {
    let item: Value = ok(client
        .get(format!("{base}/api/items/{item_id}"))
        .query(&[("expanded", "1")])
        .bearer_auth(token)
        .send()
        .await?)
    .await?
    .json()
    .await?;
    let ino = item["libraryFiles"][0]["ino"]
        .as_str()
        .context("item has no library files")?;
    let resp = client
        .get(format!("{base}/api/items/{item_id}/file/{ino}/download"))
        .header(header::RANGE, "bytes=100-1123")
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    let content_range = resp.headers().get(header::CONTENT_RANGE).cloned();
    let accept_ranges = resp.headers().get(header::ACCEPT_RANGES).cloned();
    let len = resp.bytes().await?.len();
    println!(
        "file download with Range: status {status}, content-range {content_range:?}, accept-ranges {accept_ranges:?}, {len} bytes"
    );
    Ok(())
}

async fn ok(resp: Response) -> Result<Response> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    let url = resp.url().clone();
    let body = resp.text().await.unwrap_or_default();
    bail!("{status} from {url}: {body}")
}
