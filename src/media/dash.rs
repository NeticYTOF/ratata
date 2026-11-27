#[path = "dash_manifest.rs"]
mod manifest;

use super::concat_parts;
use crate::download::DownloadManager;
use crate::download::direct;
use crate::download::progress::ProgressTracker;
use crate::error::RatataError;
use crate::http::{get_text, headers_map};
use crate::model::DownloadJob;
use anyhow::{Result, bail};
use futures_util::StreamExt;
use reqwest::header::CONTENT_LENGTH;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use url::Url;

pub async fn download(
    manager: &DownloadManager,
    job: &DownloadJob,
    tracker: &ProgressTracker,
) -> Result<PathBuf> {
    let mpd = get_text(&manager.client, &job.format.url, &job.format.headers).await?;
    let representation = manifest::select_representation(&mpd)?;
    direct::ensure_parent(&job.destination)?;
    let headers = headers_map(&job.format.headers)?;

    if let Some(base) = &representation.base_url
        && manifest::segment_template_for(&mpd, &representation.id).is_none()
    {
        let absolute = Url::parse(&job.format.url)?.join(base)?.to_string();
        let mut single = job.clone();
        single.format.url = absolute;
        single.format.manifest = None;
        single.format.size = None;
        return direct::download(manager, &single, tracker).await;
    }

    let template = manifest::segment_template_for(&mpd, &representation.id)
        .ok_or_else(|| RatataError::Extraction("MPD has no matching SegmentTemplate".into()))?;
    let (init_url, segment_urls) =
        manifest::segment_urls(&template, &job.format.url, &representation)?;
    if segment_urls.is_empty() {
        return Err(RatataError::Incomplete("empty DASH segment list".into()).into());
    }

    let stem = job
        .destination
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ratata".into());
    let mut parts: Vec<PathBuf> = Vec::new();
    let mut targets: Vec<String> = Vec::new();
    if let Some(init) = init_url {
        parts.push(job.destination.with_file_name(format!("{stem}.dash-init")));
        targets.push(init);
    }
    for (index, url) in segment_urls.iter().enumerate() {
        parts.push(
            job.destination
                .with_file_name(format!("{stem}.dash{index}")),
        );
        targets.push(url.clone());
    }

    if let Ok(response) = manager
        .client
        .head(&segment_urls[0])
        .headers(headers)
        .send()
        .await
        && let Some(size) = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    {
        tracker.set_total(size * segment_urls.len() as u64);
    }

    let ctx = manager.context();
    let concurrency = manager.config.segments.clamp(2, 12);
    let mut tasks =
        futures_util::stream::iter(parts.clone().into_iter().zip(targets).map(|(part, url)| {
            let ctx = ctx.clone();
            let tracker = tracker.clone();
            async move {
                let mut last = String::new();
                for attempt in 0..=ctx.config.retries {
                    ctx.gate().await?;
                    match ctx.client.get(&url).send().await {
                        Ok(response) if response.status().is_success() => {
                            let bytes = response.bytes().await?;
                            let mut file = OpenOptions::new()
                                .create(true)
                                .truncate(true)
                                .write(true)
                                .open(&part)
                                .await?;
                            file.write_all(&bytes).await?;
                            file.flush().await?;
                            tracker.add(bytes.len() as u64);
                            return Ok(());
                        }
                        Ok(response) => {
                            last = format!("HTTP {}", response.status());
                            if response.status().as_u16() == 404 {
                                break;
                            }
                        }
                        Err(error) => last = error.to_string(),
                    }
                    tokio::time::sleep(Duration::from_millis(100u64 << attempt.min(8))).await;
                }
                bail!("DASH segment failed: {last}")
            }
        }))
        .buffer_unordered(concurrency);

    let mut failures = Vec::new();
    while let Some(result) = tasks.next().await {
        if let Err(error) = result {
            failures.push(error.to_string());
        }
    }
    if !failures.is_empty() {
        bail!("DASH segment failures: {}", failures.join("; "));
    }
    let written = concat_parts(&parts, &job.destination).await?;
    tracker.set_total(written);
    for part in &parts {
        tokio::fs::remove_file(part).await.ok();
    }
    Ok(job.destination.clone())
}
