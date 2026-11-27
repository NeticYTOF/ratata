use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

pub async fn imgur(url: &Url, client: &Client) -> Result<Media> {
    let id = url
        .path_segments()
        .and_then(|mut segments| segments.find(|s| !s.is_empty()))
        .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let embed = format!("https://imgur.com/{id}/embed");
    let html = get_text(client, &embed, &browser_headers("https://imgur.com/")).await?;
    parse(&html, url, "Imgur", id)
}

pub async fn pinterest(url: &Url, client: &Client) -> Result<Media> {
    let html = get_text(
        client,
        url.as_str(),
        &browser_headers("https://www.pinterest.com/"),
    )
    .await?;
    parse(&html, url, "Pinterest", "pin")
}

fn parse(html: &str, url: &Url, platform: &str, fallback_title: &str) -> Result<Media> {
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
    let mut media = Media::new(
        platform,
        url.to_string(),
        meta("og:title").unwrap_or_else(|| fallback_title.to_owned()),
    );
    media.thumbnail = meta("og:image").clone();
    let mut seen = std::collections::HashSet::new();
    let mut add = |value: String, kind: MediaKind| {
        let value = if kind == MediaKind::Image {
            full_size(&value)
        } else {
            value
        };
        if seen.insert(value.clone())
            && (value.starts_with("http://") || value.starts_with("https://"))
        {
            media.add(MediaFormat::new(
                format!("f{}", media.formats.len() + 1),
                value,
                kind,
            ));
        }
    };
    for key in ["og:video", "og:video:secure_url", "og:video:url"] {
        if let Some(value) = meta(key) {
            add(value, MediaKind::Video);
        }
    }
    if let Some(image) = meta("og:image") {
        add(image, MediaKind::Image);
    }
    for selector in ["video[src]", "source[src]", "img[src]"] {
        if let Ok(rule) = Selector::parse(selector) {
            for element in document.select(&rule) {
                if let Some(value) = element.value().attr("src")
                    && (value.contains("imgur")
                        || value.contains("pinimg")
                        || value.ends_with(".mp4"))
                {
                    add(
                        value.to_owned(),
                        if value.ends_with(".mp4") {
                            MediaKind::Video
                        } else {
                            MediaKind::Image
                        },
                    );
                }
            }
        }
    }
    if media.formats.is_empty() {
        return Err(RatataError::NoFormats(url.to_string()).into());
    }
    Ok(media)
}

fn full_size(value: &str) -> String {
    if !value.contains("pinimg.com") {
        return value.to_owned();
    }
    for prefix in ["736x", "474x", "236x"] {
        let marker = format!("/{prefix}/");
        if value.contains(&marker) {
            return value.replace(&marker, "/originals/");
        }
    }
    value.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_imgur_embed() {
        let html = r#"<meta property="og:title" content="Tunnel petals">
        <meta property="og:image" content="https://i.imgur.com/a.jpg">
        <video src="https://i.imgur.com/a.mp4"></video>"#;
        let url = Url::parse("https://imgur.com/gallery/abc").unwrap();
        let media = parse(html, &url, "Imgur", "abc").unwrap();
        assert_eq!(media.title, "Tunnel petals");
        assert_eq!(media.formats.len(), 2);
    }

    #[test]
    fn parses_pinterest_pin() {
        let html = r#"<meta property="og:title" content="Flower tunnel">
        <meta property="og:video" content="https://v.pinimg.com/v.mp4">
        <meta property="og:image" content="https://i.pinimg.com/p.jpg">"#;
        let url = Url::parse("https://pinterest.com/pin/123").unwrap();
        let media = parse(html, &url, "Pinterest", "pin").unwrap();
        assert_eq!(media.formats.len(), 2);
        assert_eq!(media.formats[0].kind, MediaKind::Video);
    }
}
