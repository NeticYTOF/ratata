use crate::error::RatataError;
use crate::http::{browser_headers, get_json, get_text};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use url::Url;

/// Public player metadata endpoint; works without credentials.
pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let id = url
        .path_segments()
        .and_then(|segments| {
            segments
                .filter(|s| !s.is_empty())
                .find(|s| s.chars().all(|c| c.is_ascii_alphanumeric()))
        })
        .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let endpoint = format!("https://www.dailymotion.com/player/metadata/video/{id}");
    let mut headers = browser_headers("https://www.dailymotion.com/");
    headers.retain(|(key, _)| key != "Referer");
    headers.push(("Referer".into(), url.to_string()));
    if let Ok(json) = get_json(client, &endpoint, &headers).await
        && let Some(media) = parse(&json, url, id)
    {
        return Ok(media);
    }
    let embed = format!("https://www.dailymotion.com/embed/video/{id}");
    let page = get_text(client, &embed, &headers).await.unwrap_or_default();
    let mut media = Media::new("Dailymotion", url.to_string(), id);
    let mut seen = std::collections::HashSet::new();
    if let Ok(regex) = Regex::new(r#"https?:[^"\s<>]+\.(?:mp4|m3u8)[^"\s<>]*"#) {
        for capture in regex.captures_iter(&page) {
            let Some(raw) = capture.get(0) else { continue };
            let decoded = raw
                .as_str()
                .replace("\\/", "/")
                .replace("https//", "https://");
            if decoded.starts_with("http") && seen.insert(decoded.clone()) {
                let mut format = MediaFormat::new("video", decoded.clone(), MediaKind::Video);
                if decoded.contains(".m3u8") {
                    format.manifest = Some(ManifestKind::Hls);
                }
                media.add(format);
            }
        }
    }
    if media.formats.is_empty() {
        return Err(RatataError::NoFormats(url.to_string()).into());
    }
    Ok(media)
}

fn parse(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let mut media = Media::new(
        "Dailymotion",
        url.to_string(),
        json["title"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = json["owner"]["screenname"].as_str().map(str::to_owned);
    media.duration = json["duration"].as_f64();
    media.thumbnail = json["posters"]
        .as_object()
        .and_then(|m| m.values().next_back())
        .and_then(Value::as_str)
        .map(str::to_owned);

    if let Some(qualities) = json["qualities"].as_object() {
        for (quality, entries) in qualities {
            for entry in entries.as_array().into_iter().flatten() {
                let Some(address) = entry["url"].as_str() else {
                    continue;
                };
                let mut format = MediaFormat::new(quality, address, MediaKind::Video);
                format.label = Some(quality.clone());
                if address.ends_with(".m3u8") {
                    format.manifest = Some(ManifestKind::Hls);
                }
                media.add(format);
            }
        }
    }
    if media.formats.is_empty()
        && let Some(address) = find_media_url(json)
    {
        let mut format = MediaFormat::new("video", address.clone(), MediaKind::Video);
        if address.contains(".m3u8") {
            format.manifest = Some(ManifestKind::Hls);
        }
        media.add(format);
    }
    if media.formats.is_empty() {
        None
    } else {
        Some(media)
    }
}

fn find_media_url(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) if raw.contains(".mp4") || raw.contains(".m3u8") => Some(raw.clone()),
        Value::Object(map) => map.values().find_map(find_media_url),
        Value::Array(items) => items.iter().find_map(find_media_url),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quality_variants() {
        let json: Value = serde_json::from_str(
            r#"{
          "title":"Bloom tunnel","duration":90,"owner":{"screenname":"garden"},
          "qualities":{"1080":[{"url":"https://proxy/high.mp4","type":"video/mp4"}],
                       "720":[{"url":"https://proxy/low.m3u8","type":"application/x-mpegURL"}]}
        }"#,
        )
        .unwrap();
        let url = Url::parse("https://www.dailymotion.com/video/x123").unwrap();
        let media = parse(&json, &url, "x123").unwrap();
        assert_eq!(media.title, "Bloom tunnel");
        assert_eq!(media.formats.len(), 2);
        assert!(
            media
                .formats
                .iter()
                .any(|f| f.manifest == Some(ManifestKind::Hls))
        );
    }
}
