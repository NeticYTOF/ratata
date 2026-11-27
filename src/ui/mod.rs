pub mod theme;
pub mod tunnel;

use crate::Config;
use crate::download::DownloadManager;
use crate::download::{human_bytes, human_speed};
use crate::model::{DownloadJob, JobProgress, JobState};
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use futures_util::StreamExt;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Layout};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use reqwest::Client;
use std::collections::BTreeMap;
use std::io::{Stdout, stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// Force Windows consoles into VT/ANSI mode; harmless on other platforms.
pub fn enable_color() {
    #[cfg(windows)]
    {
        use crossterm::style::force_color_output;
        force_color_output(true);
    }
}

pub async fn run(jobs: Vec<DownloadJob>, config: Config, client: Arc<Client>) -> Result<()> {
    enable_color();
    let (tx, rx) = mpsc::unbounded_channel::<JobProgress>();
    let manager = Arc::new(DownloadManager::new(
        config.clone(),
        (*client).clone(),
        Some(tx),
    ));
    let handles = manager.clone().run_all(jobs.clone());

    enable_raw_mode()?;
    let mut screen = stdout();
    execute!(screen, EnterAlternateScreen, crossterm::cursor::Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(screen))?;
    let result = App::run(&mut terminal, &manager, jobs, rx).await;
    let mut screen = stdout();
    terminal.show_cursor()?;
    disable_raw_mode()?;
    execute!(screen, LeaveAlternateScreen, crossterm::cursor::Show)?;

    for handle in handles {
        match handle.await {
            Ok(Ok(path)) => println!("  ✽ saved {}", path.display()),
            Ok(Err(error)) => eprintln!("  ✖ {}", error),
            Err(error) => eprintln!("  ✖ {}", error),
        }
    }
    result
}

struct App {
    jobs: Vec<DownloadJob>,
    progress: BTreeMap<usize, JobProgress>,
    selected: usize,
    tick: u64,
    themes: Vec<&'static str>,
    theme: usize,
    message: String,
    started: Instant,
    finished: Option<Instant>,
    total_speed: f64,
}

impl App {
    async fn run(
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        manager: &Arc<DownloadManager>,
        jobs: Vec<DownloadJob>,
        mut rx: mpsc::UnboundedReceiver<JobProgress>,
    ) -> Result<()> {
        let mut app = Self::new(jobs);
        let mut events = EventStream::new();
        let mut timer = tokio::time::interval(Duration::from_millis(33));
        while app.finished.is_none()
            || app
                .finished
                .is_some_and(|at| at.elapsed() < Duration::from_secs(2))
        {
            tokio::select! {
                _ = timer.tick() => app.tick = app.tick.wrapping_add(1),
                event = events.next() => {
                    if let Some(Ok(Event::Key(key))) = event
                        && key.kind == KeyEventKind::Press && app.key(key.code, manager) {
                            break;
                        }
                }
                Some(update) = rx.recv() => app.update(update),
            }
            terminal.draw(|frame| app.draw(frame))?;
        }
        Ok(())
    }

    fn new(jobs: Vec<DownloadJob>) -> Self {
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
                    },
                )
            })
            .collect();
        Self {
            jobs,
            progress,
            selected: 0,
            tick: 0,
            themes: vec!["blossom", "aurora", "ember"],
            theme: 0,
            message: "blooming tunnels open — download underway".into(),
            started: Instant::now(),
            finished: None,
            total_speed: 0.0,
        }
    }

    fn key(&mut self, code: KeyCode, manager: &Arc<DownloadManager>) -> bool {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                manager.cancel();
                self.message = "cancelling blossoms…".into();
                true
            }
            KeyCode::Char('p') | KeyCode::Char(' ') => {
                let paused = !manager.is_paused();
                manager.set_pause(paused);
                self.message = if paused {
                    "tunnel paused".into()
                } else {
                    "tunnel flowing".into()
                };
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
            KeyCode::Char('t') => {
                self.theme = (self.theme + 1) % self.themes.len();
                self.message = format!("theme: {}", self.themes[self.theme]);
                false
            }
            _ => false,
        }
    }

    fn update(&mut self, progress: JobProgress) {
        if let Some(current) = self.progress.get_mut(&progress.id) {
            *current = progress;
        }
        let all_final = self.progress.values().all(|p| {
            matches!(
                p.state,
                JobState::Done | JobState::Failed | JobState::Cancelled
            )
        });
        if all_final && self.finished.is_none() {
            self.finished = Some(Instant::now());
            self.message = "all blooms collected".into();
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame) {
        let palette = theme::palette(self.themes[self.theme]);
        let area = frame.area();
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

        let title = Line::from(vec![
            Span::styled("  ✿ ", Style::default().fg(palette.accent)),
            Span::styled("R A T A T A", Style::default().fg(palette.text).bold()),
            Span::styled(
                "  ──  blossom tunnel downloader  ",
                Style::default().fg(palette.muted),
            ),
            Span::styled("✿  ", Style::default().fg(palette.bloom)),
        ]);
        frame.render_widget(
            Paragraph::new(title).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(palette.glow)),
            ),
            chunks[0],
        );

        let body = Layout::horizontal([Constraint::Percentage(61), Constraint::Percentage(39)])
            .split(chunks[1]);
        let tunnel_block = Block::default()
            .borders(Borders::ALL)
            .title(" ✿ TUNNEL OF BLOOMS ")
            .border_style(Style::default().fg(palette.accent))
            .title_style(Style::default().fg(palette.text).bold());
        let inner = tunnel_block.inner(body[0]);
        frame.render_widget(tunnel_block, body[0]);
        let energy = self
            .progress
            .values()
            .filter(|p| p.state == JobState::Running)
            .count() as f64
            / self.progress.len().max(1) as f64;
        tunnel::draw(frame.buffer_mut(), inner, &palette, self.tick, energy);

        let jobs_block = Block::default()
            .borders(Borders::ALL)
            .title(" BLOOMS IN FLIGHT ")
            .border_style(Style::default().fg(palette.bloom))
            .title_style(Style::default().fg(palette.text).bold());
        let job_area = jobs_block.inner(body[1]);
        self.total_speed = self.progress.values().map(|p| p.speed).sum();
        let downloaded: u64 = self.progress.values().map(|p| p.downloaded).sum();
        let mut lines = vec![
            Line::from(vec![
                Span::styled("  flow ", Style::default().fg(palette.muted)),
                Span::styled(
                    human_speed(self.total_speed),
                    Style::default().fg(palette.accent).bold(),
                ),
                Span::styled("   gathered ", Style::default().fg(palette.muted)),
                Span::styled(human_bytes(downloaded), Style::default().fg(palette.text)),
            ]),
            Line::from(""),
        ];
        for (index, progress) in self.progress.values().enumerate() {
            let selected = index == self.selected;
            let color = match progress.state {
                JobState::Done => ratatui::style::Color::Green,
                JobState::Failed | JobState::Cancelled => ratatui::style::Color::Red,
                JobState::Paused => ratatui::style::Color::Yellow,
                _ => palette.accent,
            };
            let marker = if selected { "▶" } else { " " };
            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {marker} {} ", progress.state.glyph()),
                    Style::default().fg(color),
                ),
                Span::styled(
                    truncate(&progress.label, job_area.width.saturating_sub(14) as usize),
                    Style::default().fg(palette.text),
                ),
                Span::styled(
                    format!(" {:>3}%", (progress.fraction() * 100.0) as u32),
                    Style::default().fg(palette.muted),
                ),
            ]));
            lines.push(self.bar_line(
                progress,
                job_area.width.saturating_sub(2) as usize,
                &palette,
            ));
        }
        let errors: Vec<&str> = self
            .progress
            .values()
            .filter_map(|p| p.error.as_deref())
            .collect();
        if !errors.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  ✖ TUNNEL TROUBLE",
                Style::default().fg(ratatui::style::Color::Red).bold(),
            )));
            for error in errors {
                lines.push(Line::from(Span::styled(
                    format!(
                        "  {}",
                        truncate(error, job_area.width.saturating_sub(4) as usize)
                    ),
                    Style::default().fg(ratatui::style::Color::Red),
                )));
            }
        }
        frame.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(jobs_block),
            body[1],
        );

        let elapsed = self.started.elapsed().as_secs();
        let footer = Line::from(vec![
            Span::styled("  q ", Style::default().fg(palette.accent).bold()),
            Span::styled("quit/cancel  ", Style::default().fg(palette.muted)),
            Span::styled("p/space ", Style::default().fg(palette.accent).bold()),
            Span::styled("pause  ", Style::default().fg(palette.muted)),
            Span::styled("j/k ", Style::default().fg(palette.accent).bold()),
            Span::styled("select  ", Style::default().fg(palette.muted)),
            Span::styled("t ", Style::default().fg(palette.accent).bold()),
            Span::styled("theme  ", Style::default().fg(palette.muted)),
            Span::styled(
                format!("  {}  ·  {elapsed}s", self.message),
                Style::default().fg(palette.text),
            ),
        ]);
        frame.render_widget(
            Paragraph::new(footer).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(palette.glow)),
            ),
            chunks[2],
        );
    }

    fn bar_line(
        &self,
        progress: &JobProgress,
        width: usize,
        palette: &theme::Palette,
    ) -> Line<'static> {
        let inner = width.saturating_sub(2).min(80);
        let filled = (progress.fraction() * inner as f64) as usize;
        let mut bar = String::with_capacity(inner + 2);
        bar.push('╭');
        for index in 0..inner {
            if index < filled {
                bar.push(if index % 5 == 0 { '✿' } else { '─' });
            } else if index == filled && progress.state == JobState::Running {
                bar.push('❃');
            } else {
                bar.push('·');
            }
        }
        bar.push('╯');
        Line::from(Span::styled(
            format!("  {bar}  {}", human_speed(progress.speed)),
            Style::default().fg(palette.accent),
        ))
    }
}

fn truncate(value: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let chars: String = value.chars().take(max).collect();
    if value.chars().count() > max {
        format!("{chars}…")
    } else {
        chars
    }
}
