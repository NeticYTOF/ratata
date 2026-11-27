use super::{concat_parts, ffmpeg_available, remux_ts};
use crate::download::DownloadManager;
use crate::download::progress::ProgressTracker;
use crate::http::{get_text, headers_map};
use crate::model::DownloadJob;
use aes::Aes128;
use aes::cipher::{BlockDecrypt, KeyInit, generic_array::GenericArray};
use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, RANGE};
use std::path::PathBuf;
use std::time::Duration;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use url::Url;

#[derive(Clone, Debug)]
struct KeyInfo {
    uri: String,
    iv: Option<String>,
}

#[derive(Clone, Debug)]
struct Segment {
    url: String,
    key: Option<KeyInfo>,
    /// `EXT-X-BYTERANGE:length@offset`, used by fMP4 playlists with one big file.
    range: Option<(u64, u64)>,
}

pub async fn download(
    manager: &DownloadManager,
    job: &DownloadJob,
    tracker: &ProgressTracker,
) -> Result<PathBuf> {
    let ctx = manager.context();
    let mut playlist_url = job.format.url.clone();
    let headers = headers_map(&job.format.headers)?;
    let timeout_secs = manager.config.timeout_secs.max(1);
    let fetch_playlist = |url: String| {
        let client = manager.client.clone();
        let job_headers = job.format.headers.clone();
        async move {
            tokio::time::timeout(
                Duration::from_secs(timeout_secs),
                get_text(&client, &url, &job_headers),
            )
            .await
            .context("HLS playlist fetch timed out")?
        }
    };
    let master = fetch_playlist(playlist_url.clone()).await?;
    if master.contains("#EXT-X-STREAM-INF") {
        playlist_url = select_variant(&playlist_url, &master)?;
    }
    let playlist = fetch_playlist(playlist_url.clone()).await?;
    if playlist.contains("METHOD=SAMPLE-AES") || playlist.contains("skd://") {
        bail!("HLS stream is DRM-protected (SAMPLE-AES); cannot download without a license");
    }
    let (segments, init) = parse_playlist(&playlist_url, &playlist)?;
    if segments.is_empty() {
        bail!("empty HLS playlist");
    }
    crate::download::direct::ensure_parent(&job.destination)?;

    let is_ts = segments
        .first()
        .is_some_and(|s| s.url.split('?').next().unwrap_or(&s.url).ends_with(".ts"));
    let destination = if is_ts {
        job.destination.with_extension("ts")
    } else {
        job.destination.clone()
    };
    let stem = destination
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ratata".into());
    let part_count = segments.len() + usize::from(init.is_some());
    let parts: Vec<PathBuf> = (0..part_count)
        .map(|i| destination.with_file_name(format!("{stem}.hls{i}")))
        .collect();
    let cleanup_parts = parts.clone();

    let ranged_total: u64 = segments
        .iter()
        .chain(init.iter())
        .filter_map(|s| s.range.map(|(len, _)| len))
        .sum();
    if ranged_total > 0 {
        tracker.set_total(ranged_total);
    } else if let Ok(response) = manager
        .client
        .head(&segments[0].url)
        .headers(headers)
        .send()
        .await
        && let Some(first_size) = response
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
    {
        tracker.set_total(first_size * parts.len() as u64);
    }
    tracker.set_segment_count(cleanup_parts.len());
    let concurrency = manager.config.segments.clamp(2, 12);
    let sequence: Vec<Segment> = std::iter::once(init.clone())
        .flatten()
        .chain(segments.iter().cloned())
        .collect();
    let mut tasks = futures_util::stream::iter(parts.into_iter().enumerate().zip(sequence).map(
        |((index, part), segment)| {
            let ctx = ctx.clone();
            let tracker = tracker.clone();
            async move {
                let bytes = fetch(&ctx, &segment).await?;
                let mut file = OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&part)
                    .await?;
                file.write_all(&bytes).await?;
                file.flush().await?;
                tracker.add(bytes.len() as u64);
                tracker.mark_segment(index);
                Ok::<(), anyhow::Error>(())
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
        bail!("HLS segment failures: {}", failures.join("; "));
    }

    let written = concat_parts(&cleanup_parts, &destination).await?;
    tracker.set_total(written);
    for part in &cleanup_parts {
        tokio::fs::remove_file(part).await.ok();
    }
    if is_ts && let Some(ffmpeg) = ffmpeg_available(manager.config.ffmpeg.as_deref()) {
        let mp4 = job.destination.clone();
        if remux_ts(&destination, &mp4, &ffmpeg).is_ok() {
            tokio::fs::remove_file(&destination).await.ok();
            return Ok(mp4);
        }
    }
    Ok(destination)
}

async fn fetch(ctx: &crate::download::WorkerContext, segment: &Segment) -> Result<Vec<u8>> {
    let mut last = String::new();
    for attempt in 0..=ctx.config.retries {
        ctx.gate().await?;
        let mut request = ctx.client.get(&segment.url);
        if let Some((length, offset)) = segment.range {
            request = request.header(RANGE, format!("bytes={offset}-{}", offset + length - 1));
        }
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                let bytes = response.bytes().await?.to_vec();
                return match &segment.key {
                    Some(key) => decrypt(&ctx.client, &segment.url, &bytes, key).await,
                    None => Ok(bytes),
                };
            }
            Ok(response) => last = format!("HTTP {}", response.status()),
            Err(error) => last = error.to_string(),
        }
        tokio::time::sleep(Duration::from_millis(100u64 << attempt.min(8))).await;
    }
    bail!("HLS segment failed: {last}")
}

