use super::yt_cipher;
use crate::config::Config;
use crate::error::RatataError;
use crate::http::{browser_headers, get_text, post_json};
use crate::model::{ManifestKind, Media, MediaFormat, MediaKind};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde_json::{Value, json};
use url::Url;

const DEFAULT_KEY: &str = "AIzaSyAO_FJ2SlqU8Q4STEHLGCilw_Y9_11qcW8";

pub async fn extract(url: &Url, client: &Client, _config: &Config) -> Result<Media> {
    let id = video_id(url)?;
    let watch_url = format!("https://www.youtube.com/watch?v={id}&hl=en");
    let page = get_text(client, &watch_url, &browser_headers(&watch_url))
        .await
        .unwrap_or_default();
    let api_key =
        capture(&page, r#""INNERTUBE_API_KEY":"([^"]+)""#).unwrap_or_else(|| DEFAULT_KEY.into());
    let client_version = capture(&page, r#""INNERTUBE_CLIENT_VERSION":"([^"]+)""#)
        .unwrap_or_else(|| "2.20250101.00.00".into());

    let clients = [
        ("ANDROID", "19.09.37", Some(30), "Android"),
        ("IOS", "20.10.4", None, "iOS"),
        ("TVHTML5", "7.20250101.18.00", None, "TV"),
        ("WEB", client_version.as_str(), None, "Web"),
        (
            "WEB_EMBEDDED_PLAYER",
            "1.20241009.01.00",
            None,
            "Web embedded",
        ),
        ("MWEB", "2.20250101.00.00", None, "Mobile web"),
        ("ANDROID_VR", "1.60.19", None, "Android VR"),
        ("TVHTML5_SIMPLY_EMBEDDED_PLAYER", "2.0", None, "TV embedded"),
    ];
    let mut last_status = String::new();
    for (name, version, sdk, label) in clients {
        let mut client_info = json!({
            "clientName": name,
            "clientVersion": version,
            "hl": "en",
            "gl": "US",
        });
        if let Some(sdk) = sdk {
            client_info["androidSdkVersion"] = json!(sdk);
            client_info["osName"] = json!("Android");
            client_info["osVersion"] = json!("11");
        }
        let body = json!({
            "context": { "client": client_info },
            "videoId": id,
            "contentCheckOk": true,
            "racyCheckOk": true,
            "playbackContext": {
                "contentPlaybackContext": { "html5Preference": "HTML5_PREF_WANTS" }
            }
        });
        let endpoint =
            format!("https://www.youtube.com/youtubei/v1/player?key={api_key}&prettyPrint=false");
        let response = match post_json(
            client,
            &endpoint,
            &body,
            &browser_headers("https://www.youtube.com/"),
        )
        .await
        {
            Ok(response) => response,
            Err(error) => {
                last_status = format!("{name}: {error}");
                continue;
            }
        };
        if let Some(reason) = response["playabilityStatus"]["reason"].as_str() {
            last_status = format!("{name}: {reason}");
        }
        if response["playabilityStatus"]["status"].as_str() != Some("OK") {
            continue;
        }
        if let Ok(media) = parse_player(&id, &response, label) {
            let mut media = media;
            media.source_url = url.to_string();
            let js_url = yt_cipher::player_js_url(&page).or_else(|| {
                capture(&page, r#""playerJsUrl":"[^"]*?/s/player/([^/]+)/"#)
                    .map(|hash| format!("https://www.youtube.com/s/player/{hash}/player_ias.vflset/en_US/base.js"))
            }).unwrap_or_default();
            if !js_url.is_empty() {
                let js = get_text(
                    client,
                    &js_url,
                    &browser_headers("https://www.youtube.com/"),
                )
                .await
                .unwrap_or_default();
                for format in &mut media.formats {
                    if let Some(deciphered) = yt_cipher::decipher(&js, &format.url) {
                        format.url = deciphered;
                    }
                }
                media.formats.retain(|f| !f.url.contains("signatureCipher"));
            }
            return Ok(media);
        }
    }
    let hint = if last_status.to_ascii_lowercase().contains("bot")
        || last_status.to_ascii_lowercase().contains("sign in")
    {
        "; YouTube asked for a bot check — use --cookies cookies.txt"
    } else {
        ""
    };
    Err(RatataError::Extraction(format!("YouTube refused video {id} ({last_status}){hint}")).into())
}

fn video_id(url: &Url) -> Result<String> {
    if let Some(host) = url.host_str()
        && (host == "youtu.be" || host == "yt.be")
        && let Some(id) = url.path_segments().and_then(|mut s| s.next())
    {
        return Ok(id.chars().take(11).collect());
    }
    if let Some(id) = url
        .query_pairs()
        .find(|(k, _)| k == "v")
        .map(|(_, v)| v.into_owned())
    {
        return Ok(id.chars().take(11).collect());
    }
    for prefix in ["shorts", "embed", "live", "v"] {
        if let Ok(id) = super::require_video_id(url, &[prefix]) {
            return Ok(id.chars().take(11).collect());
        }
    }
    Err(RatataError::UnsupportedUrl(url.to_string()).into())
}

fn parse_player(id: &str, response: &Value, client_label: &str) -> Result<Media> {
    let details = &response["videoDetails"];
    let title = details["title"]
        .as_str()
        .filter(|s| !s.is_empty())
        .unwrap_or(id);
    let mut media = Media::new(
        "YouTube",
        format!("https://www.youtube.com/watch?v={id}"),
        title,
    );
    media.id = id.into();
    media.author = details["author"].as_str().map(str::to_owned);
    media.duration = details["lengthSeconds"]
        .as_str()
        .and_then(|s| s.parse().ok());
    media.thumbnail = details["thumbnail"]["thumbnails"]
        .as_array()
        .and_then(|a| a.last())
        .and_then(|t| t["url"].as_str())
        .map(str::to_owned);

    let streaming = &response["streamingData"];
    let mut formats = Vec::new();
    for key in ["formats", "adaptiveFormats"] {
        if let Some(items) = streaming[key].as_array() {
            for item in items {
                if let Some(format) = parse_format(item) {
                    formats.push(format);
                }
            }
        }
    }
    if let Some(url) = streaming["hlsManifestUrl"].as_str() {
        let mut format = MediaFormat::new("hls", url, MediaKind::Video);
        format.manifest = Some(ManifestKind::Hls);
        format.label = Some(format!("HLS ({client_label})"));
        formats.push(format);
    }
    formats.sort_by_key(|f| std::cmp::Reverse(f.quality()));
    if formats.is_empty() {
        return Err(RatataError::NoFormats(id.into()).into());
    }
    media.formats = formats;
    Ok(media)
}

fn parse_format(item: &Value) -> Option<MediaFormat> {
    let url = item["url"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| item["signatureCipher"].as_str().map(str::to_owned))?;
    let itag = item["itag"].as_u64().unwrap_or(0).to_string();
    let mime = item["mimeType"].as_str().unwrap_or("");
    let kind = if mime.starts_with("audio/") {
        MediaKind::Audio
    } else if mime.starts_with("video/") {
        MediaKind::Video
    } else {
        MediaKind::Other
    };
    let mut format = MediaFormat::new(itag, url, kind);
    format.width = item["width"].as_u64().map(|v| v as u32);
    format.height = item["height"].as_u64().map(|v| v as u32);
    format.fps = item["fps"].as_f64().map(|v| v as f32);
    format.bitrate = item["bitrate"]
        .as_u64()
        .or_else(|| item["averageBitrate"].as_u64());
    format.size = item["contentLength"]
        .as_str()
        .and_then(|s| s.parse().ok())
        .or_else(|| item["contentLength"].as_u64());
    format.codec = Some(
        mime.split("codecs=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .unwrap_or("")
            .to_owned(),
    );
    format.label = item["qualityLabel"].as_str().map(str::to_owned);
    if let Some(headers) = item["playerHeaders"].as_object() {
        for (key, value) in headers {
            if let Some(value) = value.as_str() {
                format.headers.push((key.clone(), value.into()));
            }
        }
    }
    Some(format)
}

fn capture(input: &str, pattern: &str) -> Option<String> {
    Regex::new(pattern)
        .ok()?
        .captures(input)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_player_response_formats() {
        let response = json!({
            "playabilityStatus": {"status": "OK"},
            "videoDetails": {"title": "Tunnel Bloom", "author": "Aarav", "lengthSeconds": "125", "thumbnail": {"thumbnails": [{"url": "https://img/0.jpg"}]}},
            "streamingData": {
                "formats": [{"itag": 18, "url": "https://media/video.mp4", "mimeType": "video/mp4; codecs=\"avc1\"", "width": 640, "height": 360, "fps": 30, "contentLength": "1024", "qualityLabel": "360p"}],
                "adaptiveFormats": [{"itag": 140, "url": "https://media/audio.m4a", "mimeType": "audio/mp4", "bitrate": 128000}]
            }
        });
        let media = parse_player("abc", &response, "Web").unwrap();
        assert_eq!(media.title, "Tunnel Bloom");
        assert_eq!(media.formats.len(), 2);
        assert_eq!(media.best_video().unwrap().id, "18");
        assert_eq!(media.best_audio().unwrap().kind, MediaKind::Audio);
    }

    #[test]
    fn parses_video_id_from_short_url() {
        let url = Url::parse("https://youtu.be/dQw4w9WgXcQ").unwrap();
        assert_eq!(video_id(&url).unwrap(), "dQw4w9WgXcQ");
    }
}
