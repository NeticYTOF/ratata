use super::app::{App, Phase};
use super::theme;
use crate::download::{human_bytes, human_speed};
use crate::model::JobState;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const LOGO: [&str; 5] = [
    "█████ █████ █████ █████ █████ █████",
    "█   █ █   █   █   █   █ █   █ █   █",
    "█████ █████   █   █████   █   █████",
    "█  █  █   █   █   █   █ █   █ █   █",
    "█   █ █   █ █████ █   █ █████ █   █",
];
const SUBCELL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];

pub(crate) fn draw(app: &App, frame: &mut Frame) {
    let palette = theme::palette(app.color_mode);
    let area = frame.area();
    if area.width < 40 || area.height < 8 {
        frame.render_widget(Paragraph::new("terminal too small"), area);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(7),
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

fn top_line(width: usize, palette: &theme::Palette) -> Line<'static> {
    border_line('╭', '╮', width, palette)
}

fn bottom_line(width: usize, palette: &theme::Palette) -> Line<'static> {
    border_line('╰', '╯', width, palette)
}

fn border_line(left: char, right: char, width: usize, palette: &theme::Palette) -> Line<'static> {
    let inner = width.saturating_sub(2);
    Line::from(Span::styled(
        format!("{left}{}{right}", "─".repeat(inner)),
        Style::default().fg(palette.border),
    ))
}

fn display_width(value: &str) -> usize {
    value.width()
}

fn truncate_display(value: &str, max: usize) -> String {
    if display_width(value) <= max {
        return value.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in value.chars() {
        let width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + width > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        used += width;
    }
    out.push('…');
    out
}

fn panel_line(
    content: &str,
    width: usize,
    _palette: &theme::Palette,
    color: ratatui::style::Color,
) -> Line<'static> {
    let inner = width.saturating_sub(4);
    let text = truncate_display(content, inner);
    let padding = inner.saturating_sub(display_width(&text));
    Line::from(Span::styled(
        format!("│ {text}{} │", " ".repeat(padding)),
        Style::default().fg(color),
    ))
}

fn draw_header(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width as usize;
    let spinner = if app.phase == Phase::Summary {
        "✓".to_owned()
    } else {
        SPINNER[(app.tick / 4) as usize % SPINNER.len()].to_string()
    };
    let status = format!(
        "{spinner}  {} download{}  ·  {}",
        app.jobs.len(),
        if app.jobs.len() == 1 { "" } else { "s" },
        app.message
    );
    let elapsed = app.started.elapsed().as_secs();
    let average = human_speed(app.average_speed());
    let speed = if app.phase == Phase::Summary {
        average.clone()
    } else {
        human_speed(app.total_speed)
    };
    let total = human_bytes(app.total_bytes);
    let gauge_width = width.saturating_sub(62).clamp(8, 24);
    let effective = if app.phase == Phase::Summary {
        app.average_speed()
    } else {
        app.total_speed
    };
    let filled = ((effective / 24_000_000.0).clamp(0.0, 1.0) * gauge_width as f64) as usize;
    let gauge: String = (0..gauge_width)
        .map(|index| if index < filled { '━' } else { '·' })
        .collect();

    let mut rows = vec![top_line(width, palette)];
    for (index, logo) in LOGO.iter().enumerate() {
        let content = match index {
            0 => format!("{logo}   {status}"),
            1 => format!("{logo}   speed {gauge}"),
            2 => format!("{logo}   {speed:>12}  avg {average:>12}  {total:>10}  {elapsed:>3}s"),
            _ => (*logo).to_owned(),
        };
        let color = if index < 2 {
            palette.accent
        } else {
            palette.text
        };
        rows.push(panel_line(&content, width, palette, color));
    }
    rows.push(bottom_line(width, palette));
    frame.render_widget(Paragraph::new(rows), area);
}

fn frame_panel(
    mut lines: Vec<Line<'static>>,
    width: usize,
    height: usize,
    palette: &theme::Palette,
) -> Vec<Line<'static>> {
    let max = height.saturating_sub(2);
    lines.truncate(max);
    let mut framed = Vec::with_capacity(lines.len() + 2);
    framed.push(top_line(width, palette));
    framed.extend(lines);
    framed.push(bottom_line(width, palette));
    framed
}

fn sparkline(history: Option<&Vec<f64>>, width: usize) -> String {
    let Some(history) = history else {
        return " ".repeat(width);
    };
    let recent: Vec<f64> = history.iter().rev().take(width).cloned().collect();
    let max = recent.iter().copied().fold(1.0, f64::max);
    let mut out = String::with_capacity(width);
    for value in recent.iter().rev() {
        let level = ((*value / max).clamp(0.0, 1.0) * 7.0) as usize;
        out.push(SUBCELL[level.min(7)]);
    }
    format!("{out:<width$}")
}

