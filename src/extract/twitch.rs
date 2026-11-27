use crate::error::RatataError;
use crate::http::{browser_headers, get_text};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use reqwest::Client;
use scraper::{Html, Selector};
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let slug = clip_slug(url)?;
    let headers = browser_headers("https://www.twitch.tv/");
    let html = get_text(client, url.as_str(), &headers).await?;
    let document = Html::parse_document(&html);
    let next = Selector::parse("script#__NEXT_DATA__").expect("static selector");
    if let Some(raw) = document
        .select(&next)
        .next()
        .map(|e| e.text().collect::<String>())
        && let Ok(state) = serde_json::from_str::<Value>(&raw)
        && let Some(clip) = find_clip(&state)
        && let Some(media) = parse_clip(clip, url, &slug)
    {
        return Ok(media);
    }
    let embed = format!("https://clips.twitch.tv/embed?clip={slug}");
    let html = get_text(client, &embed, &headers).await.unwrap_or_default();
    let document = Html::parse_document(&html);
    let meta = Selector::parse("meta[property=\"og:video\"]").expect("static selector");
    if let Some(address) = document
        .select(&meta)
        .next()
        .and_then(|e| e.value().attr("content"))
    {
        let mut media = Media::new("Twitch", url.to_string(), format!("Twitch clip {slug}"));
        media.id = slug;
        media.add(MediaFormat::new("clip", address, MediaKind::Video));
        return Ok(media);
    }
    Err(RatataError::Extraction("Twitch clip media not found".into()).into())
}

fn clip_slug(url: &Url) -> Result<String> {
    let segments: Vec<&str> = url
        .path_segments()
        .map(|s| s.filter(|p| !p.is_empty()).collect())
        .unwrap_or_default();
    if url.host_str() == Some("clips.twitch.tv") {
        return segments
            .first()
            .map(|s| (*s).to_owned())
            .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()).into());
    }
    if let Some(index) = segments.iter().position(|s| *s == "clip")
        && let Some(slug) = segments.get(index + 1)
    {
        return Ok((*slug).to_owned());
    }
    Err(RatataError::UnsupportedUrl(url.to_string()).into())
}

fn find_clip(state: &Value) -> Option<&Value> {
    let candidates = [
        &state["props"]["pageState"]["clip"],
        &state["props"]["initialState"]["clip"],
        &state["props"]["pageProps"]["clip"],
        &state["props"]["pageState"]["video"],
    ];
    candidates
        .into_iter()
        .find(|value| !value.is_null() && value["videoQualities"].is_array())
}

fn parse_clip(clip: &Value, url: &Url, slug: &str) -> Option<Media> {
    let mut media = Media::new(
        "Twitch",
        url.to_string(),
        clip["title"]
            .as_str()
            .unwrap_or(slug)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = slug.into();
    media.author = clip["broadcaster"]["displayName"]
        .as_str()
        .or_else(|| clip["broadcaster"]["name"].as_str())
        .map(str::to_owned);
    media.duration = clip["durationSeconds"].as_f64();
    media.thumbnail = clip["thumbnailURL"].as_str().map(str::to_owned);
    for quality in clip["videoQualities"].as_array()? {
        let Some(address) = quality["sourceURL"].as_str() else {
            continue;
        };
        let mut format = MediaFormat::new(
            quality["quality"].as_str().unwrap_or("clip"),
            address,
            MediaKind::Video,
        );
        format.label = quality["quality"].as_str().map(str::to_owned);
        format
            .headers
            .push(("Referer".into(), "https://clips.twitch.tv/".into()));
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
    fn parses_next_data_clip() {
        let clip: Value = serde_json::from_str(
            r#"{
          "title":"Bloom clip","durationSeconds":31,
          "broadcaster":{"displayName":"Gardener"},
          "videoQualities":[{"quality":"1080","sourceURL":"https://clips-media/high.mp4"}]
        }"#,
        )
        .unwrap();
        let url = Url::parse("https://clips.twitch.tv/BloomClip").unwrap();
        let media = parse_clip(&clip, &url, "BloomClip").unwrap();
        assert_eq!(media.author.as_deref(), Some("Gardener"));
        assert_eq!(media.formats[0].label.as_deref(), Some("1080"));
    }
}
