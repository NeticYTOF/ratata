pub mod dailymotion;
pub mod galleries;
pub mod generic;
pub mod instagram;
pub mod reddit;
pub mod soundcloud;
pub mod streamable;
pub mod tiktok;
pub mod twitch;
pub mod twitter;
pub mod vimeo;
pub mod youtube;
pub mod yt_cipher;

use crate::config::Config;
use crate::error::RatataError;
use crate::model::Media;
use anyhow::{Context, Result};
use reqwest::Client;
use url::Url;

pub async fn extract(url: &str, client: &Client, config: &Config) -> Result<Media> {
    let parsed = Url::parse(url).with_context(|| format!("parsing URL {url}"))?;
    let raw_host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = raw_host
        .strip_prefix("www.")
        .or_else(|| raw_host.strip_prefix("m."))
        .unwrap_or(&raw_host);

    if host.contains("youtube.com") || host == "youtu.be" || host == "yt.be" {
        return youtube::extract(&parsed, client, config).await;
    }
    if host == "instagram.com" || host.ends_with(".instagram.com") {
        return instagram::extract(&parsed, client).await;
    }
    if host == "twitter.com" || host == "x.com" || host.ends_with(".twimg.com") {
        return twitter::extract(&parsed, client).await;
    }
    if host.ends_with("tiktok.com") {
        return tiktok::extract(&parsed, client).await;
    }
    if host == "reddit.com" || host.ends_with(".reddit.com") || host == "redd.it" {
        return reddit::extract(&parsed, client).await;
    }
    if host == "vimeo.com" || host == "player.vimeo.com" {
        return vimeo::extract(&parsed, client).await;
    }
    if host == "dailymotion.com" || host == "dai.ly" {
        return dailymotion::extract(&parsed, client).await;
    }
    if host == "streamable.com" {
        return streamable::extract(&parsed, client).await;
    }
    if host.ends_with("twitch.tv") {
        return twitch::extract(&parsed, client).await;
    }
    if host == "soundcloud.com" || host.ends_with(".soundcloud.com") {
        return soundcloud::extract(&parsed, client).await;
    }
    if host == "imgur.com" || host.ends_with(".imgur.com") {
        return galleries::imgur(&parsed, client).await;
    }
    if host == "pinterest.com" || host.ends_with(".pinterest.com") {
        return galleries::pinterest(&parsed, client).await;
    }
    generic::extract(&parsed, client).await
}

pub fn is_direct_media(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    [
        ".mp4", ".webm", ".mov", ".m4v", ".mkv", ".mp3", ".m4a", ".wav", ".ogg", ".jpg", ".jpeg",
        ".png", ".webp", ".gif", ".srt", ".vtt", ".m3u8", ".mpd",
    ]
    .iter()
    .any(|ext| path.ends_with(ext))
}

pub fn require_video_id(parsed: &Url, patterns: &[&str]) -> Result<String> {
    let segments: Vec<&str> = parsed
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    for pattern in patterns {
        if let Some(index) = segments.iter().position(|s| *s == *pattern)
            && let Some(id) = segments.get(index + 1)
        {
            let id = id.split('?').next().unwrap_or(id);
            if !id.is_empty() {
                return Ok(id.to_string());
            }
        }
    }
    Err(RatataError::UnsupportedUrl(parsed.to_string()).into())
}
