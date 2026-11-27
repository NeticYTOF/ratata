use crate::config::Config;
use crate::error::RatataError;
use anyhow::Result;
use reqwest::header::{AUTHORIZATION, COOKIE, HeaderMap, HeaderValue, REFERER, USER_AGENT};
use reqwest::{Client, ClientBuilder};
use serde_json::Value;
use std::time::Duration;

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
    Ok(builder.build()?)
}

pub fn headers_map(pairs: &[(String, String)]) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for (key, value) in pairs {
        let name = match key.to_ascii_lowercase().as_str() {
            "referer" => REFERER,
            "cookie" => COOKIE,
            "authorization" => AUTHORIZATION,
            "user-agent" => USER_AGENT,
            _ => reqwest::header::HeaderName::from_bytes(key.as_bytes())?,
        };
        map.insert(name, HeaderValue::from_str(value)?);
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
