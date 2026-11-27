pub mod app;
pub mod render;
pub mod theme;

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
    let result = app::run_screen(&mut terminal, &manager, jobs, rx).await;
    let mut screen = stdout();
    terminal.show_cursor()?;
    disable_raw_mode()?;
    execute!(screen, LeaveAlternateScreen, crossterm::cursor::Show)?;

    for handle in handles {
        match handle.await {
            Ok(Ok(path)) => println!("  ✓ saved {}", path.display()),
            Ok(Err(error)) => eprintln!("  ✗ {}", error),
            Err(error) => eprintln!("  ✗ {}", error),
        }
    }
    result
}
