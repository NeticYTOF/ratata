pub mod app;
pub mod render;
pub mod theme;

/// Five-row block glyphs. `█` is an inked cell, space is empty.
pub const GLYPHS: [[&str; 5]; 3] = [
    ["████ ", "█   █", "████ ", "█  █ ", "█   █"], // R
    [" ███ ", "█   █", "█████", "█   █", "█   █"], // A
    ["█████", "  █  ", "  █  ", "  █  ", "  █  "], // T
];

/// Builds the five banner rows for R A T A T A.
pub fn logo() -> [String; 5] {
    let letters = [0usize, 1, 2, 1, 2, 1];
    let mut rows: [String; 5] = Default::default();
    for row in 0..5 {
        let mut line = String::new();
        for (index, letter) in letters.iter().enumerate() {
            if index > 0 {
                line.push(' ');
            }
            line.push_str(GLYPHS[*letter][row]);
        }
        rows[row] = line;
    }
    rows
}

use crate::Config;
use crate::download::DownloadManager;
use crate::model::{DownloadJob, JobProgress};
use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use reqwest::Client;
use std::io::stdout;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Quit,
    Back,
}

/// Force Windows consoles into VT/ANSI mode; harmless on other platforms.
pub fn enable_color() {
    #[cfg(windows)]
    {
        use crossterm::style::force_color_output;
        force_color_output(true);
    }
}

pub async fn run(
    jobs: Vec<DownloadJob>,
    config: Config,
    client: Arc<Client>,
) -> Result<RunOutcome> {
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
    let result = app::run_screen(&mut terminal, &manager, jobs, rx).await;
    let mut screen = stdout();
    terminal.show_cursor()?;
    disable_raw_mode()?;
    execute!(screen, LeaveAlternateScreen, crossterm::cursor::Show)?;

    for handle in handles {
        if let Ok(Err(error)) = handle.await {
            eprintln!("  ✗ {error}");
        }
    }
    result
}
