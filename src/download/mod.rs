pub mod direct;
pub mod progress;
pub mod segment;
pub mod state;
use crate::config::Config;
use crate::model::{DownloadJob, JobProgress, JobState, ManifestKind};
use anyhow::{Result, bail};
use reqwest::Client;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;

pub struct DownloadManager {
    pub config: Config,
    pub client: Client,
    pub tx: mpsc::UnboundedSender<JobProgress>,
    pub pause: Arc<AtomicBool>,
    pub cancel: Arc<AtomicBool>,
    global: Arc<Semaphore>,
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
            global,
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

    pub async fn download(&self, job: DownloadJob) -> Result<std::path::PathBuf> {
        let _permit = self.global.acquire().await?;
        let total = job.format.size;
        let tracker =
            progress::ProgressTracker::new(self.tx.clone(), job.id, job.media.title.clone(), total);
        let reporter = tracker.spawn();
        tracker.set_state(JobState::Running);
        let result = match job.format.manifest {
            Some(ManifestKind::Hls) => crate::media::hls::download(self, &job, &tracker).await,
            Some(ManifestKind::Dash) => crate::media::dash::download(self, &job, &tracker).await,
            None => direct::download(self, &job, &tracker).await,
        };
        tracker.set_state(match &result {
            Ok(_) => JobState::Done,
            Err(_) if self.cancel.load(Ordering::Relaxed) => JobState::Cancelled,
            Err(_) => JobState::Failed,
        });
        reporter.abort();
        let _ = self.tx.send(JobProgress {
            id: job.id,
            label: job.media.title.clone(),
            downloaded: tracker.downloaded(),
            total,
            speed: 0.0,
            state: match &result {
                Ok(_) => JobState::Done,
                Err(_) if self.cancel.load(Ordering::Relaxed) => JobState::Cancelled,
                Err(_) => JobState::Failed,
            },
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
    use std::io::{Write, stdout};

    crate::ui::enable_color();
    let (tx, mut rx) = mpsc::unbounded_channel::<JobProgress>();
    let manager = Arc::new(DownloadManager::new(
        config.clone(),
        (*client).clone(),
        Some(tx),
    ));
    let handles = manager.run_all(jobs);
    let count = handles.len();
    let mut latest: Vec<JobProgress> = Vec::new();
    let mut rendered = 0usize;
    while latest.iter().filter(|p| is_final(p.state)).count() < count {
        let Some(progress) = rx.recv().await else {
            break;
        };
        if let Some(existing) = latest.iter_mut().find(|p| p.id == progress.id) {
            *existing = progress;
        } else {
            latest.push(progress);
        }
        if rendered > 0 {
            execute!(stdout(), cursor::MoveUp(rendered as u16)).ok();
        }
        latest.sort_by_key(|p| p.id);
        for item in &latest {
            execute!(stdout(), Clear(ClearType::CurrentLine)).ok();
            let color = match item.state {
                JobState::Done => Color::Green,
                JobState::Failed | JobState::Cancelled => Color::Red,
                JobState::Paused => Color::Yellow,
                _ => Color::Magenta,
            };
            execute!(stdout(), SetForegroundColor(color)).ok();
            let bar = bar(item, 42);
            let speed = human_speed(item.speed);
            let percent = (item.fraction() * 100.0) as u32;
            writeln!(
                stdout(),
                " {} {} {:>3}% {} {:>10}  {}",
                item.state.glyph(),
                item.label.chars().take(38).collect::<String>(),
                percent,
                bar,
                speed,
                item.total.map(human_bytes).unwrap_or_else(|| "live".into())
            )
            .ok();
            execute!(stdout(), ResetColor).ok();
        }
        stdout().flush().ok();
        rendered = latest.len();
    }
    let mut failures = Vec::new();
    for handle in handles {
        match handle.await {
            Ok(Ok(path)) => println!("  ✽ saved {}", path.display()),
            Ok(Err(error)) => failures.push(error.to_string()),
            Err(error) => failures.push(error.to_string()),
        }
    }
    if !failures.is_empty() {
        bail!(
            "{} download(s) failed: {}",
            failures.len(),
            failures.join("; ")
        );
    }
    Ok(())
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
    out.push('╭');
    for i in 0..width {
        if i < filled {
            out.push(if i % 4 == 0 { '✿' } else { '─' });
        } else {
            out.push('·');
        }
    }
    out.push('╯');
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
