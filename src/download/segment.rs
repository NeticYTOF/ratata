use super::progress::ProgressTracker;
use super::{DownloadManager, direct};
use crate::download::state::SegmentState;
use crate::http::headers_map;
use crate::model::DownloadJob;
use anyhow::{Result, bail};
use futures_util::StreamExt;
use reqwest::header::RANGE;
use std::path::Path;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

pub async fn run(
    manager: &DownloadManager,
    job: &DownloadJob,
    segment: &SegmentState,
    tracker: &ProgressTracker,
    path: &Path,
) -> Result<()> {
    direct::ensure_parent(path)?;
    let expected = segment.end - segment.start + 1;
    let mut done = tokio::fs::metadata(path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if done > expected {
        done = 0;
    }
    let headers = headers_map(&job.format.headers)?;

    for attempt in 0..=manager.config.retries {
        manager.gate().await?;
        let offset = segment.start + done;
        let response = manager
            .client
            .get(&job.format.url)
            .headers(headers.clone())
            .header(RANGE, format!("bytes={offset}-{}", segment.end))
            .send()
            .await;
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                if attempt == manager.config.retries {
                    return Err(error.into());
                }
                backoff(attempt).await;
                continue;
            }
        };
        if response.status().as_u16() == 416 {
            return Ok(());
        }
        if !response.status().is_success() {
            if attempt == manager.config.retries {
                bail!("segment {} returned {}", segment.index, response.status());
            }
            backoff(attempt).await;
            continue;
        }
        let partial = response.status().as_u16() != 206;
        if partial && segment.start > 0 {
            bail!("server ignored byte ranges");
        }
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(offset == segment.start)
            .open(path)
            .await?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        if offset > segment.start {
            writer.seek(std::io::SeekFrom::Start(done)).await?;
        }
        let mut stream = response.bytes_stream();
        let mut chunk_start = if partial { 0 } else { done };
        let mut failed = false;
        while let Some(chunk) = stream.next().await {
            manager.gate().await?;
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            if partial && chunk_start + chunk.len() as u64 > expected {
                let keep = (expected - chunk_start) as usize;
                writer.write_all(&chunk[..keep]).await?;
                tracker.add(keep as u64);
                chunk_start += keep as u64;
                break;
            }
            writer.write_all(&chunk).await?;
            tracker.add(chunk.len() as u64);
            chunk_start += chunk.len() as u64;
        }
        writer.flush().await?;
        done = chunk_start;
        if done >= expected {
            return Ok(());
        }
        if attempt == manager.config.retries {
            bail!(
                "segment {} incomplete: {}/{} bytes",
                segment.index,
                done,
                expected
            );
        }
        backoff(attempt).await;
        if failed {
            // The partially written bytes are kept; the next attempt resumes at `done`.
            continue;
        }
    }
    bail!("segment {} exhausted retries", segment.index)
}

async fn backoff(attempt: u8) {
    let shift = attempt.min(10) as u64;
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    tokio::time::sleep(
        Duration::from_millis(80u64 << shift) + Duration::from_nanos(nanos % 100_000_000),
    )
    .await;
}
