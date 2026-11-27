use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
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
    let json = get_json(
        client,
        &endpoint,
        &browser_headers("https://www.dailymotion.com/"),
    )
    .await?;
    parse(&json, url, id).ok_or_else(|| RatataError::NoFormats(url.to_string()).into())
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
