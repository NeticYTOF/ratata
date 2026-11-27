use super::concat_parts;
use crate::download::DownloadManager;
use crate::download::direct;
use crate::download::progress::ProgressTracker;
use crate::error::RatataError;
use crate::http::{get_text, headers_map};
use crate::model::DownloadJob;
use anyhow::{Result, bail};
use futures_util::StreamExt;
use regex::Regex;
use reqwest::header::CONTENT_LENGTH;
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use url::Url;

#[derive(Clone, Debug)]
struct Representation {
    id: String,
    bandwidth: u64,
    width: u32,
    height: u32,
    base_url: Option<String>,
}

pub async fn download(
    manager: &DownloadManager,
    job: &DownloadJob,
    tracker: &ProgressTracker,
) -> Result<PathBuf> {
    let mpd = get_text(&manager.client, &job.format.url, &job.format.headers).await?;
    let representation = select_representation(&mpd)?;
    direct::ensure_parent(&job.destination)?;
    let headers = headers_map(&job.format.headers)?;

    if let Some(base) = &representation.base_url
        && !mpd.contains("SegmentTemplate")
        && !mpd.contains("SegmentList")
    {
        let absolute = Url::parse(&job.format.url)?.join(base)?.to_string();
        let mut single = job.clone();
        single.format.url = absolute;
        single.format.manifest = None;
        single.format.size = None;
        return direct::download(manager, &single, tracker).await;
    }

    let (init_url, segment_urls) = segment_urls(&mpd, &job.format.url, &representation)?;
    if segment_urls.is_empty() {
        return Err(RatataError::Incomplete("empty DASH segment list".into()).into());
    }
    let stem = job
        .destination
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ratata".into());
    let mut parts: Vec<PathBuf> = Vec::new();
    let mut targets: Vec<Option<String>> = Vec::new();
    if let Some(init) = init_url {
        parts.push(job.destination.with_file_name(format!("{stem}.dash-init")));
        targets.push(Some(init));
    }
    for (index, url) in segment_urls.iter().enumerate() {
        parts.push(
            job.destination
                .with_file_name(format!("{stem}.dash{index}")),
        );
        targets.push(Some(url.clone()));
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
    let concurrency = manager.config.segments.saturating_mul(4).clamp(4, 64);
    let ctx = manager.context();
    let mut tasks = futures_util::stream::iter(parts.clone().into_iter().zip(targets).map(
        |(part, target)| {
            let ctx = ctx.clone();
            let tracker = tracker.clone();
            let part = part.clone();
            async move {
                let url = target.expect("target URL");
                let mut last = String::new();
                for attempt in 0..=ctx.config.retries {
                    ctx.gate().await?;
                    match ctx.client.get(&url).send().await {
                        Ok(response) if response.status().is_success() => {
                            let bytes = response.bytes().await?;
                            let mut file = OpenOptions::new()
                                .create(true)
                                .write(true)
                                .truncate(true)
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
        },
    ))
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

fn select_representation(mpd: &str) -> Result<Representation> {
    let regex = Regex::new(r#"(?s)<Representation\b([^>]*)>(.*?)</Representation>"#)?;
    let mut best: Option<Representation> = None;
    for capture in regex.captures_iter(mpd) {
        let attrs = capture.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = capture.get(2).map(|m| m.as_str()).unwrap_or("");
        if attribute(attrs, "mimeType").is_some_and(|m| m.starts_with("audio")) {
            continue;
        }
        let representation = Representation {
            id: attribute(attrs, "id").unwrap_or_else(|| "0".into()),
            bandwidth: attribute(attrs, "bandwidth")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            width: attribute(attrs, "width")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            height: attribute(attrs, "height")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            base_url: None,
        };
        // BaseURL is filled by the caller after selection if necessary.
        let score = (
            representation.bandwidth,
            representation.width * representation.height,
        );
        if best
            .as_ref()
            .is_none_or(|b| score > (b.bandwidth, b.width * b.height))
        {
            let mut rep = representation;
            if let Some(base) = capture_base_url(body) {
                rep.base_url = Some(base);
            }
            best = Some(rep);
        }
    }
    best.ok_or_else(|| RatataError::NoFormats("DASH MPD".into()).into())
}

fn capture_base_url(body: &str) -> Option<String> {
    let regex = Regex::new(r"(?s)<BaseURL\b[^>]*>(.*?)</BaseURL>").ok()?;
    regex
        .captures(body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn segment_urls(
    mpd: &str,
    base_url: &str,
    rep: &Representation,
) -> Result<(Option<String>, Vec<String>)> {
    if mpd.contains("<SegmentList") {
        let urls: Vec<String> = Regex::new(r#"<SegmentURL\b[^>]*\bmedia="([^"]+)""#)?
            .captures_iter(mpd)
            .map(|c| resolve(base_url, &c[1]))
            .collect::<Result<_>>()?;
        let init = Regex::new(r#"<Initialization\b[^>]*\bsourceURL="([^"]+)""#)
            .ok()
            .and_then(|r| r.captures(mpd))
            .map(|c| resolve(base_url, &c[1]))
            .transpose()?;
        return Ok((init, urls));
    }
    let template = Regex::new(r#"<SegmentTemplate\b([^>]*)>"#)?
        .captures(mpd)
        .map(|c| c[1].to_owned())
        .ok_or_else(|| RatataError::Extraction("MPD has no SegmentTemplate".into()))?;
    let media_template = attribute(&template, "media").unwrap_or_else(|| "$Number$.m4s".into());
    let init_template = attribute(&template, "initialization");
    let start = attribute(&template, "startNumber")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(1);
    let count = segment_count(mpd, &template);
    let expand = |value: &str, number: u64| -> String {
        value
            .replace("$RepresentationID$", &rep.id)
            .replace("$Number%05d$", &format!("{number:05}"))
            .replace("$Number%04d$", &format!("{number:04}"))
            .replace("$Number$", &number.to_string())
    };
    let init = init_template
        .map(|t| resolve(base_url, &expand(&t, start)))
        .transpose()?;
    let urls = (start..start + count)
        .map(|n| resolve(base_url, &expand(&media_template, n)))
        .collect::<Result<Vec<_>>>()?;
    Ok((init, urls))
}

fn segment_count(mpd: &str, template: &str) -> u64 {
    let timeline = Regex::new(r"<S\b[^>]*>").ok().and_then(|r| {
        let captures: Vec<_> = r.captures_iter(mpd).collect();
        if captures.is_empty() {
            return None;
        }
        Some(
            captures
                .iter()
                .map(|c| {
                    attribute(c.get(0).unwrap().as_str(), "r")
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(0)
                        + 1
                })
                .sum(),
        )
    });
    if let Some(count) = timeline {
        return count;
    }
    let duration = attribute(template, "duration")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(4.0);
    let timescale = attribute(template, "timescale")
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(1.0);
    let total = attribute(mpd, "mediaPresentationDuration")
        .map(|value| iso_duration(&value))
        .unwrap_or(0.0);
    ((total * timescale) / duration).ceil() as u64
}

fn iso_duration(value: &str) -> f64 {
    let regex = Regex::new(r"PT(?:(\d+)H)?(?:(\d+)M)?(?:(\d+(?:\.\d+)?)S)?").expect("static regex");
    let Some(captures) = regex.captures(value) else {
        return 0.0;
    };
    let hours: f64 = captures
        .get(1)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let minutes: f64 = captures
        .get(2)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    let seconds: f64 = captures
        .get(3)
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(0.0);
    hours * 3600.0 + minutes * 60.0 + seconds
}

fn attribute(input: &str, name: &str) -> Option<String> {
    let regex = Regex::new(&format!(r#"\b{name}="([^"]+)""#)).ok()?;
    regex.captures(input).map(|c| c[1].to_owned())
}

fn resolve(base: &str, uri: &str) -> Result<String> {
    Ok(Url::parse(base)?.join(uri)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_segment_template() {
        let mpd = r#"<MPD mediaPresentationDuration="PT20S"><Period><AdaptationSet><SegmentTemplate timescale="1" duration="4" media="seg-$Number$.m4s" initialization="init-$RepresentationID$.mp4" /><Representation id="720p" bandwidth="2000000" width="1280" height="720"><BaseURL>v/</BaseURL></Representation></AdaptationSet></Period></MPD>"#;
        let rep = select_representation(mpd).unwrap();
        assert_eq!(rep.id, "720p");
        assert_eq!(rep.base_url.as_deref(), Some("v/"));
        let (init, urls) = segment_urls(mpd, "https://cdn.example/mpd/manifest.mpd", &rep).unwrap();
        assert_eq!(
            init.as_deref(),
            Some("https://cdn.example/mpd/init-720p.mp4")
        );
        assert_eq!(urls.len(), 5);
        assert_eq!(urls[0], "https://cdn.example/mpd/seg-1.m4s");
    }
}
