use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    if super::is_direct_media(url) {
        return direct(url);
    }
    let raw = get_text(client, url.as_str(), &browser_headers(url.as_str())).await?;
    parse(url, &raw)
}

pub fn direct(url: &Url) -> Result<Media> {
    let name = url
        .path_segments()
        .and_then(|mut s| s.next_back())
        .filter(|s| !s.is_empty())
        .unwrap_or("asset");
    let title = percent_decode(name);
    let kind = kind_for(url.path());
    let mut format = MediaFormat::new("direct", url.as_str(), kind);
    if url.path().ends_with(".m3u8") {
        format.manifest = Some(ManifestKind::Hls);
    } else if url.path().ends_with(".mpd") {
        format.manifest = Some(ManifestKind::Dash);
    }
    let mut media = Media::new("direct", url.as_str(), title);
    media.add(format);
    Ok(media)
}

fn percent_decode(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}

fn kind_for(path: &str) -> MediaKind {
    let path = path.to_ascii_lowercase();
    if path.ends_with(".m3u8")
        || path.ends_with(".mpd")
        || path.ends_with(".mp4")
        || path.ends_with(".webm")
        || path.ends_with(".mov")
        || path.ends_with(".mkv")
    {
        MediaKind::Video
    } else if path.ends_with(".mp3")
        || path.ends_with(".m4a")
        || path.ends_with(".wav")
        || path.ends_with(".ogg")
    {
        MediaKind::Audio
    } else if path.ends_with(".srt") || path.ends_with(".vtt") {
        MediaKind::Subtitle
    } else if path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".png")
        || path.ends_with(".webp")
        || path.ends_with(".gif")
    {
        MediaKind::Image
    } else {
        MediaKind::Other
    }
}

pub fn parse(base: &Url, html: &str) -> Result<Media> {
    let document = Html::parse_document(html);
    let mut media = Media::new(
        "web",
        base.as_str(),
        meta_content(&document, "og:title", "property")
            .or_else(|| meta_content(&document, "twitter:title", "name"))
            .or_else(|| text(&document, "title"))
            .unwrap_or_else(|| "ratata-download".into()),
    );
    if let Some(author) = meta_content(&document, "og:site_name", "property") {
        media.author = Some(author);
    }
    if let Some(image) = meta_content(&document, "og:image", "property") {
        media.thumbnail = resolve(base, &image);
    }

    let mut candidates: Vec<(i32, MediaFormat)> = Vec::new();
    for property in [
        "og:video",
        "og:video:url",
        "og:video:secure_url",
        "og:video:iframe",
        "twitter:player",
        "twitter:player:stream",
    ] {
        if let Some(value) = meta_content(
            &document,
            property,
            if property.starts_with("twitter") {
                "name"
            } else {
                "property"
            },
        ) {
            push_candidate(base, &mut candidates, &value, 100);
        }
    }
    if let Some(image) = meta_content(&document, "og:image", "property") {
        push_candidate(base, &mut candidates, &image, 30);
    }
    for selector in [
        "video[src]",
        "source[src]",
        "audio[src]",
        "img[src]",
        "a[href]",
    ] {
        let sel = Selector::parse(selector).expect("static selector");
        for element in document.select(&sel) {
            let attr = if selector.starts_with("a[") {
                "href"
            } else {
                "src"
            };
            if let Some(value) = element.value().attr(attr) {
                push_candidate(base, &mut candidates, value, 10);
            }
        }
    }
    let json_ld = Selector::parse("script[type=\"application/ld+json\"]").expect("static selector");
    for element in document.select(&json_ld) {
        let raw = element.text().collect::<String>();
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            collect_json(base, &value, &mut candidates);
        }
    }

    let mut seen = std::collections::HashSet::new();
    candidates.sort_by_key(|(priority, _)| -priority);
    for (_, mut format) in candidates {
        if !seen.insert(format.url.clone()) {
            continue;
        }
        if format.kind == MediaKind::Other {
            continue;
        }
        if format.url.ends_with(".m3u8") {
            format.manifest = Some(ManifestKind::Hls);
        } else if format.url.ends_with(".mpd") {
            format.manifest = Some(ManifestKind::Dash);
        }
        format.id = format!("f{}", media.formats.len() + 1);
        media.add(format);
    }
    if media.formats.is_empty() {
        return Err(RatataError::NoFormats(base.to_string()).into());
    }
    Ok(media)
}

fn push_candidate(base: &Url, out: &mut Vec<(i32, MediaFormat)>, value: &str, priority: i32) {
    let Some(url) = resolve(base, value) else {
        return;
    };
    if url.starts_with("data:") || url.starts_with("javascript:") {
        return;
    }
    let kind = kind_for(
        &Url::parse(&url)
            .map(|u| u.path().to_owned())
            .unwrap_or_else(|_| url.clone()),
    );
    if kind == MediaKind::Other {
        return;
    }
    let mut format = MediaFormat::new(format!("f{}", out.len() + 1), url, kind);
    format.label = Some(value.chars().take(60).collect());
    out.push((priority, format));
}

fn collect_json(base: &Url, value: &Value, out: &mut Vec<(i32, MediaFormat)>) {
    match value {
        Value::Object(map) => {
            for key in ["contentUrl", "embedUrl", "url", "videoUrl", "thumbnailUrl"] {
                if let Some(Value::String(raw)) = map.get(key) {
                    push_candidate(base, out, raw, if key == "contentUrl" { 60 } else { 20 });
                }
            }
            for nested in map.values() {
                collect_json(base, nested, out);
            }
        }
        Value::Array(items) => {
            for nested in items {
                collect_json(base, nested, out);
            }
        }
        _ => {}
    }
}

fn resolve(base: &Url, value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(
        base.join(trimmed)
            .map(|u| u.to_string())
            .unwrap_or_else(|_| trimmed.to_owned()),
    )
}

fn meta_content(document: &Html, key: &str, attribute: &str) -> Option<String> {
    let selector =
        Selector::parse(&format!("meta[{attribute}=\"{key}\"]")).expect("static selector");
    document.select(&selector).find_map(|element| {
        element
            .value()
            .attr("content")
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
    })
}

fn text(document: &Html, tag: &str) -> Option<String> {
    let selector = Selector::parse(tag).ok()?;
    document
        .select(&selector)
        .next()
        .map(|e| e.text().collect::<String>().trim().to_owned())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_video_and_image_metadata() {
        let html = r#"<html><head><title>Ignored</title>
        <meta property="og:title" content="Bloom in tunnel">
        <meta property="og:video" content="/clip.mp4">
        <meta property="og:image" content="https://cdn.example/poster.jpg">
        </head><body><source src="https://cdn.example/alt.webm"></body></html>"#;
        let base = Url::parse("https://example.com/post").unwrap();
        let media = parse(&base, html).unwrap();
        assert_eq!(media.title, "Bloom in tunnel");
        assert_eq!(media.formats.len(), 3);
        assert_eq!(media.formats[0].url, "https://example.com/clip.mp4");
        assert_eq!(
            media.thumbnail.as_deref(),
            Some("https://cdn.example/poster.jpg")
        );
    }

    #[test]
    fn detects_hls_manifest() {
        let html = r#"<video src="https://example.com/master.m3u8"></video>"#;
        let base = Url::parse("https://example.com/").unwrap();
        let media = parse(&base, html).unwrap();
        assert_eq!(media.formats[0].manifest, Some(ManifestKind::Hls));
    }
}