fn draw_jobs(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width as usize;
    let mut lines: Vec<Line> = Vec::new();
    for job in &app.jobs {
        let Some(progress) = app.progress.get(&job.id) else {
            continue;
        };
        let badge = badge(&job.media.platform);
        let color = match progress.state {
            JobState::Done => palette.success,
            JobState::Failed | JobState::Cancelled => palette.error,
            JobState::Paused => palette.warning,
            _ => palette.text,
        };
        let overhead = 4 + 3 + 15 + 5 + 12 + 10 + 12 + 8 + 9;
        let title_width = width.saturating_sub(overhead).max(10);
        let size = match progress.total {
            Some(total) => format!(
                "{}/{}",
                human_bytes(progress.downloaded),
                human_bytes(total)
            ),
            None => human_bytes(progress.downloaded),
        };
        let speed = if progress.state == JobState::Running {
            human_speed(progress.speed)
        } else {
            "—".into()
        };
        let eta = progress.eta.map(format_eta).unwrap_or_else(|| "—".into());
        let row = format!(
            "{} {badge:<3} {:<tw$} {size:>15} {:>3}% {} {} {speed:>12} {eta:>7}",
            progress.state.glyph(),
            truncate_display(&progress.label, title_width),
            (progress.fraction() * 100.0) as u32,
            subcell_bar(progress.fraction(), 10),
            sparkline(app.history.get(&job.id), 8),
            tw = title_width
        );
        lines.push(panel_line(&row, width, palette, color));
        if job.media.platform.contains('→') {
            lines.push(panel_line(
                &format!(
                    "    {} → matched on YouTube Music (audio)",
                    job.media.platform.split(' ').next().unwrap_or("Music")
                ),
                width,
                palette,
                palette.muted,
            ));
        }
        if let Some(error) = &progress.error {
            lines.push(panel_line(
                &format!("    {error}"),
                width,
                palette,
                palette.error,
            ));
        }
    }
    let lines = frame_panel(lines, width, area.height as usize, palette);
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_summary(app: &App, frame: &mut Frame, area: Rect, palette: &theme::Palette) {
    let width = area.width as usize;
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
    let mut lines = vec![panel_line(
        &format!(
            "{} {} file{} · {} · {} · avg {}",
            if failed == 0 { "✓" } else { "!" },
            done,
            if done == 1 { "" } else { "s" },
            human_bytes(bytes),
            format_eta(elapsed as f64),
            human_speed(app.average_speed())
        ),
        width,
        palette,
        if failed == 0 {
            palette.success
        } else {
            palette.warning
        },
    )];
    for job in &app.jobs {
        let Some(progress) = app.progress.get(&job.id) else {
            continue;
        };
        let mark = if progress.state == JobState::Done {
            "✓"
        } else {
            "✗"
        };
        let raw_name = job
            .destination
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| job.destination.to_string_lossy().into_owned());
        let name: String = raw_name
            .chars()
            .filter(|ch| (*ch as u32) < 0x10000 && !ch.is_control())
            .collect();
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
        lines.push(panel_line(
            &format!(
                "{mark} {name}  {}  {}",
                human_bytes(progress.downloaded),
                format_eta(seconds)
            ),
            width,
            palette,
            color,
        ));
    }
    if let Some(folder) = app.jobs.first().and_then(|job| job.destination.parent()) {
        lines.push(panel_line(
            &format!("saved in {}", folder.display()),
            width,
            palette,
            palette.muted,
        ));
    }
    if app.jobs.iter().any(|job| job.media.platform.contains('→')) {
        lines.push(panel_line(
            "audio matched on YouTube Music from a public source, not Spotify/Apple",
            width,
            palette,
            palette.muted,
        ));
    }
    lines.push(panel_line(
        "press any key to quit",
        width,
        palette,
        palette.muted,
    ));
    let lines = frame_panel(lines, width, area.height as usize, palette);
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_footer(frame: &mut Frame, area: Rect, palette: &theme::Palette) {
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
    if platform.contains("Spotify") {
        return "SP";
    }
    if platform.contains("Apple") {
        return "AM";
    }
    if platform.contains("YouTube Music") {
        return "YM";
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{DownloadJob, JobProgress, Media, MediaFormat, MediaKind};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn test_app() -> App {
        let job = DownloadJob {
            id: 0,
            media: Media::new(
                "YouTube",
                "https://example.test/watch",
                "Emoji 🎬 title with wide chars",
            ),
            format: MediaFormat::new("f1", "https://example.test/clip.mp4", MediaKind::Video),
            destination: std::path::PathBuf::from("/tmp/ratata/clip.mp4"),
        };
        let mut app = App::new(vec![job]);
        app.update(JobProgress {
            id: 0,
            label: "Emoji 🎬 title with wide chars".into(),
            downloaded: 50_000_000,
            total: Some(100_000_000),
            speed: 8_000_000.0,
            state: JobState::Done,
            error: None,
            segments: vec![true, false],
            eta: Some(6.0),
        });
        app
    }

    #[test]
    fn panels_have_borders_at_80_and_100_columns() {
        for columns in [80u16, 100u16] {
            let app = test_app();
            let mut terminal = Terminal::new(TestBackend::new(columns, 24)).unwrap();
            terminal.draw(|frame| draw(&app, frame)).unwrap();
            let buffer = terminal.backend().buffer();
            for row in 0..7 {
                let first = buffer[(0, row)].symbol();
                let last = buffer[(columns - 1, row)].symbol();
                assert!(
                    matches!(first, "╭" | "│" | "╰"),
                    "bad left border at {columns}/{row}: {first}"
                );
                assert!(
                    matches!(last, "╮" | "│" | "╯"),
                    "bad right border at {columns}/{row}: {last}"
                );
            }
        }
    }

    #[test]
    fn banner_spells_ratata_snapshot() {
        let expected = [
            "█████ █████ █████ █████ █████ █████",
            "█   █ █   █   █   █   █ █   █ █   █",
            "█████ █████   █   █████   █   █████",
            "█  █  █   █   █   █   █ █   █ █   █",
            "█   █ █   █ █████ █   █ █████ █   █",
        ];
        assert_eq!(LOGO, expected);
        for row in LOGO {
            assert_eq!(display_width(row), 6 * 5 + 5, "banner row: {row}");
        }
    }
}
