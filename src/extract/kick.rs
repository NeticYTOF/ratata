use crate::error::RatataError;
use crate::http::{browser_headers, get_json, get_text};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    let (kind, id) = if segments.first().is_some_and(|s| *s == "video") {
        ("videos", segments.get(1).copied())
    } else if segments.first().is_some_and(|s| *s == "clip") {
        ("clips", segments.get(1).copied())
    } else {
        ("videos", segments.first().copied())
    };
    let id = id.ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let endpoint = format!("https://kick.com/api/v1/{kind}/{id}");
    let mut headers = browser_headers("https://kick.com/");
    headers.push(("Accept".into(), "application/json, text/plain, */*".into()));
    if let Ok(json) = get_json(client, &endpoint, &headers).await
        && let Some(media) = parse(&json, url, id)
    {
        return Ok(media);
    }
    let page = get_text(client, url.as_str(), &headers)
        .await
        .unwrap_or_default();
    super::generic::parse(url, &page).context("Kick page had no playable media")
}

fn parse(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let mut media = Media::new(
        "Kick",
        url.to_string(),
        json["title"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = json["channel"]["name"].as_str().map(str::to_owned);
    media.duration = json["duration"].as_f64();
    media.thumbnail = json["thumbnail"].as_str().map(str::to_owned);
    for key in ["source", "clip_url"] {
        if let Some(address) = json[key].as_str() {
            let mut format = MediaFormat::new(key, address, MediaKind::Video);
            if address.ends_with(".m3u8") {
                format.manifest = Some(ManifestKind::Hls);
            }
            media.add(format);
        }
    }
    if let Some(files) = json["sources"].as_array() {
        for (index, file) in files.iter().enumerate() {
            if let Some(address) = file["url"].as_str() {
                media.add(MediaFormat::new(
                    format!("s{index}"),
                    address,
                    MediaKind::Video,
                ));
            }
        }
    }
    if media.formats.is_empty() {
        None
    } else {
        Some(media)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kick_video_source() {
        let json: Value = serde_json::from_str(r#"{"title":"Tunnel stream","duration":300,"channel":{"name":"grower"},"source":"https://stream/kick/master.m3u8"}"#).unwrap();
        let url = Url::parse("https://kick.com/grower/videos/123").unwrap();
        let media = parse(&json, &url, "123").unwrap();
        assert_eq!(media.formats[0].manifest, Some(ManifestKind::Hls));
        assert_eq!(media.author.as_deref(), Some("grower"));
    }
}
