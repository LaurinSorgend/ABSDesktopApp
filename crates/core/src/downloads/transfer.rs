use std::path::Path;

use abs_api::AbsClient;
use abs_api::reqwest::StatusCode;
use futures_util::StreamExt;
use tokio::fs::{File, OpenOptions};
use tokio::io::AsyncWriteExt;

use super::DownloadError;
use crate::db::DownloadFile;

/// Fetches one file into `dir`, resuming a `.part` file left by an earlier try.
/// `progress` gets the bytes on disk after each chunk. The file is renamed to its
/// final name once its size matches.
pub async fn fetch_file(
    client: &AbsClient,
    item_id: &str,
    dir: &Path,
    file: &DownloadFile,
    mut progress: impl FnMut(u64),
) -> Result<(), DownloadError> {
    let target = dir.join(&file.file_name);
    if tokio::fs::metadata(&target)
        .await
        .is_ok_and(|m| m.len() == file.size)
    {
        progress(file.size);
        return Ok(());
    }
    tokio::fs::create_dir_all(dir).await?;
    let part = dir.join(format!("{}.part", file.file_name));
    let mut offset = part_length(&part, file.size).await?;
    if offset > 0 && offset == file.size {
        progress(file.size);
        return finish(&part, &target, file).await;
    }
    let response = client.download_file(item_id, &file.ino, offset).await?;
    if response.status() != StatusCode::PARTIAL_CONTENT {
        offset = 0;
    }
    let mut out = open_part(&part, offset).await?;
    let mut done = offset;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        out.write_all(&chunk).await?;
        done += chunk.len() as u64;
        progress(done);
    }
    out.flush().await?;
    drop(out);
    finish(&part, &target, file).await
}

/// Bytes already in the `.part` file; a file longer than expected starts over.
async fn part_length(part: &Path, size: u64) -> std::io::Result<u64> {
    match tokio::fs::metadata(part).await {
        Ok(meta) if meta.len() <= size => Ok(meta.len()),
        Ok(_) => {
            tokio::fs::remove_file(part).await?;
            Ok(0)
        }
        Err(_) => Ok(0),
    }
}

async fn open_part(part: &Path, offset: u64) -> std::io::Result<File> {
    if offset == 0 {
        return File::create(part).await;
    }
    OpenOptions::new().append(true).open(part).await
}

async fn finish(part: &Path, target: &Path, file: &DownloadFile) -> Result<(), DownloadError> {
    let actual = tokio::fs::metadata(part).await?.len();
    if actual != file.size {
        if actual > file.size {
            tokio::fs::remove_file(part).await?;
        }
        return Err(DownloadError::SizeMismatch {
            file: file.file_name.clone(),
            expected: file.size,
            actual,
        });
    }
    tokio::fs::rename(part, target).await?;
    Ok(())
}
