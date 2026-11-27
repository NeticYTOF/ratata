use crate::config::Config;
use crate::error::RatataError;
use anyhow::{Context, Result};
use reqwest::cookie::Jar;
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use reqwest::{Client, ClientBuilder};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

pub fn build_client(config: &Config) -> Result<Client> {
    let mut builder = ClientBuilder::new()
        .user_agent(&config.user_agent)
        .timeout(Duration::from_secs(config.timeout_secs))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(32)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .gzip(true)
        .brotli(true)
        .deflate(true);
    if let Some(proxy) = &config.proxy {
        builder = builder.proxy(reqwest::Proxy::all(proxy)?);
    }
    let jar = Arc::new(Jar::default());
    if let Some(path) = &config.cookies {
        let raw =
            std::fs::read_to_string(path).with_context(|| format!("reading cookies {}", path))?;
        for (domain, cookie_path, name, value) in parse_netscape(&raw) {
            let url = Url::parse(&format!("https://{domain}{cookie_path}"))?;
            jar.add_cookie_str(&format!("{name}={value}"), &url);
        }
    }
    builder = builder.cookie_provider(jar);
    Ok(builder.build()?)
}

/// Parses Netscape cookie files (`domain flag path secure expiry name value`).
pub fn parse_netscape(raw: &str) -> Vec<(String, String, String, String)> {
    raw.lines()
        .filter(|line| !line.trim_start().starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            if fields.len() < 7 {
                return None;
            }
            Some((
                fields[0].to_owned(),
                fields[2].to_owned(),
                fields[5].to_owned(),
                fields[6].to_owned(),
            ))
        })
        .collect()
}

/// Normalises CDN URLs seen in site APIs and previously corrupted URLs.
pub fn normalize_media_url(raw: &str) -> String {
    let mut value = raw.trim().to_owned();
    loop {
        let next = if let Some(rest) = value.strip_prefix("https:https://") {
            format!("https://{rest}")
        } else if let Some(rest) = value.strip_prefix("https:https//") {
            format!("https://{rest}")
        } else if let Some(rest) = value.strip_prefix("https://https://") {
            format!("https://{rest}")
        } else if let Some(rest) = value.strip_prefix("https://https//") {
            format!("https://{rest}")
        } else if let Some(rest) = value.strip_prefix("http:http://") {
            format!("http://{rest}")
        } else if let Some(rest) = value.strip_prefix("http//") {
            format!("http://{rest}")
        } else if let Some(rest) = value.strip_prefix("https//") {
            format!("https://{rest}")
        } else if let Some(rest) = value.strip_prefix("//") {
            format!("https://{rest}")
        } else {
            break;
        };
        if next == value {
            break;
        }
        value = next;
    }
    value
}

pub fn headers_map(pairs: &[(String, String)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in pairs {
        let name = match key.to_ascii_lowercase().as_str() {
            "referer" => REFERER,
            "cookie" => COOKIE,
            "authorization" => AUTHORIZATION,
            "user-agent" => USER_AGENT,
            _ => match reqwest::header::HeaderName::from_bytes(key.as_bytes()) {
                Ok(name) => name,
                Err(_) => continue,
            },
        };
        let cleaned: String = value
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>();
        let cleaned = cleaned.trim();
        if cleaned.is_empty() {
            continue;
        }
        if let Ok(header_value) = HeaderValue::from_str(cleaned) {
            map.insert(name, header_value);
        }
    }
    Ok(map)
}

pub async fn get_text(client: &Client, url: &str, headers: &[(String, String)]) -> Result<String> {
    let response = client
        .get(url)
        .headers(headers_map(headers)?)
        .send()
        .await?
        .error_for_status()
        .map_err(RatataError::from)?;
    Ok(response.text().await?)
}

pub async fn get_json(client: &Client, url: &str, headers: &[(String, String)]) -> Result<Value> {
    let response = client
        .get(url)
        .headers(headers_map(headers)?)
        .send()
        .await?
        .error_for_status()
        .map_err(RatataError::from)?;
    Ok(response.json::<Value>().await?)
}

pub async fn post_json(
    client: &Client,
    url: &str,
    body: &Value,
    headers: &[(String, String)],
) -> Result<Value> {
    let response = client
        .post(url)
        .headers(headers_map(headers)?)
        .json(body)
        .send()
        .await?
        .error_for_status()
        .map_err(RatataError::from)?;
    Ok(response.json::<Value>().await?)
}

pub fn browser_headers(referer: &str) -> Vec<(String, String)> {
    vec![
        ("User-Agent".into(), "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36".into()),
        ("Accept-Language".into(), "en-US,en;q=0.9".into()),
        ("Referer".into(), referer.into()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_netscape_cookies() {
        let raw = "# Netscape HTTP Cookie File\n.youtube.com\tTRUE\t/\tFALSE\t0\tsession\tabc123\n";
        let cookies = parse_netscape(raw);
        assert_eq!(cookies.len(), 1);
        assert_eq!(
            cookies[0],
            (
                ".youtube.com".into(),
                "/".into(),
                "session".into(),
                "abc123".into()
            )
        );
        assert_eq!(
            cookies[0],
            (
                ".youtube.com".into(),
                "/".into(),
                "session".into(),
                "abc123".into()
            )
        );
    }

    #[test]
    fn sanitizes_download_headers() {
        let headers = headers_map(&[
            ("User-Agent".into(), "Mozilla/5.0\r\nX-Bad: yes".into()),
            ("Referer".into(), "https://www.tiktok.com/".into()),
            ("bad header".into(), "value".into()),
        ])
        .unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.contains_key(USER_AGENT));
    }

    #[test]
    fn normalizes_corrupted_cdn_urls() {
        assert_eq!(
            normalize_media_url("https://https//cdn-cf-west.streamable.com/video/a.mp4"),
            "https://cdn-cf-west.streamable.com/video/a.mp4"
        );
        assert_eq!(
            normalize_media_url("https:https://cdn-cf-west.streamable.com/video/a.mp4?x=1"),
            "https://cdn-cf-west.streamable.com/video/a.mp4?x=1"
        );
        assert_eq!(
            normalize_media_url("//cdn.example/a.mp4"),
            "https://cdn.example/a.mp4"
        );
        assert_eq!(
            normalize_media_url("https//cdn.example/a.mp4"),
            "https://cdn.example/a.mp4"
        );
    }
}
