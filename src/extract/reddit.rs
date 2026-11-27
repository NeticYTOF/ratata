use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let id = post_id(url)?;
    let headers = browser_headers("https://www.reddit.com/");
    let endpoints = [
        format!("https://www.reddit.com/comments/{id}.json?raw_json=1"),
        format!("https://api.reddit.com/comments/{id}?raw_json=1"),
        format!("https://old.reddit.com/comments/{id}.json?raw_json=1"),
    ];
    let mut last_error = String::new();
    for endpoint in endpoints {
        match get_json(client, &endpoint, &headers).await {
            Ok(json) => {
                if let Some(media) = parse_listing(&json, url, &id) {
                    return Ok(media);
                }
                last_error = "no media in response".into();
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(RatataError::Extraction(format!("Reddit post {id}: {last_error}")).into())
}

fn post_id(url: &Url) -> Result<String> {
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    if url.host_str() == Some("redd.it") {
        return segments
            .first()
            .map(|s| s.to_string())
            .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()).into());
    }
    if let Some(index) = segments.iter().position(|s| *s == "comments")
        && let Some(id) = segments.get(index + 1)
    {
        return Ok((*id).to_owned());
    }
    Err(RatataError::UnsupportedUrl(url.to_string()).into())
}

fn parse_listing(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let post = &json.as_array()?.first()?["data"]["children"][0]["data"];
    if post.is_null() {
        return None;
    }
    let mut media = Media::new(
        "Reddit",
        url.to_string(),
        post["title"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = post["author"].as_str().map(str::to_owned);
    media.duration = post["media"]["reddit_video"]["duration"].as_f64();
    if let Some(video) = post["media"]["reddit_video"]["fallback_url"].as_str() {
        let mut format = MediaFormat::new("v1", video, MediaKind::Video);
        format.width = post["media"]["reddit_video"]["width"]
            .as_u64()
            .map(|v| v as u32);
        format.height = post["media"]["reddit_video"]["height"]
            .as_u64()
            .map(|v| v as u32);
        media.add(format);
    }
    if let Some(image) = post["preview"]["images"][0]["source"]["url"].as_str() {
        media.thumbnail = Some(image.to_owned());
        media.add(MediaFormat::new("i1", image, MediaKind::Image));
    }
    if let Some(url) = post["url_overridden_by_dest"].as_str()
        && (url.ends_with(".mp4")
            || url.ends_with(".webm")
            || url.ends_with(".jpg")
            || url.ends_with(".png"))
    {
        media.add(MediaFormat::new(
            format!("d{}", media.formats.len() + 1),
            url,
            MediaKind::Video,
        ));
    }
    if let Some(metadata) = post["media_metadata"].as_object() {
        for (index, item) in metadata.values().enumerate() {
            let source = item["s"]["u"]
                .as_str()
                .or_else(|| item["s"]["gif"].as_str());
            if let Some(source) = source {
                media.add(MediaFormat::new(
                    format!("g{}", index + 1),
                    source,
                    MediaKind::Image,
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
    fn parses_reddit_video_post() {
        let json: Value = serde_json::from_str(r#"[{"data":{"children":[{"data":{
          "title":"Blooms","author":"gardener",
          "media":{"reddit_video":{"fallback_url":"https://v.redd.it/x/DASH_720.mp4","width":1280,"height":720,"duration":18}},
          "preview":{"images":[{"source":{"url":"https://preview.redd.it/p.jpg"}}]}
        }}]}}]"#).unwrap();
        let url = Url::parse("https://www.reddit.com/r/test/comments/abc123/blooms/").unwrap();
        let media = parse_listing(&json, &url, "abc123").unwrap();
        assert_eq!(media.title, "Blooms");
        assert_eq!(media.formats.len(), 2);
        assert_eq!(media.duration, Some(18.0));
    }
}
