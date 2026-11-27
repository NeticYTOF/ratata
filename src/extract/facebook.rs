use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let embed = format!(
        "https://www.facebook.com/plugins/video.php?href={}",
        percent_encoding::utf8_percent_encode(url.as_str(), percent_encoding::NON_ALPHANUMERIC)
    );
    let headers = browser_headers("https://www.facebook.com/");
    let html = get_text(client, &embed, &headers).await.unwrap_or_default();
    if let Ok(media) = parse(&html, url) {
        return Ok(media);
    }
    let mbasic = url
        .to_string()
        .replace("www.facebook.com", "mbasic.facebook.com");
    let fallback = get_text(client, &mbasic, &headers)
        .await
        .unwrap_or_default();
    parse(&fallback, url)
}

fn parse(html: &str, url: &Url) -> Result<Media> {
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
        "Facebook",
        url.to_string(),
        meta("og:title").unwrap_or_else(|| "Facebook video".into()),
    );
    media.thumbnail = meta("og:image");
    let mut seen = std::collections::HashSet::new();
    for key in [
        "playable_url_quality_hd",
        "browser_native_hd_url",
        "hd_src",
        "playable_url",
        "browser_native_sd_url",
        "sd_src",
    ] {
        let pattern = format!(r#"{key}\\?":\\?"([^"]+)""#);
        let Some(value) = Regex::new(&pattern)
            .ok()
            .and_then(|r| r.captures(html))
            .map(|c| c[1].trim_end_matches('\\').to_owned())
        else {
            continue;
        };
        let mut decoded = value;
        for _ in 0..3 {
            match serde_json::from_str::<String>(&format!("\"{decoded}\"")) {
                Ok(value) => decoded = value,
                Err(_) => break,
            }
        }
        let decoded = decoded.replace("\\/", "/");
        if decoded.starts_with("http") && seen.insert(decoded.clone()) {
            let mut format = MediaFormat::new(key, decoded, MediaKind::Video);
            format.label = Some(key.to_owned());
            media.add(format);
        }
    }
    if media.formats.is_empty()
        && let Ok(regex) = Regex::new(r#"https?(?:\\/\\/|//)[^"\s<>]+\.mp4[^"\s<>]*"#)
    {
        for capture in regex.captures_iter(html) {
            if let Some(raw) = capture.get(0).map(|m| m.as_str().replace("\\/", "/"))
                && seen.insert(raw.clone())
            {
                media.add(MediaFormat::new("video", raw, MediaKind::Video));
            }
        }
    }
    if media.formats.is_empty() {
        return Err(RatataError::NoFormats(url.to_string()).into());
    }
    Ok(media)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_escaped_playable_urls() {
        let html = r#"<meta property="og:title" content="Flowers">
        <script>{"playable_url_quality_hd":"https:\/\/video.fbcdn.net\/hd.mp4","playable_url":"https:\/\/video.fbcdn.net\/sd.mp4"}</script>"#;
        let url = Url::parse("https://www.facebook.com/watch/?v=123").unwrap();
        let media = parse(html, &url).unwrap();
        assert_eq!(media.title, "Flowers");
        assert_eq!(media.formats.len(), 2);
        assert_eq!(media.formats[0].url, "https://video.fbcdn.net/hd.mp4");
    }
}
