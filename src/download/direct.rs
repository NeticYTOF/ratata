use super::progress::ProgressTracker;
use super::state::{DownloadState, SegmentState};
use super::{DownloadManager, human_bytes};
use crate::config::state_path;
use crate::error::RatataError;
use crate::http::headers_map;
use crate::model::DownloadJob;
use anyhow::{Context, Result, anyhow, bail};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, ETAG, RANGE};
use std::path::{Path, PathBuf};
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
#[derive(Clone)]
struct Probe {
    total: Option<u64>,
    etag: Option<String>,
    ranges: bool,
}

pub async fn download(
    manager: &DownloadManager,
    job: &DownloadJob,
    tracker: &ProgressTracker,
) -> Result<PathBuf> {
    let probe = probe(manager, job).await?;
    if !probe.ranges || probe.total.is_none() || probe.total == Some(0) {
        return stream_single(manager, job, tracker, probe.total).await;
    }
    let total = probe.total.unwrap();
    let count = manager
        .config
        .segments
        .min(
            total
                .div_ceil(manager.config.min_segment_size.max(1))
                .max(1) as usize,
        )
        .clamp(1, 64);
    let chunk = total.div_ceil(count as u64);
    let segments: Vec<SegmentState> = (0..count)
        .map(|index| {
            let start = index as u64 * chunk;
            let end = (start + chunk - 1).min(total - 1);
            SegmentState {
                index,
                start,
                end,
                done: 0,
            }
        })
        .collect();

    let state_file = state_path(&job.destination);
    let mut state = DownloadState::load(&state_file, &job.format.url)
        .filter(|s| s.matches(Some(total), &probe.etag) && s.segments.len() == count)
        .unwrap_or_else(|| {
            DownloadState::new(
                &job.format.url,
                Some(total),
                probe.etag.clone(),
                segments.clone(),
            )
        });

    for segment in &mut state.segments {
        let path = part_path(&job.destination, segment.index);
        let size = tokio::fs::metadata(&path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        segment.done = if size <= segment.end - segment.start + 1 {
            size
        } else {
            0
        };
        if segment.done == 0 && size > 0 {
            tokio::fs::remove_file(&path).await.ok();
        }
    }
    state.save(&state_file).ok();
    tracker.add(state.segments.iter().map(|s| s.done).sum());

    let mut tasks = futures_util::stream::iter(state.segments.clone().into_iter().map(|segment| {
        let tracker = tracker.clone();
        let path = part_path(&job.destination, segment.index);
        async move { super::segment::run(manager, job, &segment, &tracker, &path).await }
    }))
    .buffer_unordered(count);

    let mut failures = Vec::new();
    while let Some(result) = tasks.next().await {
        if let Err(error) = result {
            failures.push(error.to_string());
        }
    }
    if !failures.is_empty() {
        bail!("segment failures: {}", failures.join("; "));
    }

    assemble(&job.destination, count, total).await?;
    tokio::fs::remove_file(&state_file).await.ok();
    Ok(job.destination.clone())
}

async fn probe(manager: &DownloadManager, job: &DownloadJob) -> Result<Probe> {
    let headers = headers_map(&job.format.headers)?;
    let head = manager
        .client
        .head(&job.format.url)
        .headers(headers.clone())
        .send()
        .await;
    if let Ok(response) = head
        && response.status().is_success()
    {
        let total = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok());
        let etag = response
            .headers()
            .get(ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let ranges = response
            .headers()
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| !v.eq_ignore_ascii_case("none"));
        return Ok(Probe {
            total,
            etag,
            ranges,
        });
    }
    // Some hosts do not answer HEAD; a single-byte GET provides the same metadata.
    let response = manager
        .client
        .get(&job.format.url)
        .headers(headers)
        .header(RANGE, "bytes=0-0")
        .send()
        .await
        .context("probing download URL")?;
    let total = response
        .headers()
        .get(CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit('/').next())
        .and_then(|v| v.parse().ok())
        .or_else(|| {
            response
                .headers()
                .get(CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok())
        });
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let ranges = response.status().as_u16() == 206;
    Ok(Probe {
        total,
        etag,
        ranges,
    })
}

async fn stream_single(
    manager: &DownloadManager,
    job: &DownloadJob,
    tracker: &ProgressTracker,
    total: Option<u64>,
) -> Result<PathBuf> {
    let part = job
        .destination
        .with_file_name(format!("{}.part", file_name(&job.destination)));
    let mut start = tokio::fs::metadata(&part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    let headers = headers_map(&job.format.headers)?;
    let mut response = manager
        .client
        .get(&job.format.url)
        .headers(headers.clone())
        .header(RANGE, format!("bytes={start}-"))
        .send()
        .await?;
    if response.status().as_u16() != 206 {
        start = 0;
        response = manager
            .client
            .get(&job.format.url)
            .headers(headers)
            .send()
            .await?;
    }
    response.error_for_status_ref()?;
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(start == 0)
        .open(&part)
        .await?;
    let mut writer = BufWriter::with_capacity(256 * 1024, file);
    if start > 0 {
        writer.seek(std::io::SeekFrom::Start(start)).await?;
    }
    tracker.add(start);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        manager.gate().await?;
        let chunk = chunk?;
        writer.write_all(&chunk).await?;
        tracker.add(chunk.len() as u64);
    }
    writer.flush().await?;
    tokio::fs::rename(&part, &job.destination).await?;
    let _ = total;
    Ok(job.destination.clone())
}

async fn assemble(destination: &Path, count: usize, total: u64) -> Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(destination)
        .await?;
    let mut writer = BufWriter::with_capacity(1 << 20, file);
    let mut written = 0u64;
    for index in 0..count {
        let part = part_path(destination, index);
        let mut reader = File::open(&part)
            .await
            .with_context(|| format!("opening part {index}"))?;
        written += tokio::io::copy(&mut reader, &mut writer).await?;
        tokio::fs::remove_file(&part).await.ok();
    }
    writer.flush().await?;
    if written != total {
        return Err(RatataError::Incomplete(format!(
            "expected {}, got {} bytes",
            human_bytes(total),
            human_bytes(written)
        ))
        .into());
    }
    Ok(())
}

pub fn part_path(destination: &Path, index: usize) -> PathBuf {
    destination.with_file_name(format!("{}.part{index}", file_name(destination)))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ratata".into())
}

pub fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[allow(dead_code)]
fn unused() -> anyhow::Error {
    anyhow!("unused")
}
