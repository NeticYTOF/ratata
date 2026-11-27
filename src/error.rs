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
}
