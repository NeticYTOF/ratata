use super::app::{App, Phase};
use super::theme;
use crate::download::{human_bytes, human_speed};
use crate::model::{JobProgress, JobState};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

pub(crate) fn draw(app: &App, frame: &mut Frame) {
    let palette = theme::palette(app.color_mode);
    let area = frame.area();
    if area.width < 40 || area.height < 8 {
        frame.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(area);

    draw_header(app, frame, chunks[0], &palette);
    if app.phase == Phase::Summary {
        draw_summary(app, frame, chunks[1], &palette);
    } else {
        draw_jobs(app, frame, chunks[1], &palette);
    }
    draw_footer(app, frame, chunks[2], &palette);
}

fn draw_header(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let spinner = SPINNER[(app.tick / 4) as usize % SPINNER.len()];
    let title: String = if app.phase == Phase::Summary {
        "✓".to_owned()
    } else {
        spinner.to_string()
    };
    let line = Line::from(vec![
        Span::styled(
            format!(" {title} "),
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "ratata",
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "  ·  {} download{}  ·  {}",
                app.jobs.len(),
                if app.jobs.len() == 1 { "" } else { "s" },
                app.message
            ),
            Style::default().fg(palette.muted),
        ),
    ]);

    let elapsed = app.started.elapsed().as_secs();
    let average = human_speed(app.average_speed());
    let flow = if app.phase == Phase::Summary {
        average.clone()
    } else {
        human_speed(app.total_speed)
    };
    let total = human_bytes(app.total_bytes);
    let width = area.width.saturating_sub(2) as usize;
    let gauge_width = width.saturating_sub(58).clamp(8, 28);
    let effective = if app.phase == Phase::Summary { app.average_speed() } else { app.total_speed };
    let filled = ((effective / 24_000_000.0).clamp(0.0, 1.0) * gauge_width as f64) as usize;
    let gauge: String = (0..gauge_width)
        .map(|i| if i < filled { '━' } else { '·' })
        .collect();
    let flow_line = Line::from(vec![
        Span::styled(" flow ", Style::default().fg(palette.muted)),
        Span::styled(gauge, Style::default().fg(palette.accent)),
        Span::styled(
            format!(" {flow:>12}  avg {average:>12}  {total:>10}  {elapsed:>3}s"),
            Style::default().fg(palette.text),
        ),
    ]);
    let rule = Line::from(Span::styled(
        "─".repeat(width),
        Style::default().fg(palette.border),
    ));
    frame.render_widget(Paragraph::new(vec![line, flow_line, rule]), area);
}

fn draw_jobs(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (index, job) in app.jobs.iter().enumerate() {
        let Some(progress) = app.progress.get(&job.id) else {
            continue;
        };
        let selected = index == app.selected;
        let marker = if selected { "›" } else { " " };
        let badge = badge(&job.media.platform);
        let color = match progress.state {
            JobState::Done => palette.success,
            JobState::Failed | JobState::Cancelled => palette.error,
            JobState::Paused => palette.warning,
            _ => palette.text,
        };
        let title_width = width
            .saturating_sub(4 + 3 + 15 + 5 + 15 + 13 + 9 + 2)
            .max(12);
        let title = truncate(&progress.label, title_width);
        let size = match progress.total {
            Some(total) => format!(
                "{}/{}",
                human_bytes(progress.downloaded),
                human_bytes(total)
            ),
            None => human_bytes(progress.downloaded),
        };
        let percent = format!("{:>3}%", (progress.fraction() * 100.0) as u32);
        let bar_width = 12usize;
        let bar = bar(progress, bar_width, palette);
        let speed = if progress.state == JobState::Running {
            human_speed(progress.speed)
        } else {
            "—".to_owned()
        };
        let eta = progress
            .eta
            .map(format_eta)
            .unwrap_or_else(|| "—".to_owned());
        let row = format!(
            "{marker} {badge:<3} {title:<tw$} {size:>15} {percent:>4} {bar} {speed:>12} {eta:>7}",
            tw = title_width
        );
        lines.push(Line::from(vec![Span::styled(
            row,
            Style::default().fg(color).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        )]));
        if let Some(error) = &progress.error {
            lines.push(Line::from(Span::styled(
                format!("    {error}"),
                Style::default().fg(palette.error),
            )));
        }
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_summary(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let done = app
        .progress
        .values()
        .filter(|p| p.state == JobState::Done)
        .count();
    let failed = app
        .progress
        .values()
        .filter(|p| matches!(p.state, JobState::Failed | JobState::Cancelled))
        .count();
    let bytes: u64 = app.progress.values().map(|p| p.downloaded).sum();
    let elapsed = app.started.elapsed().as_secs();
    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  complete",
            Style::default()
                .fg(if failed == 0 {
                    palette.success
                } else {
                    palette.warning
                })
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(
                "  {} file{}  ·  {}  ·  {}  ·  avg {}",
                done,
                if done == 1 { "" } else { "s" },
                human_bytes(bytes),
                format_eta(elapsed as f64),
                human_speed(app.average_speed())
            ),
            Style::default().fg(palette.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  press q to quit",
            Style::default().fg(palette.muted),
        )),
    ];
    let block = Paragraph::new(lines).alignment(Alignment::Left);
    frame.render_widget(block, area);
}

fn draw_footer(_app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let rule = Line::from(Span::styled(
        "─".repeat(area.width.saturating_sub(1) as usize),
        Style::default().fg(palette.border),
    ));
    let keys = Line::from(vec![
        Span::styled(
            " q ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("quit  ", Style::default().fg(palette.muted)),
        Span::styled(
            "p ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("pause  ", Style::default().fg(palette.muted)),
        Span::styled(
            "↑/↓ ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("select", Style::default().fg(palette.muted)),
    ]);
    frame.render_widget(Paragraph::new(vec![rule, keys]), area);
}

fn badge(platform: &str) -> &'static str {
    match platform.to_ascii_lowercase().as_str() {
        "youtube" => "YT",
        "instagram" => "IG",
        "x" | "twitter" => "X",
        "tiktok" => "TT",
        "reddit" => "RD",
        "vimeo" => "VM",
        "bilibili" => "BL",
        "soundcloud" => "SC",
        "dailymotion" => "DM",
        "twitch" => "TW",
        "kick" => "KC",
        "streamable" => "SB",
        "facebook" => "FB",
        "imgur" => "IM",
        "pinterest" => "PN",
        _ => "WEB",
    }
}

fn bar(progress: &JobProgress, width: usize, _palette: &theme::Palette) -> String {
    let filled = (progress.fraction() * width as f64) as usize;
    (0..width)
        .map(|index| if index < filled { '━' } else { '·' })
        .collect()
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn format_eta(seconds: f64) -> String {
    let seconds = seconds.round().max(0.0) as u64;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m{:02}s", seconds / 60, seconds % 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}
