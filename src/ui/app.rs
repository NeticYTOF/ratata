use super::render;
use crate::download::DownloadManager;
use crate::model::{DownloadJob, JobProgress, JobState};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::collections::BTreeMap;
use std::io::Stdout;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Phase {
    Running,
    Summary,
}

pub(crate) struct App {
    pub(crate) jobs: Vec<DownloadJob>,
    pub(crate) progress: BTreeMap<usize, JobProgress>,
    pub(crate) history: BTreeMap<usize, Vec<f64>>,
    pub(crate) last_seen: BTreeMap<usize, (u64, Instant)>,
    pub(crate) selected: usize,
    pub(crate) tick: u64,
    pub(crate) color_mode: super::theme::ColorMode,
    pub(crate) message: String,
    pub(crate) started: Instant,
    pub(crate) finished: Option<Instant>,
    pub(crate) finished_at: BTreeMap<usize, Instant>,
    pub(crate) phase: Phase,
    pub(crate) total_speed: f64,
    pub(crate) total_bytes: u64,
}

pub(crate) async fn run_screen(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    manager: &Arc<DownloadManager>,
    jobs: Vec<DownloadJob>,
    mut rx: mpsc::UnboundedReceiver<JobProgress>,
) -> Result<()> {
    let mut app = App::new(jobs);
    let mut events = EventStream::new();
    let mut timer = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            _ = timer.tick() => app.tick = app.tick.wrapping_add(1),
            event = events.next() => {
                if let Some(Ok(Event::Key(key))) = event
                    && key.kind == KeyEventKind::Press
                    && app.key(key.code, manager)
                {
                    break;
                }
            }
            Some(update) = rx.recv() => app.update(update),
        }
        terminal.draw(|frame| render::draw(&app, frame))?;
    }
    Ok(())
}

impl App {
    pub(crate) fn new(jobs: Vec<DownloadJob>) -> Self {
        let progress = jobs
            .iter()
            .map(|job| {
                (
                    job.id,
                    JobProgress {
                        id: job.id,
                        label: job.media.title.clone(),
                        downloaded: 0,
                        total: job.format.size,
                        speed: 0.0,
                        state: JobState::Queued,
                        error: None,
                        segments: Vec::new(),
                        eta: None,
                    },
                )
            })
            .collect();
        let history = jobs.iter().map(|job| (job.id, Vec::new())).collect();
        let last_seen = jobs
            .iter()
            .map(|job| (job.id, (0, Instant::now())))
            .collect();
        let total_bytes = jobs.iter().map(|job| job.format.size.unwrap_or(0)).sum();
        Self {
            jobs,
            progress,
            history,
            last_seen,
            selected: 0,
            tick: 0,
            color_mode: super::theme::ColorMode::from_env(),
            message: "downloads underway".into(),
            started: Instant::now(),
            finished: None,
            finished_at: BTreeMap::new(),
            phase: Phase::Running,
            total_speed: 0.0,
            total_bytes,
        }
    }

