use super::app::{App, Phase};
use super::theme;
use crate::download::{human_bytes, human_speed};
use crate::model::JobState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const LOGO: [&str; 2] = ["▄▀█ █▄▄ ▀█▀ ▄▀█ ▄▀█ ▀█▀", "█▀█ █▄█  █  █▀█ █▀█  █"];
const SUBCELL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

pub(crate) fn draw(app: &App, frame: &mut Frame) {
    let palette = theme::palette(app.color_mode);
    let area = frame.area();
    if area.width < 40 || area.height < 8 {
        frame.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(5),
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
    draw_footer(frame, chunks[2], &palette);
}

fn top_border(width: usize, palette: &theme::Palette) -> Line<'static> {
    Line::from(Span::styled(
        format!("╭{}╯", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(palette.border),
    ))
}

fn bottom_border(width: usize, palette: &theme::Palette) -> Line<'static> {
    Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(width.saturating_sub(2))),
        Style::default().fg(palette.border),
    ))
}

fn draw_header(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let spinner: String = if app.phase == Phase::Summary {
        "✓".to_owned()
    } else {
        SPINNER[(app.tick / 4) as usize % SPINNER.len()].to_string()
    };
    let logo_style = Style::default()
        .fg(palette.accent)
        .add_modifier(Modifier::BOLD);
    let info_style = Style::default().fg(palette.muted);
    let info = format!(
        " {spinner}  {} download{}  ·  {}",
        app.jobs.len(),
        if app.jobs.len() == 1 { "" } else { "s" },
        app.message
    );
    let line1 = Line::from(vec![
        Span::styled(format!(" {}", LOGO[0]), logo_style),
        Span::styled(info, info_style),
    ]);
    let line2 = Line::from(Span::styled(format!(" {}", LOGO[1]), logo_style));
    let width = area.width.saturating_sub(1) as usize;

    let elapsed = app.started.elapsed().as_secs();
    let average = human_speed(app.average_speed());
    let flow = if app.phase == Phase::Summary {
        average.clone()
    } else {
        human_speed(app.total_speed)
    };
    let total = human_bytes(app.total_bytes);
    let gauge_width = width.saturating_sub(58).clamp(8, 24);
    let effective = if app.phase == Phase::Summary {
        app.average_speed()
    } else {
        app.total_speed
    };
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
    frame.render_widget(
        Paragraph::new(vec![
            top_border(width, palette),
            line1,
            line2,
            flow_line,
            bottom_border(width, palette),
        ]),
        area,
    );
}

fn draw_jobs(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width.saturating_sub(1) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (index, job) in app.jobs.iter().enumerate() {
        let Some(progress) = app.progress.get(&job.id) else {
            continue;
        };
        let selected = index == app.selected;
        let badge = badge(&job.media.platform);
        let color = match progress.state {
            JobState::Done => palette.success,
            JobState::Failed | JobState::Cancelled => palette.error,
            JobState::Paused => palette.warning,
            _ => palette.text,
        };
        let overhead = 3 + 3 + 15 + 5 + 12 + 12 + 8 + 8;
        let title_width = width.saturating_sub(overhead).max(10);
        let size = match progress.total {
            Some(total) => format!(
                "{}/{}",
                human_bytes(progress.downloaded),
                human_bytes(total)
            ),
            None => human_bytes(progress.downloaded),
        };
        let percent = format!("{:>3}%", (progress.fraction() * 100.0) as u32);
        let speed = if progress.state == JobState::Running {
            human_speed(progress.speed)
        } else {
            "—".into()
        };
        let eta = progress.eta.map(format_eta).unwrap_or_else(|| "—".into());
        let row = format!(
            "{} {badge:<3} {:<tw$} {size:>15} {percent:>4} {} {speed:>12} {eta:>7}",
            progress.state.glyph(),
            truncate(&progress.label, title_width),
            subcell_bar(progress.fraction(), 10),
            tw = title_width
        );
        lines.push(Line::from(Span::styled(
            row,
            Style::default().fg(color).add_modifier(if selected {
                Modifier::BOLD
            } else {
                Modifier::empty()
            }),
        )));
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
    let width = area.width.saturating_sub(1) as usize;
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
    let mut lines = vec![
        top_border(width, palette),
        Line::from(Span::styled(
            format!(
                " {} {} file{} · {} · {} · avg {}",
                if failed == 0 { "✓" } else { "!" },
                done,
                if done == 1 { "" } else { "s" },
                human_bytes(bytes),
                format_eta(elapsed as f64),
                human_speed(app.average_speed())
            ),
            Style::default()
                .fg(if failed == 0 {
                    palette.success
                } else {
                    palette.warning
                })
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];
    for job in &app.jobs {
        let Some(progress) = app.progress.get(&job.id) else {
            continue;
        };
        let mark = if progress.state == JobState::Done {
            "✓"
        } else {
            "✗"
        };
        let path = truncate(&job.destination.to_string_lossy(), width.saturating_sub(28));
        let seconds = app
            .finished_at
            .get(&job.id)
            .map(|at| at.duration_since(app.started).as_secs_f64())
            .unwrap_or(0.0);
        let color = if progress.state == JobState::Done {
            palette.success
        } else {
            palette.error
        };
        lines.push(Line::from(Span::styled(
            format!(
                " {mark} {path:<w$} {}  {}",
                human_bytes(progress.downloaded),
                format_eta(seconds),
                w = width.saturating_sub(28)
            ),
            Style::default().fg(color),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  press any key to quit",
        Style::default().fg(palette.muted),
    )));
    lines.push(bottom_border(width, palette));
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_footer(frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width.saturating_sub(1) as usize;
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
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "─".repeat(width),
                Style::default().fg(palette.border),
            )),
            keys,
        ]),
        area,
    );
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

fn subcell_bar(fraction: f64, width: usize) -> String {
    let exact = fraction.clamp(0.0, 1.0) * width as f64;
    let full = exact as usize;
    let partial = ((exact - full as f64) * 8.0) as usize;
    let mut out = String::with_capacity(width + 2);
    out.push('[');
    for _ in 0..full.min(width) {
        out.push('█');
    }
    if full < width {
        out.push(SUBCELL[partial.min(7)]);
    }
    for _ in full + 1..width {
        out.push('░');
    }
    out.push(']');
    out
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
