use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let id = super::require_video_id(url, &["status", "statuses"])?;
    let headers = browser_headers("https://x.com/");
    let token = format!(
        "{:x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let syndication =
        format!("https://cdn.syndication.twimg.com/tweet-result?id={id}&lang=en&token={token}");
    if let Ok(json) = get_json(client, &syndication, &headers).await
        && let Some(media) = parse_syndication(&json, url, &id)
    {
        return Ok(media);
    }
    for endpoint in [
        format!("https://api.fxtwitter.com/status/{id}"),
        format!("https://api.vxtwitter.com/Twitter/status/{id}"),
    ] {
        if let Ok(json) = get_json(client, &endpoint, &headers).await
            && let Some(media) = parse_fx(&json, url, &id)
        {
            return Ok(media);
        }
    }
    Err(RatataError::Extraction(format!("no media found in X post {id}")).into())
}

fn parse_syndication(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let mut media = Media::new(
        "X",
        url.to_string(),
        json["text"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = json["user"]["name"].as_str().map(str::to_owned);

    if let Some(variants) = json["video"]["variants"].as_array() {
        add_variants(&mut media, variants);
    }
    if let Some(details) = json["mediaDetails"].as_array() {
        for detail in details {
            if let Some(variants) = detail["video_info"]["variants"].as_array() {
                add_variants(&mut media, variants);
            }
            if let Some(image) = detail["media_url_https"].as_str() {
                media.add(MediaFormat::new(
                    format!("i{}", media.formats.len() + 1),
                    image,
                    MediaKind::Image,
                ));
            }
        }
    }
    if let Some(photo) = json["photo"].as_str() {
        media.add(MediaFormat::new(
            format!("i{}", media.formats.len() + 1),
            photo,
            MediaKind::Image,
        ));
    }
    if media.formats.is_empty() {
        None
    } else {
        Some(media)
    }
}

fn add_variants(media: &mut Media, variants: &[Value]) {
    for variant in variants {
        let Some(url) = variant["url"].as_str() else {
            continue;
        };
        if !url.contains("video.twimg.com") && !url.ends_with(".mp4") && !url.ends_with(".m3u8") {
            continue;
        }
        let mut format = MediaFormat::new(
            format!("v{}", media.formats.len() + 1),
            url,
            MediaKind::Video,
        );
        format.bitrate = variant["bitrate"].as_u64();
        format.label = variant["quality"].as_str().map(str::to_owned);
        if url.ends_with(".m3u8") {
            format.label = Some("HLS".into());
        }
        media.add(format);
    }
    // Prefer progressive MP4 variants and higher bitrates.
    media
        .formats
        .sort_by_key(|f| std::cmp::Reverse(f.bitrate.unwrap_or(0)));
}

fn parse_fx(json: &Value, url: &Url, id: &str) -> Option<Media> {
    let tweet = if json["tweet"].is_object() {
        &json["tweet"]
    } else {
        json
    };
    let mut media = Media::new(
        "X",
        url.to_string(),
        tweet["text"]
            .as_str()
            .unwrap_or(id)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = id.into();
    media.author = tweet["author"]["name"]
        .as_str()
        .or_else(|| tweet["user_name"].as_str())
        .map(str::to_owned);
    if let Some(videos) = tweet["media"]["videos"].as_array() {
        for video in videos {
            if let Some(url) = video["url"].as_str() {
                let mut format = MediaFormat::new(
                    format!("v{}", media.formats.len() + 1),
                    url,
                    MediaKind::Video,
                );
                format.bitrate = video["bitrate"].as_u64();
                format.size = video["size"].as_u64();
                media.add(format);
            }
        }
    }
    if let Some(images) = tweet["media"]["images"].as_array() {
        for image in images {
            if let Some(url) = image["url"].as_str() {
                media.add(MediaFormat::new(
                    format!("i{}", media.formats.len() + 1),
                    url,
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
    fn parses_syndication_variants() {
        let json: Value = serde_json::from_str(r#"{
          "text":"Blooming tunnel","user":{"name":"Aarav"},
          "video":{"variants":[{"url":"https://video.twimg.com/a.mp4","bitrate":832000},{"url":"https://video.twimg.com/b.mp4","bitrate":2176000}]},
          "mediaDetails":[{"media_url_https":"https://pbs.twimg.com/p.jpg"}]
        }"#).unwrap();
        let url = Url::parse("https://x.com/a/status/123").unwrap();
        let media = parse_syndication(&json, &url, "123").unwrap();
        assert_eq!(media.author.as_deref(), Some("Aarav"));
        assert_eq!(media.formats.len(), 3);
        assert_eq!(media.formats[0].bitrate, Some(2_176_000));
    }
}
