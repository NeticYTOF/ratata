use super::DownloadManager;
use super::progress::ProgressTracker;
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
    part: &Path,
) -> Result<()> {
    let expected = segment.end - segment.start + 1;
    let mut done = segment.done.min(expected);
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
        // A separate handle per segment lets all ranges write concurrently at fixed offsets.
        let file = OpenOptions::new().write(true).open(part).await?;
        let mut writer = BufWriter::with_capacity(256 * 1024, file);
        writer.seek(std::io::SeekFrom::Start(offset)).await?;
        let mut stream = response.bytes_stream();
        let mut failed = false;
        let stall = std::time::Duration::from_secs(manager.config.timeout_secs.max(1));
        loop {
            let next = tokio::time::timeout(stall, stream.next()).await;
            let chunk = match next {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            manager.gate().await?;
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    failed = true;
                    break;
                }
            };
            if partial && done + chunk.len() as u64 > expected {
                let keep = (expected - done) as usize;
                writer.write_all(&chunk[..keep]).await?;
                tracker.add(keep as u64);
                done += keep as u64;
                break;
            }
            writer.write_all(&chunk).await?;
            tracker.add(chunk.len() as u64);
            done += chunk.len() as u64;
        }
        writer.flush().await?;
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
        if !failed {
            done = done.min(expected);
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
