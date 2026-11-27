use super::theme::{Palette, petal_for};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

const LANES: [f32; 6] = [0.35, 1.38, 2.42, 3.51, 4.63, 5.66];

pub fn draw(buf: &mut Buffer, area: Rect, palette: &Palette, tick: u64, energy: f64) {
    if area.width < 12 || area.height < 8 {
        return;
    }
    let cx = area.x as f32 + area.width as f32 / 2.0;
    let cy = area.y as f32 + area.height as f32 / 2.0;
    let layers = 30u32;
    let speed = (0.20 + energy * 0.35) as f32;

    for layer in (0..layers).rev() {
        let phase = ((layer as f32 + tick as f32 * speed) % layers as f32) / layers as f32;
        let scale = phase.max(0.015);
        let rx = area.width as f32 * 0.60 * scale;
        let ry = area.height as f32 * 0.72 * scale * scale;
        let glyph = match layer % 4 {
            0 => '·',
            1 => '∘',
            2 => '◦',
            _ => '∙',
        };
        let color = petal_for(palette, layer as usize, tick);
        let points = (16 + (scale * 72.0) as usize).min(96);
        for step in 0..points {
            let angle = std::f32::consts::TAU * step as f32 / points as f32;
            let x = (cx + angle.cos() * rx).round() as i32;
            let y = (cy + angle.sin() * ry).round() as i32;
            if inside(area, x, y) {
                let cell = &mut buf[(x as u16, y as u16)];
                cell.set_char(glyph);
                cell.set_style(Style::default().fg(color));
            }
        }
    }

    // Perspective ribs make the rings read as a tunnel rather than a spiral.
    for rib in 0..16 {
        let angle = std::f32::consts::TAU * rib as f32 / 16.0 + tick as f32 * 0.004;
        let color = palette.petals[rib as usize % palette.petals.len()];
        for step in 6..24 {
            let radius = step as f32 / 24.0;
            let x = (cx + angle.cos() * area.width as f32 * 0.58 * radius * radius).round() as i32;
            let y = (cy + angle.sin() * area.height as f32 * 0.70 * radius * radius).round() as i32;
            if inside(area, x, y) {
                let cell = &mut buf[(x as u16, y as u16)];
                cell.set_char('·');
                cell.set_style(Style::default().fg(color));
            }
        }
    }

    for (lane, base_angle) in LANES.iter().enumerate() {
        let phase =
            ((tick as f32 * speed * 0.85 + lane as f32 * 5.2) % layers as f32) / layers as f32;
        let scale = phase.max(0.02);
        let angle = base_angle + tick as f32 * 0.006 * if lane % 2 == 0 { 1.0 } else { -1.0 };
        let x = (cx + angle.cos() * area.width as f32 * 0.58 * scale).round() as i32;
        let y = (cy + angle.sin() * area.height as f32 * 0.70 * scale * scale).round() as i32;
        let color = palette.petals[lane % palette.petals.len()];
        let glyphs = if scale > 0.42 {
            ['✿', '❀', '✽', '❁']
        } else {
            ['✾', '✿', '❃', '✽']
        };
        draw_flower(buf, area, x, y, glyphs[lane % glyphs.len()], color, scale);
    }

    let spin = tick / 5;
    for petal in 0..8 {
        let angle = std::f32::consts::TAU * petal as f32 / 8.0 + spin as f32 * 0.08;
        let x = (cx + angle.cos() * 2.4).round() as i32;
        let y = (cy + angle.sin() * 1.3).round() as i32;
        if inside(area, x, y) {
            let cell = &mut buf[(x as u16, y as u16)];
            cell.set_char(if petal % 2 == 0 { '✿' } else { '❀' });
            cell.set_style(Style::default().fg(palette.bloom));
        }
    }
    if inside(area, cx as i32, cy as i32) {
        buf[(cx as u16, cy as u16)]
            .set_char('✽')
            .set_style(Style::default().fg(Color::White).bg(palette.glow));
    }
}

fn draw_flower(
    buf: &mut Buffer,
    area: Rect,
    x: i32,
    y: i32,
    glyph: char,
    color: Color,
    scale: f32,
) {
    if !inside(area, x, y) {
        return;
    }
    let cell = &mut buf[(x as u16, y as u16)];
    cell.set_char(glyph);
    cell.set_style(Style::default().fg(color));
    if scale > 0.35 {
        for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
            let nx = x + dx;
            let ny = y + dy;
            if inside(area, nx, ny) {
                let cell = &mut buf[(nx as u16, ny as u16)];
                cell.set_char(if dx == 0 { '│' } else { '─' });
                cell.set_style(Style::default().fg(color));
            }
        }
    }
}

fn inside(area: Rect, x: i32, y: i32) -> bool {
    x >= area.x as i32
        && y >= area.y as i32
        && x < (area.x + area.width) as i32
        && y < (area.y + area.height) as i32
}
