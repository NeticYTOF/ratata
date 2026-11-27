use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let headers = browser_headers("https://www.tiktok.com/");
    let html = get_text(client, url.as_str(), &headers).await?;
    let document = Html::parse_document(&html);
    let selector =
        Selector::parse("script#__UNIVERSAL_DATA_FOR_REHYDRATION__").expect("static selector");
    let data = document
        .select(&selector)
        .next()
        .and_then(|e| serde_json::from_str::<Value>(&e.text().collect::<String>()).ok());
    if let Some(state) = data {
        let scope = &state["__DEFAULT_SCOPE__"];
        let first = &scope["webapp.video-detail"]["itemInfo"]["itemStruct"];
        let second = &scope["videoDetail"]["itemInfo"]["itemStruct"];
        let third = &scope["webapp.video-detail"]["itemStruct"];
        let item = if !first.is_null() {
            first
        } else if !second.is_null() {
            second
        } else {
            third
        };
        if let Some(parsed) = parse_item(item, url) {
            return Ok(parsed);
        }
    }
    let oembed = format!(
        "https://www.tiktok.com/oembed?url={}",
        urlencoding(url.as_str())
    );
    if let Ok(json) = get_text(client, &oembed, &headers).await
        && let Ok(value) = serde_json::from_str::<Value>(&json)
    {
        let mut media = Media::new(
            "TikTok",
            url.to_string(),
            value["title"].as_str().unwrap_or("TikTok").to_owned(),
        );
        media.author = value["author_name"].as_str().map(str::to_owned);
        media.thumbnail = value["thumbnail_url"].as_str().map(str::to_owned);
        if !media.formats.is_empty() {
            return Ok(media);
        }
    }
    Err(RatataError::Extraction("TikTok playable media not found".into()).into())
}

fn urlencoding(value: &str) -> String {
    percent_encoding::utf8_percent_encode(value, percent_encoding::NON_ALPHANUMERIC).to_string()
}

fn parse_item(item: &Value, url: &Url) -> Option<Media> {
    if item.is_null() {
        return None;
    }
    let mut media = Media::new(
        "TikTok",
        url.to_string(),
        item["desc"]
            .as_str()
            .unwrap_or("TikTok")
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = item["id"].as_str().unwrap_or_default().to_owned();
    media.author = item["author"]["uniqueId"]
        .as_str()
        .or_else(|| item["author"]["nickname"].as_str())
        .map(str::to_owned);
    media.duration = item["video"]["duration"].as_f64();
    media.thumbnail = item["video"]["cover"]
        .as_str()
        .or_else(|| item["video"]["originCover"].as_str())
        .map(str::to_owned);

    for key in ["downloadAddr", "playAddr", "bitrateAddr"] {
        if let Some(address) = item["video"][key].as_str() {
            add_format(&mut media, address, None);
        }
    }
    if let Some(bitrate_info) = item["video"]["bitrateInfo"].as_array() {
        for info in bitrate_info {
            let address = info["PlayAddr"]["DataUrl"]
                .as_str()
                .or_else(|| info["PlayAddr"]["Src"].as_str());
            if let Some(address) = address {
                add_format(&mut media, address, info["Bitrate"].as_u64());
            }
        }
    }
    if let Some(images) = item["imagePost"]["images"].as_array() {
        for image in images {
            let address = image["imageURL"]["urlList"]
                .as_array()
                .and_then(|a| a.first())
                .and_then(Value::as_str);
            if let Some(address) = address {
                let mut format = MediaFormat::new(
                    format!("i{}", media.formats.len() + 1),
                    address,
                    MediaKind::Image,
                );
                format.headers.push((
                    "User-Agent".into(),
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
                ));
                format
                    .headers
                    .push(("Referer".into(), "https://www.tiktok.com/".into()));
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

fn add_format(media: &mut Media, address: &str, bitrate: Option<u64>) {
    let url = address.replace("&amp;", "&");
    if url.starts_with("data:") {
        return;
    }
    let mut format = MediaFormat::new(
        format!("v{}", media.formats.len() + 1),
        url,
        MediaKind::Video,
    );
    format.bitrate = bitrate;
    format.headers.push((
        "User-Agent".into(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into(),
    ));
    format
        .headers
        .push(("Referer".into(), "https://www.tiktok.com/".into()));
    if format.url.ends_with(".m3u8") {
        format.manifest = Some(ManifestKind::Hls);
    }
    media.add(format);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tiktok_state_item() {
        let item: Value = serde_json::from_str(r#"{
          "id":"735","desc":"Tunnel of blossoms","author":{"uniqueId":"bloom"},
          "video":{"duration":21,"downloadAddr":"https://v16.tiktok.com/video.mp4",
            "bitrateInfo":[{"Bitrate":1200000,"PlayAddr":{"DataUrl":"https://v16.tiktok.com/hd.mp4"}}]}
        }"#).unwrap();
        let url = Url::parse("https://www.tiktok.com/@bloom/video/735").unwrap();
        let media = parse_item(&item, &url).unwrap();
        assert_eq!(media.title, "Tunnel of blossoms");
        assert_eq!(media.formats.len(), 2);
        assert!(media.formats.iter().any(|f| f.bitrate == Some(1_200_000)));
    }

    #[test]
    fn tiktok_download_headers_are_valid() {
        let item: Value = serde_json::from_str(
            r#"{"id":"1","desc":"clip","video":{"downloadAddr":"https://v16-webapp-prime.us.tiktok.com/v.mp4"}}"#,
        )
        .unwrap();
        let url = Url::parse("https://www.tiktok.com/@user/video/1").unwrap();
        let media = parse_item(&item, &url).unwrap();
        for format in &media.formats {
            assert!(format.url.starts_with("https://"));
            for (name, value) in &format.headers {
                assert!(!name.is_empty() && !value.is_empty(), "empty header");
                assert!(
                    !value.chars().any(char::is_control),
                    "control character in {name}"
                );
            }
            let map = crate::http::headers_map(&format.headers).unwrap();
            assert!(map.contains_key("user-agent"));
            assert!(map.contains_key("referer"));
        }
    }
}
