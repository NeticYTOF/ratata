use crate::error::RatataError;
use crate::http::{browser_headers, get_json};
use crate::model::{Media, MediaFormat, MediaKind};
use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde_json::Value;
use url::Url;

pub async fn extract(url: &Url, client: &Client) -> Result<Media> {
    let pattern = Regex::new(r"^(BV[0-9A-Za-z]+|av\d+)$")?;
    let bvid = url
        .path_segments()
        .and_then(|mut segments| {
            segments.find_map(|segment| pattern.captures(segment).map(|m| m[0].to_owned()))
        })
        .or_else(|| {
            url.query_pairs()
                .find(|(key, value)| key == "bvid" && pattern.is_match(value))
                .map(|(_, value)| value.into_owned())
        })
        .ok_or_else(|| RatataError::UnsupportedUrl(url.to_string()))?;
    let headers = browser_headers("https://www.bilibili.com/");
    let view = get_json(
        client,
        &format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}"),
        &headers,
    )
    .await?;
    if view["code"].as_i64().unwrap_or(-1) != 0 {
        return Err(RatataError::Extraction(format!("Bilibili rejected {bvid}")).into());
    }
    let cid = view["data"]["cid"]
        .as_u64()
        .ok_or_else(|| RatataError::Extraction("missing CID".into()))?;
    let playurl = get_json(
        client,
        &format!(
            "https://api.bilibili.com/x/player/playurl?bvid={bvid}&cid={cid}&qn=64&fnval=0&fourk=1"
        ),
        &headers,
    )
    .await?;
    parse(&view, &playurl, url, &bvid).ok_or_else(|| RatataError::NoFormats(url.to_string()).into())
}

fn parse(view: &Value, playurl: &Value, url: &Url, bvid: &str) -> Option<Media> {
    let data = &view["data"];
    let mut media = Media::new(
        "Bilibili",
        url.to_string(),
        data["title"]
            .as_str()
            .unwrap_or(bvid)
            .chars()
            .take(140)
            .collect::<String>(),
    );
    media.id = bvid.into();
    media.author = data["owner"]["name"].as_str().map(str::to_owned);
    media.duration = data["duration"].as_f64();
    media.thumbnail = data["pic"].as_str().map(str::to_owned);
    if let Some(durl) = playurl["data"]["durl"].as_array() {
        for entry in durl {
            let Some(address) = entry["url"].as_str() else {
                continue;
            };
            let mut format = MediaFormat::new(
                entry["quality"]
                    .as_u64()
                    .map(|q| q.to_string())
                    .unwrap_or_else(|| "video".into()),
                address,
                MediaKind::Video,
            );
            format.size = entry["size"].as_u64();
            format
                .headers
                .push(("Referer".into(), "https://www.bilibili.com/".into()));
            media.add(format);
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
    fn parses_bilibili_playurl() {
        let view: Value = serde_json::from_str(r#"{"code":0,"data":{"bvid":"BV1xx","cid":42,"title":"Bloom","duration":120,"owner":{"name":"Gardener"},"pic":"https://img/p.jpg"}}"#).unwrap();
        let play: Value = serde_json::from_str(
            r#"{"data":{"durl":[{"url":"https://upos/video.mp4","size":9000,"quality":64}]}}"#,
        )
        .unwrap();
        let url = Url::parse("https://www.bilibili.com/video/BV1xx").unwrap();
        let media = parse(&view, &play, &url, "BV1xx").unwrap();
        assert_eq!(media.author.as_deref(), Some("Gardener"));
        assert_eq!(media.formats[0].size, Some(9000));
    }

    #[test]
    fn rejects_wrong_video_id_shape() {
        assert!(
            Regex::new(r"^(BV[0-9A-Za-z]+|av\d+)$")
                .unwrap()
                .captures("nope")
                .is_none()
        );
    }
}
