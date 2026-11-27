use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MediaKind {
    #[default]
    Video,
    Audio,
    Image,
    Subtitle,
    Other,
}

impl MediaKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Video => "video",
            Self::Audio => "audio",
            Self::Image => "image",
            Self::Subtitle => "subtitle",
            Self::Other => "asset",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaFormat {
    pub id: String,
    pub url: String,
    pub kind: MediaKind,
    pub ext: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub bitrate: Option<u64>,
    pub size: Option<u64>,
    pub codec: Option<String>,
    pub headers: Vec<(String, String)>,
    pub label: Option<String>,
    /// HLS/dash manifests need a playlist-aware download path.
    pub manifest: Option<ManifestKind>,
}

impl MediaFormat {
    pub fn new(id: impl Into<String>, url: impl Into<String>, kind: MediaKind) -> Self {
        let id = id.into();
        let url = crate::http::normalize_media_url(&url.into());
        let ext = guess_ext(&url, kind);
        Self {
            id,
            url,
            kind,
            ext,
            width: None,
            height: None,
            fps: None,
            bitrate: None,
            size: None,
            codec: None,
            headers: Vec::new(),
            label: None,
            manifest: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn quality(&self) -> u64 {
        u64::from(self.width.unwrap_or(0)) * u64::from(self.height.unwrap_or(0))
            + self.bitrate.unwrap_or(0) / 1000
    }

    pub fn describe(&self) -> String {
        let dimensions = match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}x{h}"),
            _ => "—".into(),
        };
        let size = self
            .size
            .map(|s| format!("{:.1} MiB", s as f64 / 1_048_576.0))
            .unwrap_or_else(|| "?".into());
        format!(
            "{:<14} {:<8} {:<10} {:>8} {:>8} {}",
            self.id,
            self.kind.label(),
            self.ext,
            dimensions,
            self.fps
                .map(|f| format!("{f:.0}fps"))
                .unwrap_or_else(|| "-".into()),
            size
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManifestKind {
    Hls,
    Dash,
}

fn guess_ext(url: &str, kind: MediaKind) -> String {
    let path = url.split('?').next().unwrap_or(url);
    let suffix = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    if matches!(
        suffix.as_str(),
        "mp4"
            | "webm"
            | "mov"
            | "mkv"
            | "m4a"
            | "mp3"
            | "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "gif"
            | "srt"
            | "vtt"
    ) {
        if suffix == "jpeg" {
            return "jpg".into();
        }
        return suffix;
    }
    if path.ends_with(".m3u8") {
        return "mp4".into();
    }
    if path.ends_with(".mpd") {
        return "mp4".into();
    }
    match kind {
        MediaKind::Video => "mp4",
        MediaKind::Audio => "m4a",
        MediaKind::Image => "jpg",
        MediaKind::Subtitle => "srt",
        MediaKind::Other => "bin",
    }
    .into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Media {
    pub id: String,
    pub platform: String,
    pub title: String,
    pub author: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub formats: Vec<MediaFormat>,
    pub source_url: String,
}

impl Media {
    pub fn new(
        platform: impl Into<String>,
        source_url: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            id: String::new(),
            platform: platform.into(),
            title: title.into(),
            author: None,
            duration: None,
            thumbnail: None,
            formats: Vec::new(),
            source_url: source_url.into(),
        }
    }

    pub fn add(&mut self, format: MediaFormat) {
        self.formats.push(format);
    }

    pub fn best_video(&self) -> Option<&MediaFormat> {
        self.formats
            .iter()
            .filter(|f| f.kind == MediaKind::Video)
            .max_by_key(|f| f.quality())
    }

    pub fn best_audio(&self) -> Option<&MediaFormat> {
        self.formats
            .iter()
            .filter(|f| f.kind == MediaKind::Audio)
            .max_by_key(|f| f.bitrate.unwrap_or(0))
    }

    pub fn best_image(&self) -> Option<&MediaFormat> {
        self.formats.iter().find(|f| f.kind == MediaKind::Image)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Done,
    Failed,
    Cancelled,
}

impl JobState {
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Queued => "·",
            Self::Running => "▸",
            Self::Paused => "‖",
            Self::Done => "✓",
            Self::Failed => "✗",
            Self::Cancelled => "○",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobProgress {
    pub id: usize,
    pub label: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub speed: f64,
    pub state: JobState,
    pub error: Option<String>,
    pub segments: Vec<bool>,
    pub eta: Option<f64>,
}

impl JobProgress {
    pub fn fraction(&self) -> f64 {
        match self.total {
            Some(total) if total > 0 => (self.downloaded as f64 / total as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub id: usize,
    pub media: Media,
    pub format: MediaFormat,
    pub destination: std::path::PathBuf,
    /// Bytes are downloaded here first when MP3 conversion is requested.
    pub temp: Option<std::path::PathBuf>,
    pub mp3: Option<crate::media::audio::AudioMeta>,
    pub keep_source: bool,
    pub validation_error: Option<String>,
}
