use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub output_dir: String,
    pub segments: usize,
    pub concurrent_downloads: usize,
    pub retries: u8,
    pub timeout_secs: u64,
    pub min_segment_size: u64,
    pub user_agent: String,
    pub theme: String,
    pub tui: bool,
    pub overwrite: bool,
    pub proxy: Option<String>,
    pub ffmpeg: Option<String>,
    pub keyring: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            output_dir: "~/Downloads/Ratata".into(),
            segments: 8,
            concurrent_downloads: 3,
            retries: 4,
            timeout_secs: 30,
            min_segment_size: 1_048_576,
            user_agent: concat!("ratata/", env!("CARGO_PKG_VERSION")).into(),
            theme: "blossom".into(),
            tui: true,
            overwrite: false,
            proxy: None,
            ffmpeg: None,
            keyring: false,
        }
    }
}

impl Config {
    pub fn path() -> Result<PathBuf> {
        let base = dirs_base();
        Ok(base.join("ratata").join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("reading config {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parsing config {}", path.display()))
    }

    pub fn save(&self) -> Result<PathBuf> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string_pretty(self)?)?;
        Ok(path)
    }

    pub fn output_dir(&self) -> Result<PathBuf> {
        Ok(expand_tilde(&self.output_dir))
    }
}

fn dirs_base() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg);
        }
    }
    #[cfg(windows)]
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata);
    }
    #[cfg(not(windows))]
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config");
    }
    if let Ok(home) = std::env::var("USERPROFILE") {
        return PathBuf::from(home).join("AppData").join("Roaming");
    }
    PathBuf::from(".")
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            return PathBuf::from(home).join(rest.replace('\\', "/"));
        }
    }
    PathBuf::from(path)
}

pub fn state_path(destination: &Path) -> PathBuf {
    let mut name = destination
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".into());
    name.push_str(".ratata.json");
    destination.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilde_expansion_uses_home() {
        let expanded = expand_tilde("~/Downloads/Ratata");
        assert!(expanded.ends_with("Downloads/Ratata"));
        assert!(!expanded.to_string_lossy().contains('~'));
    }

    #[test]
    fn state_path_sits_next_to_target() {
        let path = state_path(Path::new("/tmp/clip.mp4"));
        assert_eq!(path, Path::new("/tmp/clip.mp4.ratata.json"));
    }
}
