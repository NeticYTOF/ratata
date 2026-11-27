use crate::http::{browser_headers, headers_map};
use anyhow::{Context, Result, bail};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Default)]
pub struct AudioMeta {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub year: Option<u32>,
    pub track: Option<u32>,
    pub artwork: Option<String>,
}

pub fn ffmpeg_or_error(configured: Option<&str>) -> Result<PathBuf> {
    super::ffmpeg_available(configured).ok_or_else(|| {
        anyhow::anyhow!("MP3 conversion needs ffmpeg, which was not found on this computer")
    })
}

pub async fn convert_to_mp3(
    client: &Client,
    input: &Path,
    meta: &AudioMeta,
    ffmpeg: &Path,
) -> Result<PathBuf> {
    if !input.exists() {
        bail!("audio file is missing: {}", input.display());
    }
    let output = input.with_extension("mp3");
    let cover = if let Some(url) = &meta.artwork {
        download_cover(client, url).await.ok()
    } else {
        None
    };
    let mut command = Command::new(ffmpeg);
    command.arg("-y").arg("-i").arg(input);
    if let Some(cover) = &cover {
        command.arg("-i").arg(cover);
    }
    command
        .arg("-map")
        .arg("0:a:0")
        .arg("-c:a")
        .arg("libmp3lame")
        .arg("-b:a")
        .arg("192k")
        .arg("-id3v2_version")
        .arg("3")
        .arg("-metadata")
        .arg(format!("title={}", meta.title))
        .arg("-metadata")
        .arg(format!("artist={}", meta.artist))
        .arg("-metadata")
        .arg(format!("album={}", meta.album));
    if let Some(year) = meta.year {
        command.arg("-metadata").arg(format!("date={year}"));
    }
    if let Some(track) = meta.track {
        command.arg("-metadata").arg(format!("track={track}"));
    }
    if cover.is_some() {
        command
            .arg("-map")
            .arg("1:v:0")
            .arg("-c:v")
            .arg("copy")
            .arg("-disposition:v:0")
            .arg("attached_pic");
    }
    command.arg(&output);
    let result = command
        .output()
        .with_context(|| format!("running {} for MP3 conversion", ffmpeg.display()))?;
    if !result.status.success() {
        bail!(
            "ffmpeg could not convert audio: {}",
            String::from_utf8_lossy(&result.stderr)
                .chars()
                .take(400)
                .collect::<String>()
        );
    }
    if let Some(cover) = cover {
        let _ = tokio::fs::remove_file(cover).await;
    }
    Ok(output)
}

async fn download_cover(client: &Client, url: &str) -> Result<PathBuf> {
    let response = client
        .get(url)
        .headers(headers_map(&browser_headers("https://music.apple.com/"))?)
        .send()
        .await?;
    let bytes = response.error_for_status()?.bytes().await?;
    let path = std::env::temp_dir().join(format!("ratata-cover-{}.jpg", std::process::id()));
    tokio::fs::write(&path, &bytes).await?;
    Ok(path)
}

pub fn friendly_missing() -> String {
    "Install ffmpeg (https://ffmpeg.org) and try again, or use --audio-only for the original audio track.".into()
}