    fn key(&mut self, code: KeyCode, manager: &Arc<DownloadManager>) -> bool {
        match code {
            KeyCode::Enter | KeyCode::Char(' ') if self.phase == Phase::Summary => true,
            KeyCode::Char(_) if self.phase == Phase::Summary => true,
            KeyCode::Char('q') | KeyCode::Esc => {
                manager.cancel();
                self.message = "cancelled".into();
                true
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => {
                let paused = !manager.is_paused();
                manager.set_pause(paused);
                self.message = if paused { "paused" } else { "running" }.into();
                for progress in self.progress.values_mut() {
                    if !matches!(
                        progress.state,
                        JobState::Done | JobState::Failed | JobState::Cancelled
                    ) {
                        progress.state = if paused {
                            JobState::Paused
                        } else {
                            JobState::Running
                        };
                    }
                }
                false
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.selected = (self.selected + 1).min(self.jobs.len().saturating_sub(1));
                false
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                false
            }
            _ => false,
        }
    }

    /// Rate is measured from actual byte deltas, not from reporter ticks.
    pub(crate) fn update(&mut self, progress: JobProgress) {
        let now = Instant::now();
        let (previous_bytes, previous_time) = self
            .last_seen
            .get(&progress.id)
            .copied()
            .unwrap_or((0, self.started));
        let elapsed = now.duration_since(previous_time).as_secs_f64().max(0.02);
        let measured = (progress.downloaded.saturating_sub(previous_bytes)) as f64 / elapsed;
        self.last_seen
            .insert(progress.id, (progress.downloaded, now));

        if let Some(current) = self.progress.get_mut(&progress.id) {
            let mut incoming = progress.clone();
            if incoming.segments.is_empty() {
                incoming.segments = current.segments.clone();
            }
            let effective = if progress.speed > 1.0 {
                progress.speed
            } else {
                measured
            };
            incoming.speed = effective;
            if let Some(total) = incoming.total
                && incoming.eta.is_none()
                && effective > 1.0
                && total > incoming.downloaded
            {
                incoming.eta = Some((total - incoming.downloaded) as f64 / effective);
            }
            *current = incoming;
        }
        if let Some(history) = self.history.get_mut(&progress.id)
            && history.len() > 36
        {
            history.remove(0);
        }
        if matches!(
            progress.state,
            JobState::Done | JobState::Failed | JobState::Cancelled
        ) {
            self.finished_at.entry(progress.id).or_insert(now);
        }
        let all_final = self.progress.values().all(|p| {
            matches!(
                p.state,
                JobState::Done | JobState::Failed | JobState::Cancelled
            )
        });
        if all_final && self.finished.is_none() {
            self.finished = Some(Instant::now());
            self.phase = Phase::Summary;
            self.message = "complete".into();
        }
    }

    pub(crate) fn average_speed(&self) -> f64 {
        let elapsed = self.started.elapsed().as_secs_f64().max(0.1);
        let downloaded: u64 = self.progress.values().map(|p| p.downloaded).sum();
        downloaded as f64 / elapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Media, MediaFormat, MediaKind};
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    #[test]
    fn renders_aligned_rows_and_summary() {
        let job = DownloadJob {
            id: 0,
            media: Media::new("YouTube", "https://example.test/watch", "Bloom clip"),
            format: MediaFormat::new("f1", "https://example.test/clip.mp4", MediaKind::Video),
            destination: std::path::PathBuf::from("clip.mp4"),
        };
        let mut app = App::new(vec![job]);
        app.update(JobProgress {
            id: 0,
            label: "Bloom clip".into(),
            downloaded: 50_000_000,
            total: Some(100_000_000),
            speed: 8_000_000.0,
            state: JobState::Running,
            error: None,
            segments: vec![true, false],
            eta: Some(6.0),
        });
        let mut terminal = Terminal::new(TestBackend::new(110, 20)).unwrap();
        terminal.draw(|frame| render::draw(&app, frame)).unwrap();
        let buffer: &Buffer = terminal.backend().buffer();
        let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(
            rendered.contains('▄') && rendered.contains('█'),
            "logo not rendered"
        );
        assert!(rendered.contains("YT"));
        assert!(rendered.contains("Bloom clip"));
        assert!(rendered.contains("50.0") || rendered.contains("MiB"));
    }

    #[test]
    fn renders_at_80_columns() {
        let job = DownloadJob {
            id: 0,
            media: Media::new(
                "YouTube",
                "https://example.test/watch",
                "A very long video title that will be truncated",
            ),
            format: crate::MediaFormat::new(
                "f1",
                "https://example.test/clip.mp4",
                crate::MediaKind::Video,
            ),
            destination: std::path::PathBuf::from("clip.mp4"),
        };
        let mut app = App::new(vec![job]);
        app.update(JobProgress {
            id: 0,
            label: "A very long video title that will be truncated".into(),
            downloaded: 1_000_000,
            total: Some(10_000_000),
            speed: 2_000_000.0,
            state: JobState::Running,
            error: None,
            segments: vec![true, false, false],
            eta: Some(4.0),
        });
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render::draw(&app, frame)).unwrap();
        let buffer: &Buffer = terminal.backend().buffer();
        let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(rendered.contains("YT"));
        assert!(rendered.contains("MiB"));
        assert!(rendered.contains('%'));
    }
}
