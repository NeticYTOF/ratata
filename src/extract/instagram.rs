use crate::error::RatataError;
use crate::http::{browser_headers, get_json, get_text};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::{Context, Result};
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let shortcode = super::require_video_id(url, &["p", "reel", "reels", "tv"])?;
    let media_id = decode_shortcode(&shortcode);
    let mut media = Media::new(
        "Instagram",
        url.to_string(),
        format!("Instagram {shortcode}"),
    );
    media.id = shortcode.clone();

    if let Some(id) = media_id {
        let endpoint = format!("https://www.instagram.com/api/v1/media/{id}/info/");
        let mut headers = browser_headers("https://www.instagram.com/");
        headers.push(("X-IG-App-ID".into(), "936619743392459".into()));
        if let Ok(info) = get_json(client, &endpoint, &headers).await
            && let Some(parsed) = parse_api(&info, url, &shortcode)
        {
            return Ok(parsed);
        }
    }

    let embed_url = format!("https://www.instagram.com/p/{shortcode}/embed/captioned/");
    let html = get_text(
        client,
        &embed_url,
        &browser_headers("https://www.instagram.com/"),
    )
    .await
    .context("fetching Instagram embed")?;
    parse_embed(&html, url, &shortcode, media)
}

fn decode_shortcode(code: &str) -> Option<u64> {
    let mut id = 0u64;
    for byte in code.bytes() {
        let index = ALPHABET.iter().position(|c| *c == byte)? as u64;
        id = id.checked_mul(64)?.checked_add(index)?;
    }
    let padding_bits = ((4 - code.len() % 4) % 4) * 2;
    Some(id >> padding_bits)
}

fn parse_api(info: &Value, url: &Url, shortcode: &str) -> Option<Media> {
    let item = &info["items"][0];
    if item.is_null() {
        return None;
    }
    let mut media = Media::new("Instagram", url.to_string(), caption(item, shortcode));
    media.id = shortcode.into();
    media.author = item["user"]["username"].as_str().map(str::to_owned);
    media.duration = item["video_duration"].as_f64();
    media.thumbnail = item["image_versions2"]["candidates"][0]["url"]
        .as_str()
        .map(str::to_owned);
    add_item(&mut media, item);
    if let Some(carousel) = item["carousel_media"].as_array() {
        for child in carousel {
            add_item(&mut media, child);
        }
    }
    if media.formats.is_empty() {
        return None;
    }
    Some(media)
}

fn add_item(media: &mut Media, item: &Value) {
    if let Some(videos) = item["video_versions"].as_array() {
        for video in videos {
            let Some(url) = video["url"].as_str() else {
                continue;
            };
            let mut format = MediaFormat::new(
                format!("v{}", media.formats.len() + 1),
                url,
                MediaKind::Video,
            );
            format.width = video["width"].as_u64().map(|v| v as u32);
            format.height = video["height"].as_u64().map(|v| v as u32);
            format.codec = video["type"].as_u64().map(|t| t.to_string());
            format.size = video["file_size"]
                .as_u64()
                .or_else(|| video["fileSize"].as_u64());
            format.label = Some(format!("IG video {}", format.id));
            media.add(format);
        }
    }
    if let Some(images) = item["image_versions2"]["candidates"].as_array()
        && let Some(image) = images
            .iter()
            .max_by_key(|i| i["width"].as_u64().unwrap_or(0))
        && let Some(url) = image["url"].as_str()
    {
        let mut format = MediaFormat::new(
            format!("i{}", media.formats.len() + 1),
            url,
            MediaKind::Image,
        );
        format.width = image["width"].as_u64().map(|v| v as u32);
        format.height = image["height"].as_u64().map(|v| v as u32);
        media.add(format);
    }
    if let Some(audio) = item["audio"].as_str() {
        media.add(MediaFormat::new(
            format!("a{}", media.formats.len() + 1),
            audio,
            MediaKind::Audio,
        ));
    }
}

fn caption(item: &Value, shortcode: &str) -> String {
    item["caption"]
        .as_str()
        .or_else(|| item["caption"]["text"].as_str())
        .map(|s| s.chars().take(120).collect())
        .filter(|s: &String| !s.trim().is_empty())
        .unwrap_or_else(|| format!("Instagram {shortcode}"))
}

fn parse_embed(html: &str, url: &Url, _shortcode: &str, mut fallback: Media) -> Result<Media> {
    let document = Html::parse_document(html);
    let meta = |key: &str| -> Option<String> {
        let selector = Selector::parse(&format!("meta[property=\"{key}\"]")).ok()?;
        document
            .select(&selector)
            .next()?
            .value()
            .attr("content")
            .map(str::to_owned)
    };
    if let Some(title) = meta("og:title").or_else(|| meta("og:description"))
        && !title.trim().is_empty()
    {
        fallback.title = title;
    }
    for key in ["og:video", "og:video:url", "og:video:secure_url"] {
        if let Some(video) = meta(key) {
            fallback.add(MediaFormat::new(
                format!("v{}", fallback.formats.len() + 1),
                video,
                MediaKind::Video,
            ));
            break;
        }
    }
    if let Some(image) = meta("og:image") {
        fallback.thumbnail = Some(image.clone());
        if fallback.formats.is_empty() {
            fallback.add(MediaFormat::new("i1", image, MediaKind::Image));
        }
    }
    if fallback.formats.is_empty() {
        return Err(RatataError::NoFormats(url.to_string()).into());
    }
    Ok(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_known_shortcode() {
        assert_eq!(
            decode_shortcode("BXYzNlQFYqb"),
            Some(393_558_325_872_386_726)
        );
    }

    #[test]
    fn parses_api_video_and_carousel() {
        let data: Value = serde_json::from_str(r#"{"items":[{
            "user":{"username":"bloom"},"video_duration":12.5,
            "caption":{"text":"Flowers in a tunnel"},
            "image_versions2":{"candidates":[{"url":"https://cdn/thumb.jpg","width":1080}]},
            "video_versions":[{"url":"https://cdn/v.mp4","width":1080,"height":1920,"type":101}],
            "carousel_media":[{"image_versions2":{"candidates":[{"url":"https://cdn/pic.jpg","width":1080}]}}]
        }]}"#).unwrap();
        let url = Url::parse("https://www.instagram.com/reel/abc/").unwrap();
        let media = parse_api(&data, &url, "abc").unwrap();
        assert_eq!(media.title, "Flowers in a tunnel");
        assert_eq!(media.author.as_deref(), Some("bloom"));
        assert_eq!(media.formats.len(), 3);
    }
}
