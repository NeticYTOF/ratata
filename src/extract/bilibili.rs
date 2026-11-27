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
    let mut headers = browser_headers("https://www.bilibili.com/");
    headers.retain(|(key, _)| key != "Referer");
    headers.push(("Referer".into(), url.to_string()));
    headers.push(("Origin".into(), "https://www.bilibili.com".into()));
    headers.push(("Accept".into(), "application/json, text/plain, */*".into()));
    headers.push(("Accept-Language".into(), "zh-CN,zh;q=0.9,en;q=0.8".into()));
    let cookie = bilibili_cookie(client, &headers).await;
    headers.push(("Cookie".into(), cookie));
    let view = get_json(
        client,
        &format!("https://api.bilibili.com/x/web-interface/view?bvid={bvid}"),
        &headers,
    )
    .await;
    if let Ok(view) = &view
        && view["code"].as_i64().unwrap_or(-1) == 0
    {
        let cid = view["data"]["cid"]
            .as_u64()
            .ok_or_else(|| RatataError::Extraction("missing CID".into()))?;
        let playurl = get_json(
            client,
            &format!("https://api.bilibili.com/x/player/playurl?bvid={bvid}&cid={cid}&qn=64&fnval=0&fourk=1"),
            &headers,
        )
        .await?;
        return parse(view, &playurl, url, &bvid)
            .ok_or_else(|| RatataError::NoFormats(url.to_string()).into());
    }
    parse_page_fallback(client, url, &bvid, &headers).await
}

async fn parse_page_fallback(
    client: &Client,
    url: &Url,
    bvid: &str,
    headers: &[(String, String)],
) -> Result<Media> {
    let page = crate::http::get_text(client, url.as_str(), headers).await?;
    let pattern = Regex::new(r"(?s)window\.__playinfo__\s*=\s*(\{.*?\})\s*</script>")?;
    let playinfo = pattern
        .captures(&page)
        .and_then(|c| serde_json::from_str::<Value>(&c[1]).ok())
        .ok_or_else(|| RatataError::Extraction("Bilibili play info not found".into()))?;
    let mut initial =
        serde_json::json!({"data": {"title": bvid, "owner": {"name": ""}, "duration": 0}});
    let state = Regex::new(r"(?s)window\.__INITIAL_STATE__\s*=\s*(\{.*?\});")
        .ok()
        .and_then(|r| r.captures(&page))
        .and_then(|c| serde_json::from_str::<Value>(&c[1]).ok());
    if let Some(value) = state {
        initial["data"] = value["videoData"].clone();
    }
    parse(&initial, &playinfo, url, bvid)
        .ok_or_else(|| RatataError::NoFormats(url.to_string()).into())
}

async fn bilibili_cookie(client: &Client, headers: &[(String, String)]) -> String {
    let spi = get_json(
        client,
        "https://api.bilibili.com/x/frontend/finger/spi",
        headers,
    )
    .await
    .ok();
    if let Some(b3) = spi.as_ref().and_then(|json| json["data"]["b_3"].as_str())
        && let Some(b4) = spi.as_ref().and_then(|json| json["data"]["b_4"].as_str())
    {
        return format!("buvid3={b3}; buvid4={b4}; b_nut=1700000000");
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let stamp = format!("{nanos:032x}");
    format!(
        "buvid3={}-{}-{}-{}-{}infoc; buvid4={}-{}infoc; b_nut={}",
        &stamp[0..8],
        &stamp[8..12],
        &stamp[12..16],
        &stamp[16..20],
        &stamp[20..32],
        &stamp[0..8],
        &stamp[8..24],
        nanos / 1_000_000_000
    )
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
