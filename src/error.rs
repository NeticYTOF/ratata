use thiserror::Error;

#[derive(Debug, Error)]
pub enum RatataError {
    #[error("HTTP {0}")]
    Http(#[from] reqwest::Error),
    #[error("unsupported URL: {0}")]
    UnsupportedUrl(String),
    #[error("no media formats found for {0}")]
    NoFormats(String),
    #[error("format not found: {0}")]
    FormatNotFound(String),
    #[error("download incomplete: {0}")]
    Incomplete(String),
    #[error("extraction failed: {0}")]
    Extraction(String),
}

pub fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' | '/' | '\\' | '\n' | '\r' | '\t' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim_matches('_').trim();
    let trimmed = if trimmed.is_empty() {
        "ratata-download"
    } else {
        trimmed
    };
    if trimmed.chars().count() > 140 {
        trimmed.chars().take(140).collect()
    } else {
        trimmed.to_owned()
    }
}

pub fn verbose_enabled() -> bool {
    std::env::var_os("RATATA_DEBUG").is_some() || std::env::args_os().any(|arg| arg == "--verbose")
}

/// Removes raw URLs (including query strings) and long signatures from user-facing errors.
pub fn sanitize_error(message: &str) -> String {
    if verbose_enabled() {
        return message.to_owned();
    }
    let stripped = strip_urls(message);
    let lower = stripped.to_ascii_lowercase();
    let reason = if lower.contains("403") || lower.contains("forbidden") {
        "Server refused the request (HTTP 403)"
    } else if lower.contains("401") {
        "Server requires sign-in (HTTP 401)"
    } else if lower.contains("412") {
        "Server refused the request (HTTP 412)"
    } else if lower.contains("timed out")
        || lower.contains("stalled")
        || lower.contains("no progress")
        || lower.contains("no data received")
    {
        "No data received from the server (timed out after 20s)"
    } else if lower.contains("unsupported url") || lower.contains("not supported") {
        "That link is not supported"
    } else if lower.contains("no media formats") || lower.contains("no formats") {
        "No playable media found for this link"
    } else if lower.contains("no such file") {
        "A downloaded file could not be found"
    } else {
        return stripped.chars().take(160).collect();
    };
    reason.to_owned()
}

fn strip_urls(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    loop {
        let http = rest.find("http://");
        let https = rest.find("https://");
        let start = match (http, https) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => {
                out.push_str(rest);
                break;
            }
        };
        out.push_str(&rest[..start]);
        out.push_str("[URL]");
        let tail = &rest[start..];
        let end = tail
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | ')' | ',' | ';'))
            .unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_path_separators_from_tweet_titles() {
        let title = "watch https://youtu.be/abc?x=1\nnow";
        let safe = sanitize_filename(title);
        assert!(!safe.contains('/'));
        assert!(!safe.contains('\\'));
        assert!(!safe.contains('\n'));
        assert!(safe.contains("https"));
    }

    #[test]
    fn trims_underscore_edges() {
        assert_eq!(
            sanitize_filename("__Please dont eat me!__"),
            "Please dont eat me!"
        );
    }

    #[test]
    fn shortens_errors_with_long_urls() {
        let raw = "error sending request for url (https://vod.example.com/path/abc?token=abcdef1234567890&sig=deadbeef): HTTP 403 Forbidden";
        let clean = sanitize_error(raw);
        assert!(!clean.contains("vod.example.com"));
        assert!(!clean.contains("token="));
        assert!(clean.contains("Server refused the request (HTTP 403)"));
    }
}
