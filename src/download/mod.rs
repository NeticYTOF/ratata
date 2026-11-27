pub mod direct;
pub mod progress;
pub mod segment;
pub mod state;
use crate::config::Config;
use crate::model::{DownloadJob, JobProgress, JobState, ManifestKind};
use anyhow::{Result, bail};
use parking_lot::Mutex;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use url::Url;

pub struct DownloadManager {
    pub config: Config,
    pub client: Client,
    pub tx: mpsc::UnboundedSender<JobProgress>,
    pub pause: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    pub average_speed: Arc<AtomicU64>,
    global: Arc<Semaphore>,
    host_limits: Mutex<HashMap<String, Arc<Semaphore>>>,
}

#[derive(Clone)]
pub struct WorkerContext {
    pub client: Client,
    pub config: Config,
    pub pause: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
}

impl WorkerContext {
    pub async fn gate(&self) -> Result<()> {
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                bail!("download cancelled");
            }
            if !self.pause.load(Ordering::SeqCst) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl DownloadManager {
    pub fn new(
        config: Config,
        client: Client,
        tx: Option<mpsc::UnboundedSender<JobProgress>>,
    ) -> Self {
        let fallback = mpsc::unbounded_channel().0;
        let global = Arc::new(Semaphore::new(config.concurrent_downloads.max(1)));
        Self {
            config,
            client,
            tx: tx.unwrap_or(fallback),
            pause: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            average_speed: Arc::new(AtomicU64::new(0)),
            global,
            host_limits: Mutex::new(HashMap::new()),
        }
    }

    pub fn context(&self) -> WorkerContext {
        WorkerContext {
            client: self.client.clone(),
            config: self.config.clone(),
            pause: self.pause.clone(),
            cancel: self.cancel.clone(),
        }
    }

    pub fn run_all(
        self: Arc<Self>,
        jobs: Vec<DownloadJob>,
    ) -> Vec<JoinHandle<Result<std::path::PathBuf>>> {
        jobs.into_iter()
            .map(|job| {
                let manager = self.clone();
                tokio::spawn(async move { manager.download(job).await })
            })
            .collect()
    }

    async fn fetch_job(
        &self,
        job: &DownloadJob,
        tracker: &progress::ProgressTracker,
    ) -> Result<std::path::PathBuf> {
        let mut result = match job.format.manifest {
            Some(ManifestKind::Hls) => crate::media::hls::download(self, job, tracker).await,
            Some(ManifestKind::Dash) => crate::media::dash::download(self, job, tracker).await,
            None => direct::download(self, job, tracker).await,
        };
        let blocked = result.as_ref().err().is_some_and(|error| {
            let message = error.to_string().to_ascii_lowercase();
            message.contains("403")
                || message.contains("401")
                || message.contains("forbidden")
                || message.contains("stalled")
                || message.contains("no download progress")
                || message.contains("timed out")
        });
        if blocked {
            let first_error = result
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            let alternatives: Vec<_> = job
                .media
                .formats
                .iter()
                .filter(|format| format.id != job.format.id && format.kind == job.format.kind)
                .take(6)
                .cloned()
                .collect();
            let mut tried = Vec::new();
            for alternative in alternatives {
                let _ = tokio::fs::remove_file(direct::single_part_path(&job.destination)).await;
                let _ = tokio::fs::remove_file(crate::config::state_path(&job.destination)).await;
                tracker.reset();
                let mut retry = job.clone();
                retry.format = alternative;
                result = direct::download(self, &retry, tracker).await;
                if result.is_ok() {
                    return result;
                }
                tried.push(
                    result
                        .as_ref()
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_default(),
                );
            }
            if result.is_err() {
                bail!(
                    "all formats failed; first error: {}; other attempts: {}",
                    first_error,
                    tried.join(" | ")
                );
            }
        }
        result
    }
    pub async fn download(&self, job: DownloadJob) -> Result<std::path::PathBuf> {
        let started = Instant::now();
        if let Some(reason) = &job.validation_error {
            let _ = self.tx.send(JobProgress {
                id: job.id,
                label: job.media.title.clone(),
                downloaded: 0,
                total: None,
                speed: 0.0,
                state: JobState::Failed,
                error: Some(reason.clone()),
                segments: Vec::new(),
                eta: None,
            });
            return Err(anyhow::anyhow!("{reason}"));
        }
        let _permit = self.global.acquire().await?;
        let host = Url::parse(&job.format.url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_default();
        let host_limit = {
            let mut limits = self.host_limits.lock();
            limits
                .entry(host)
                .or_insert_with(|| {
                    Arc::new(Semaphore::new(self.config.concurrent_downloads.max(1)))
                })
                .clone()
        };
        let _host_permit = host_limit.acquire().await?;
        let total = job.format.size;
        let tracker =
            progress::ProgressTracker::new(self.tx.clone(), job.id, job.media.title.clone(), total);
        let reporter = tracker.spawn();
        tracker.set_state(JobState::Running);
        let mut fetch_job = job.clone();
        if let Some(temp) = &job.temp {
            fetch_job.destination = temp.clone();
        }
        let fetch = self.fetch_job(&fetch_job, &tracker);
        tokio::pin!(fetch);
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut last_bytes = 0u64;
        let mut stalled = 0u32;
        let result = loop {
            tokio::select! {
                result = &mut fetch => break result,
                _ = tick.tick() => {
                    let now_bytes = tracker.downloaded();
                    if now_bytes > last_bytes {
                        last_bytes = now_bytes;
                        stalled = 0;
                    } else {
                        stalled += 1;
                        if stalled >= 20 {
                            break Err(anyhow::anyhow!(
                                "no download progress for 20 seconds at {}",
                                job.format.url
                            ));
                        }
                    }
                }
            }
        };
        let mut result = result;
        if let (Ok(path), Some(meta)) = (&result, &job.mp3) {
            match crate::media::audio::ffmpeg_or_error(self.config.ffmpeg.as_deref()) {
                Ok(ffmpeg) => {
                    result = match crate::media::audio::convert_to_mp3(
                        &self.client,
                        path,
                        meta,
                        &ffmpeg,
                        !job.keep_source,
                    )
                    .await
                    {
                        Ok(output) => Ok(output),
                        Err(error) => Err(error),
                    };
                }
                Err(error) => result = Err(error),
            }
        }
        let _ = tokio::fs::remove_file(crate::config::state_path(&fetch_job.destination)).await;
        let _ = tokio::fs::remove_file(direct::single_part_path(&fetch_job.destination)).await;
        if job.temp.is_some() {
            let _ = tokio::fs::remove_file(&fetch_job.destination).await;
        }
        let elapsed = started.elapsed().as_secs_f64().max(0.1);
        let measured = (tracker.downloaded() as f64 / elapsed) as u64;
        let previous = self.average_speed.load(Ordering::Relaxed);
        self.average_speed.store(
            ((previous as f64 * 0.6) + (measured as f64 * 0.4)) as u64,
            Ordering::Relaxed,
        );
        tracker.set_state(match &result {
            Ok(_) => JobState::Done,
            Err(_) if self.cancel.load(Ordering::Relaxed) => JobState::Cancelled,
            Err(_) => JobState::Failed,
        });
        reporter.abort();
        let final_state = match &result {
            Ok(_) => JobState::Done,
            Err(_) if self.cancel.load(Ordering::Relaxed) => JobState::Cancelled,
            Err(_) => JobState::Failed,
        };
        let error = result.as_ref().err().map(|e| e.to_string());
        let _ = self.tx.send(JobProgress {
            id: job.id,
            label: job.media.title.clone(),
            downloaded: tracker.downloaded(),
            total,
            speed: 0.0,
            state: final_state,
            error,
            segments: Vec::new(),
            eta: None,
        });
        result
    }

    pub fn set_pause(&self, paused: bool) {
        self.pause.store(paused, Ordering::SeqCst);
    }

    pub fn is_paused(&self) -> bool {
        self.pause.load(Ordering::SeqCst)
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub async fn gate(&self) -> Result<()> {
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                bail!("download cancelled");
            }
            if !self.pause.load(Ordering::SeqCst) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

pub async fn run_plain(jobs: Vec<DownloadJob>, config: &Config, client: Arc<Client>) -> Result<()> {
    use crossterm::cursor;
    use crossterm::execute;
    use crossterm::style::{Color, ResetColor, SetForegroundColor};
    use crossterm::terminal::{Clear, ClearType};
    use std::collections::HashMap;
    use std::io::{Write, stdout};

    crate::ui::enable_color();
    let (tx, mut rx) = mpsc::unbounded_channel::<JobProgress>();
    let manager = Arc::new(DownloadManager::new(
        config.clone(),
        (*client).clone(),
        Some(tx),
    ));
    let count = jobs.len();
    let handles = manager.run_all(jobs);
    let started = Instant::now();
    let mut latest: Vec<JobProgress> = Vec::new();
    let mut last_seen: HashMap<usize, (u64, Instant)> = HashMap::new();
    let mut rendered = 0usize;
    while latest.iter().filter(|p| is_final(p.state)).count() < count {
        let Some(mut progress) = rx.recv().await else {
            break;
        };
        let now = Instant::now();
        let (previous_bytes, previous_time) =
            last_seen.get(&progress.id).copied().unwrap_or((0, started));
        let elapsed = now.duration_since(previous_time).as_secs_f64().max(0.02);
        let measured = (progress.downloaded.saturating_sub(previous_bytes)) as f64 / elapsed;
        last_seen.insert(progress.id, (progress.downloaded, now));
        if progress.speed <= 1.0 {
            progress.speed = measured;
        }
        if let Some(total) = progress.total
            && progress.eta.is_none()
            && progress.speed > 1.0
            && total > progress.downloaded
        {
            progress.eta = Some((total - progress.downloaded) as f64 / progress.speed);
        }

        if let Some(existing) = latest.iter_mut().find(|p| p.id == progress.id) {
            *existing = progress;
        } else {
            latest.push(progress);
        }
        if rendered > 0 {
            execute!(
                stdout(),
                cursor::MoveUp(rendered as u16),
                Clear(ClearType::FromCursorDown)
            )
            .ok();
        }
        latest.sort_by_key(|p| p.id);
        for item in &latest {
            let color = match item.state {
                JobState::Done => Color::Green,
                JobState::Failed | JobState::Cancelled => Color::Red,
                JobState::Paused => Color::Yellow,
                _ => Color::Magenta,
            };
            execute!(stdout(), SetForegroundColor(color)).ok();
            let speed = if item.state == JobState::Running {
                human_speed(item.speed)
            } else {
                "—".into()
            };
            let eta = item.eta.map(format_duration).unwrap_or_else(|| "—".into());
            let detail = item.error.as_deref().unwrap_or("");
            writeln!(
                stdout(),
                " {} {:<32} {:>3}% {} {:>10}  eta {:>7} {}",
                item.state.glyph(),
                item.label.chars().take(32).collect::<String>(),
                (item.fraction() * 100.0) as u32,
                bar(item, 32),
                speed,
                eta,
                detail.chars().take(40).collect::<String>()
            )
            .ok();
            execute!(stdout(), ResetColor).ok();
        }
        stdout().flush().ok();
        rendered = latest.len();
    }
    let mut failures = Vec::new();
    let mut total_bytes = 0u64;
    for handle in handles {
        match handle.await {
            Ok(Ok(path)) => {
                total_bytes += std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!("  ✓ saved {}", path.display());
            }
            Ok(Err(error)) => failures.push(error.to_string()),
            Err(error) => failures.push(error.to_string()),
        }
    }
    let elapsed = started.elapsed().as_secs_f64().max(0.1);
    println!(
        "  {} file(s) · {} · avg {}",
        count,
        human_bytes(total_bytes),
        human_speed(total_bytes as f64 / elapsed)
    );
    if !failures.is_empty() {
        bail!(
            "{} download(s) failed: {}",
            failures.len(),
            failures.join("; ")
        );
    }
    Ok(())
}

fn format_duration(seconds: f64) -> String {
    let seconds = seconds.round().max(0.0) as u64;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn is_final(state: JobState) -> bool {
    matches!(
        state,
        JobState::Done | JobState::Failed | JobState::Cancelled
    )
}

fn bar(progress: &JobProgress, width: usize) -> String {
    let filled = (progress.fraction() * width as f64) as usize;
    let filled = filled.min(width);
    let mut out = String::with_capacity(width + 2);
    out.push('[');
    for i in 0..width {
        if i < filled {
            out.push('━');
        } else {
            out.push('·');
        }
    }
    out.push(']');
    out
}

pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

pub fn human_speed(bytes_per_second: f64) -> String {
    if bytes_per_second <= 0.0 {
        "—".into()
    } else {
        format!("{}/s", human_bytes(bytes_per_second as u64))
    }
}