async fn decrypt(
    client: &reqwest::Client,
    segment_url: &str,
    bytes: &[u8],
    key: &KeyInfo,
) -> Result<Vec<u8>> {
    let key_url = Url::parse(segment_url)?.join(&key.uri)?;
    let key_bytes = client
        .get(key_url)
        .send()
        .await?
        .error_for_status()?
        .bytes()
        .await?;
    if key_bytes.len() != 16 {
        bail!("invalid AES key length");
    }
    let iv = match &key.iv {
        Some(hex) => hex_to_bytes(hex)?,
        None => Vec::new(),
    };
    if iv.len() != 16 {
        bail!("EXT-X-KEY requires a 16-byte IV");
    }
    decrypt_cbc(&key_bytes, &iv, bytes)
}

fn decrypt_cbc(key: &[u8], iv: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        bail!("invalid AES-CBC payload length");
    }
    let cipher = Aes128::new_from_slice(key)?;
    let mut previous = [0u8; 16];
    previous.copy_from_slice(iv);
    let mut output = Vec::with_capacity(data.len());
    for block in data.as_chunks::<16>().0 {
        let mut decrypted = GenericArray::clone_from_slice(block);
        cipher.decrypt_block(&mut decrypted);
        for index in 0..16 {
            output.push(decrypted[index] ^ previous[index]);
        }
        previous.copy_from_slice(block);
    }
    let padding = *output.last().expect("payload is not empty") as usize;
    if padding == 0
        || padding > 16
        || padding > output.len()
        || output[output.len() - padding..]
            .iter()
            .any(|b| usize::from(*b) != padding)
    {
        bail!("invalid PKCS#7 padding");
    }
    output.truncate(output.len() - padding);
    Ok(output)
}

fn hex_to_bytes(hex: &str) -> Result<Vec<u8>> {
    let trimmed = hex.trim_start_matches("0x");
    if !trimmed.len().is_multiple_of(2) {
        bail!("invalid IV hex");
    }
    (0..trimmed.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&trimmed[i..i + 2], 16).map_err(Into::into))
        .collect()
}

fn select_variant(base: &str, master: &str) -> Result<String> {
    let lines: Vec<&str> = master.lines().collect();
    let mut best: Option<(u64, String)> = None;
    for (index, line) in lines.iter().enumerate() {
        if line.starts_with("#EXT-X-STREAM-INF") {
            let bandwidth = attribute(line, "BANDWIDTH")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if let Some(uri) = lines
                .get(index + 1)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && !s.starts_with('#'))
                && best.as_ref().is_none_or(|(score, _)| bandwidth > *score)
            {
                best = Some((bandwidth, resolve(base, uri)?));
            }
        }
    }
    best.map(|(_, url)| url).context("no HLS variants")
}

