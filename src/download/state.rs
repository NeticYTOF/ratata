use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentState {
    pub index: usize,
    pub start: u64,
    pub end: u64,
    pub done: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub version: u8,
    pub url: String,
    pub total: Option<u64>,
    pub etag: Option<String>,
    pub segments: Vec<SegmentState>,
}

impl DownloadState {
    pub const VERSION: u8 = 1;

    pub fn new(
        url: &str,
        total: Option<u64>,
        etag: Option<String>,
        segments: Vec<SegmentState>,
    ) -> Self {
        Self {
            version: Self::VERSION,
            url: url.to_owned(),
            total,
            etag,
            segments,
        }
    }

    pub fn load(path: &Path, url: &str) -> Option<Self> {
        let raw = std::fs::read_to_string(path).ok()?;
        let state: Self = serde_json::from_str(&raw).ok()?;
        if state.version != Self::VERSION || state.url != url {
            return None;
        }
        Some(state)
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let json = serde_json::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    pub fn matches(&self, total: Option<u64>, etag: &Option<String>) -> bool {
        self.total == total && self.etag.as_deref() == etag.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_state_from_other_url() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.ratata.json");
        let state = DownloadState::new("https://a/v.mp4", Some(100), None, vec![]);
        state.save(&path).unwrap();
        assert!(DownloadState::load(&path, "https://a/v.mp4").is_some());
        assert!(DownloadState::load(&path, "https://b/v.mp4").is_none());
    }
}
