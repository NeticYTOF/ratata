use super::progress::ProgressTracker;
use super::state::{DownloadState, SegmentState};
use super::{DownloadManager, human_bytes};
use crate::config::state_path;
use crate::http::headers_map;
use crate::model::DownloadJob;
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::header::{ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, ETAG, RANGE};
use std::path::{Path, PathBuf};
use tokio::fs::OpenOptions;
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};

#[derive(Clone)]
struct Probe {
    total: Option<u64>,
    etag: Option<String>,
    ranges: bool,
}

/// Downloads into a single preallocated `.part` file. Each segment owns its own file
/// handle and writes at a fixed byte offset, so no second copy pass is needed.
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
    let segment_bytes = target_segment_bytes(manager, total, job);
    let count = manager
        .config
        .segments
        .min(total.div_ceil(segment_bytes).max(1) as usize)
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
    let part = single_part_path(&job.destination);
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

    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&part)
        .await?;
    if file.metadata().await?.len() != total {
        file.set_len(total).await?;
    }
    drop(file);

    for segment in &mut state.segments {
        segment.done = segment.done.min(segment.end - segment.start + 1);
    }
    state.save(&state_file).ok();
    tracker.add(state.segments.iter().map(|s| s.done).sum());

    let mut tasks = futures_util::stream::iter(state.segments.clone().into_iter().map(|segment| {
        let tracker = tracker.clone();
        let path = part.clone();
        async move {
            let index = segment.index;
            let result = super::segment::run(manager, job, &segment, &tracker, &path).await;
            (index, result)
        }
    }))
    .buffer_unordered(count);

    let mut failures = Vec::new();
    while let Some((index, result)) = tasks.next().await {
        match result {
            Ok(()) => {
                if let Some(segment) = state.segments.iter_mut().find(|s| s.index == index) {
                    segment.done = segment.end - segment.start + 1;
                }
                state.save(&state_file).ok();
            }
            Err(error) => failures.push(error.to_string()),
        }
    }
    if !failures.is_empty() {
        bail!("segment failures: {}", failures.join("; "));
    }
    let written = tokio::fs::metadata(&part).await?.len();
    if written != total {
        return Err(crate::error::RatataError::Incomplete(format!(
            "expected {}, got {}",
            human_bytes(total),
            human_bytes(written)
        ))
        .into());
    }
    tokio::fs::rename(&part, &job.destination).await?;
    tokio::fs::remove_file(&state_file).await.ok();
    Ok(job.destination.clone())
}

fn target_segment_bytes(manager: &DownloadManager, total: u64, job: &DownloadJob) -> u64 {
    if let Some(size) = job.format.size {
        let count = manager.config.segments.clamp(1, 64) as u64;
        return size
            .div_ceil(count)
            .max(manager.config.min_segment_size)
            .max(1);
    }
    // Without a declared size, use the observed connection quality to pick chunks.
    let observed = manager
        .average_speed
        .load(std::sync::atomic::Ordering::Relaxed);
    let per_second = observed.max(1);
    (per_second / 4)
        .clamp(manager.config.min_segment_size, 16 * 1024 * 1024)
        .min(total.max(1))
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
        .with_file_name(format!("{}.stream.part", file_name(&job.destination)));
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

pub fn single_part_path(destination: &Path) -> PathBuf {
    destination.with_file_name(format!("{}.part", file_name(destination)))
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
