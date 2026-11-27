use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let headers = browser_headers("https://soundcloud.com/");
    let page = get_text(client, url.as_str(), &headers).await?;
    let mut client_id = find_client_id(&page);

    if client_id.is_none() {
        client_id = discover_client_id(client).await;
    }
    if let Some(id) = client_id.as_deref() {
        let encoded =
            percent_encoding::utf8_percent_encode(url.as_str(), percent_encoding::NON_ALPHANUMERIC);
        let resolve = format!("https://api-v2.soundcloud.com/resolve?url={encoded}&client_id={id}");
        if let Ok(info) = crate::http::get_json(client, &resolve, &headers).await {
            let mut item = info.clone();
            item["hydratable"] = serde_json::json!("track");
            if let Some(media) = parse_hydration(&Value::Array(vec![item]), url, Some(id)) {
                return resolve_streams(client, media, &headers).await;
            }
        }
    }

    let hydration = Regex::new(r"(?s)window\.__sc_hydration\s*=\s*(\[.*?\]);")
        .ok()
        .and_then(|r| r.captures(&page))
        .and_then(|c| serde_json::from_str::<Value>(&c[1]).ok());
    if let Some(state) = hydration
        && let Some(media) = parse_hydration(&state, url, client_id.as_deref())
    {
        return resolve_streams(client, media, &headers).await;
    }
    let document = scraper::Html::parse_document(&page);
    let selector =
        scraper::Selector::parse("meta[property=\"og:audio\"]").expect("static selector");
    if let Some(address) = document
        .select(&selector)
        .next()
        .and_then(|e| e.value().attr("content"))
    {
        let mut media = Media::new("SoundCloud", url.to_string(), "SoundCloud track");
        media.add(MediaFormat::new("audio", address, MediaKind::Audio));
        return Ok(media);
    }
    Err(RatataError::Extraction("SoundCloud stream not found".into()).into())
}

fn find_client_id(page: &str) -> Option<String> {
    for pattern in [
        r#""client_id":"([^"]+)""#,
        r#"client_id=([a-zA-Z0-9]{20,})"#,
    ] {
        if let Some(capture) = Regex::new(pattern).ok()?.captures(page) {
            return Some(capture[1].to_owned());
        }
    }
    None
}

async fn discover_client_id(client: &Client) -> Option<String> {
    let headers = browser_headers("https://soundcloud.com/");
    let home = get_text(client, "https://soundcloud.com/", &headers)
        .await
        .ok()?;
    find_client_id(&home)
}

async fn resolve_streams(
    client: &Client,
    mut media: Media,
    headers: &[(String, String)],
) -> Result<Media> {
    for format in &mut media.formats {
        if format.url.contains("api-v2.soundcloud.com")
            && let Ok(stream) = crate::http::get_json(client, &format.url, headers).await
            && let Some(address) = stream["url"].as_str()
        {
            format.url = address.to_owned();
        }
    }
    Ok(media)
}

fn parse_hydration(state: &Value, url: &Url, client_id: Option<&str>) -> Option<Media> {
    let mut track = None;
    let mut user = None;
    if let Some(items) = state.as_array() {
        for item in items {
            match item["hydratable"].as_str().unwrap_or_default() {
                "track" if track.is_none() => track = Some(item),
                "user" if user.is_none() => user = Some(item),
                _ => {}
            }
        }
    }
    let track = track?;
    let mut media = Media::new(
        "SoundCloud",
        url.to_string(),
        track["title"]
            .as_str()
            .unwrap_or("SoundCloud track")
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = track["id"]
        .as_u64()
        .map(|id| id.to_string())
        .unwrap_or_default();
    media.author = user.and_then(|u| u["username"].as_str()).map(str::to_owned);
    media.duration = track["duration"].as_u64().map(|ms| ms as f64 / 1000.0);
    media.thumbnail = track["artwork_url"].as_str().map(str::to_owned);
    for transcoding in track["media"]["transcodings"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let Some(address) = transcoding["url"].as_str() else {
            continue;
        };
        let mut address = address.to_owned();
        if let Some(id) = client_id {
            let separator = if address.contains('?') { '&' } else { '?' };
            address.push_str(&format!("{separator}client_id={id}"));
        }
        let mut format = MediaFormat::new(
            transcoding["format"]["protocol"]
                .as_str()
                .unwrap_or("stream"),
            address,
            MediaKind::Audio,
        );
        format.label = transcoding["quality"].as_str().map(str::to_owned);
        media.add(format);
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
    fn parses_hydration_transcodings() {
        let state: Value = serde_json::from_str(r#"[
          {"hydratable":"user","username":"gardener"},
          {"hydratable":"track","id":42,"title":"Bloom","duration":180000,
           "artwork_url":"https://art/p.jpg",
           "media":{"transcodings":[{"url":"https://api/stream","quality":"mp3","format":{"protocol":"progressive"}}]}}
        ]"#).unwrap();
        let url = Url::parse("https://soundcloud.com/gardener/bloom").unwrap();
        let media = parse_hydration(&state, &url, Some("abc")).unwrap();
        assert_eq!(media.title, "Bloom");
        assert_eq!(media.author.as_deref(), Some("gardener"));
        assert_eq!(media.duration, Some(180.0));
        assert!(media.formats[0].url.ends_with("client_id=abc"));
    }
}
