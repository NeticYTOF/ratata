use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let code = url
        .path_segments()
        .and_then(|mut segments| segments.find(|s| !s.is_empty()))
        .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let endpoint = format!("https://api.streamable.com/videos/{code}");
    let json = get_json(
        client,
        &endpoint,
        &browser_headers("https://streamable.com/"),
    )
    .await?;
    parse(&json, url, code).ok_or_else(|| RatataError::NoFormats(url.to_string()).into())
}

fn parse(json: &Value, url: &Url, code: &str) -> Option<Media> {
    let mut media = Media::new(
        "Streamable",
        url.to_string(),
        json["title"]
            .as_str()
            .unwrap_or(code)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = code.into();
    media.duration = json["duration"].as_f64();
    media.thumbnail = json["thumbnail_url"].as_str().map(str::to_owned);
    if let Some(files) = json["files"].as_object() {
        for (name, file) in files {
            let Some(address) = file["url"].as_str() else {
                continue;
            };
            let address = normalize_streamable_url(address);
            let mut format = MediaFormat::new(name, address, MediaKind::Video);
            format
                .headers
                .push(("Referer".into(), "https://streamable.com/".into()));
            format.headers.push((
                "User-Agent".into(),
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/131.0 Safari/537.36".into(),
            ));
            media.add(format);
        }
    }
    media
        .formats
        .sort_by_key(|f| std::cmp::Reverse(f.quality()));
    if media.formats.is_empty() {
        None
    } else {
        Some(media)
    }
}

fn normalize_streamable_url(raw: &str) -> String {
    let value = crate::http::normalize_media_url(raw);
    if value.starts_with('/') {
        return format!("https://streamable.com{value}");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_cdn_url_variants() {
        assert_eq!(
            normalize_streamable_url("https:https://cdn-cf-west.streamable.com/video/a.mp4?x=1"),
            "https://cdn-cf-west.streamable.com/video/a.mp4?x=1"
        );
        assert_eq!(
            normalize_streamable_url("https//cdn-cf-west.streamable.com/video/a.mp4"),
            "https://cdn-cf-west.streamable.com/video/a.mp4"
        );
        assert_eq!(
            normalize_streamable_url("//cdn.streamable.com/a.mp4"),
            "https://cdn.streamable.com/a.mp4"
        );
        assert_eq!(
            normalize_streamable_url("/video/a.mp4"),
            "https://streamable.com/video/a.mp4"
        );
        assert_eq!(
            normalize_streamable_url("https://cdn.streamable.com/a.mp4"),
            "https://cdn.streamable.com/a.mp4"
        );
    }

    #[test]
    fn picks_largest_streamable_file() {
        let json: Value = serde_json::from_str(r#"{
          "title":"Petals","duration":8,
          "files":{"mp4":{"url":"https://cdn/a.mp4","width":640,"height":360,"filesize":1000},
                   "mp4-1080p":{"url":"https://cdn/b.mp4","width":1920,"height":1080,"filesize":4000}}
        }"#).unwrap();
        let url = Url::parse("https://streamable.com/abcd").unwrap();
        let media = parse(&json, &url, "abcd").unwrap();
        assert_eq!(media.best_video().unwrap().id, "mp4-1080p");
    }
}
