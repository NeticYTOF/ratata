use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let id = url
        .path_segments()
        .and_then(|s| {
            s.filter(|p| !p.is_empty())
                .find(|p| p.chars().all(|c| c.is_ascii_digit()))
        })
        .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let endpoint = format!("https://player.vimeo.com/video/{id}/config");
    let json = get_json(client, &endpoint, &browser_headers("https://vimeo.com/")).await?;
    parse_config(&json, url, id)
        .ok_or_else(|| RatataError::Extraction(format!("no Vimeo files for {id}")).into())
}

fn parse_config(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let video = &json["video"];
    let mut media = Media::new(
        "Vimeo",
        url.to_string(),
        video["title"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = video["owner"]["name"].as_str().map(str::to_owned);
    media.duration = video["duration"].as_f64();
    media.thumbnail = video["thumbs"]
        .as_object()
        .and_then(|m| m.values().next_back())
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    if let Some(progressive) = json["request"]["files"]["progressive"].as_array() {
        for file in progressive {
            let Some(address) = file["url"].as_str() else {
                continue;
            };
            let mut format = MediaFormat::new(
                format!("p{}", media.formats.len() + 1),
                address,
                MediaKind::Video,
            );
            format.width = file["width"].as_u64().map(|v| v as u32);
            format.height = file["height"].as_u64().map(|v| v as u32);
            format.codec = file["codec"].as_str().map(str::to_owned);
            add_vimeo_headers(&mut format);
            media.add(format);
        }
    }
    for (key, label, suffix) in [("hls", "HLS", ".m3u8"), ("dash", "DASH", ".mpd")] {
        let section = &json["request"]["files"][key];
        let mut addresses = find_manifest_urls(section, suffix);
        if let Some(fallback) = section["cdn_url"]
            .as_str()
            .or_else(|| section["url"].as_str())
        {
            addresses.push(fallback.to_owned());
        }
        addresses.sort();
        addresses.dedup();
        for (index, address) in addresses.into_iter().enumerate() {
            let mut format = MediaFormat::new(
                format!("{}{}", label.to_ascii_lowercase(), index),
                address,
                MediaKind::Video,
            );
            format.manifest = Some(if key == "hls" {
                ManifestKind::Hls
            } else {
                ManifestKind::Dash
            });
            format.label = Some(label.into());
            add_vimeo_headers(&mut format);
            media.add(format);
        }
    }
    if media.formats.is_empty() {
        None
    } else {
        Some(media)
    }
}

fn add_vimeo_headers(format: &mut MediaFormat) {
    format
        .headers
        .push(("Referer".into(), "https://vimeo.com/".into()));
    format.headers.push((
        "User-Agent".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Safari/537.36"
            .into(),
    ));
}

fn find_manifest_urls(value: &Value, suffix: &str) -> Vec<String> {
    let mut found = Vec::new();
    match value {
        Value::String(raw) if raw.contains(suffix) => found.push(raw.clone()),
        Value::Object(map) => {
            for nested in map.values() {
                found.extend(find_manifest_urls(nested, suffix));
            }
        }
        Value::Array(items) => {
            for nested in items {
                found.extend(find_manifest_urls(nested, suffix));
            }
        }
        _ => {}
    }
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progressive_and_hls() {
        let json: Value = serde_json::from_str(r#"{
          "video":{"title":"Tunnel","duration":42,"owner":{"name":"Studio"}},
          "request":{"files":{
            "progressive":[{"url":"https://vod/progressive.mp4","width":1920,"height":1080,"codec":"h264"}],
            "hls":{"cdn_url":"https://vod/master.m3u8"}
          }}
        }"#).unwrap();
        let url = Url::parse("https://vimeo.com/12345").unwrap();
        let media = parse_config(&json, &url, "12345").unwrap();
        assert_eq!(media.title, "Tunnel");
        assert_eq!(media.formats.len(), 2);
        assert_eq!(media.formats[1].manifest, Some(ManifestKind::Hls));
    }

    #[test]
    fn finds_manifest_inside_cdn_map() {
        let json: Value = serde_json::from_str(r#"{
          "video":{"title":"Tunnel"},
          "request":{"files":{"dash":{"cdns":{"akfire":{"avc_url":"https://vod/playlist.mpd?token=1"}}}}}
        }"#).unwrap();
        let url = Url::parse("https://vimeo.com/42").unwrap();
        let media = parse_config(&json, &url, "42").unwrap();
        assert_eq!(media.formats[0].manifest, Some(ManifestKind::Dash));
        assert!(media.formats[0].url.contains("playlist.mpd"));
    }
}