fn parse_range(line: &str) -> Option<(u64, u64)> {
    let value = line.strip_prefix("#EXT-X-BYTERANGE:")?;
    let (length, offset) = value
        .split_once('@')
        .map(|(length, offset)| (length, Some(offset)))
        .unwrap_or((value, None));
    let length = length.parse().ok()?;
    let offset = match offset {
        Some(offset) => offset.parse().ok()?,
        None => 0,
    };
    Some((length, offset))
}

fn parse_playlist(base: &str, playlist: &str) -> Result<(Vec<Segment>, Option<Segment>)> {
    let mut segments = Vec::new();
    let mut init = None;
    let mut current_key: Option<KeyInfo> = None;
    let mut pending_range: Option<(u64, u64)> = None;
    for line in playlist.lines().map(str::trim) {
        if let Some(range) = parse_range(line) {
            pending_range = Some(range);
        } else if line.starts_with("#EXT-X-KEY") {
            let method = attribute(line, "METHOD").unwrap_or_default();
            current_key = if method == "AES-128" {
                attribute(line, "URI").map(|uri| KeyInfo {
                    uri: resolve(base, &unquote(&uri)).unwrap_or(uri),
                    iv: attribute(line, "IV").map(|v| v.to_ascii_lowercase()),
                })
            } else {
                None
            };
        } else if line.starts_with("#EXT-X-MAP") {
            if let Some(uri) = attribute(line, "URI") {
                let range = attribute(line, "BYTERANGE")
                    .and_then(|v| parse_range(&format!("#EXT-X-BYTERANGE:{}", unquote(&v))));
                init = Some(Segment {
                    url: resolve(base, &unquote(&uri))?,
                    key: None,
                    range,
                });
            }
        } else if !line.is_empty() && !line.starts_with('#') {
            segments.push(Segment {
                url: resolve(base, line)?,
                key: current_key.clone(),
                range: pending_range.take(),
            });
        }
    }
    Ok((segments, init))
}

fn attribute(line: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=");
    let start = line.find(&marker)? + marker.len();
    let rest = &line[start..];
    Some(rest.split(',').next().unwrap_or(rest).trim().to_owned())
}

fn unquote(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}

fn resolve(base: &str, uri: &str) -> Result<String> {
    Ok(Url::parse(base)?.join(uri)?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encrypted_playlist() {
        let playlist = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x00112233445566778899aabbccddeeff\n#EXTINF:10,\nseg1.ts\n#EXTINF:10,\nseg2.ts\n";
        let (segments, _) = parse_playlist("https://cdn.example/hls/index.m3u8", playlist).unwrap();
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].url, "https://cdn.example/hls/seg1.ts");
        assert_eq!(
            segments[1].key.as_ref().unwrap().iv.as_deref(),
            Some("0x00112233445566778899aabbccddeeff")
        );
    }

    #[test]
    fn parses_fmp4_byteranges() {
        let playlist = "#EXTM3U\n#EXT-X-MAP:URI=\"main.mp4\",BYTERANGE=\"721@0\"\n#EXTINF:6,\n#EXT-X-BYTERANGE:5874288@721\nmain.mp4\n#EXTINF:6,\n#EXT-X-BYTERANGE:5863101@5875009\nmain.mp4\n";
        let (segments, init) =
            parse_playlist("https://cdn.example/hls/index.m3u8", playlist).unwrap();
        assert_eq!(init.unwrap().range, Some((721, 0)));
        assert_eq!(segments[0].range, Some((5_874_288, 721)));
        assert_eq!(segments[1].range, Some((5_863_101, 5_875_009)));
    }

    #[test]
    fn selects_highest_bandwidth_variant() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=800000\nlow.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=3000000\nhigh.m3u8\n";
        assert!(
            select_variant("https://cdn.example/master.m3u8", master)
                .unwrap()
                .ends_with("high.m3u8")
        );
    }
}
