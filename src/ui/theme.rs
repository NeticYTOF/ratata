use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    True,
    Ansi256,
    NoColor,
}

impl ColorMode {
    pub fn from_env() -> Self {
        if std::env::var("NO_COLOR").is_ok() {
            return Self::NoColor;
        }
        let colorterm = std::env::var("COLORTERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        let term = std::env::var("TERM")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if colorterm.contains("truecolor") || colorterm.contains("24bit") {
            Self::True
        } else if term.contains("256color") {
            Self::Ansi256
        } else if cfg!(windows) {
            // Windows Terminal and modern conhost support truecolor via VT.
            Self::True
        } else {
            Self::Ansi256
        }
    }
}

pub fn adapt(color: Color, mode: ColorMode) -> Color {
    match (color, mode) {
        (_, ColorMode::NoColor) => Color::Reset,
        (Color::Rgb(r, g, b), ColorMode::Ansi256) => {
            let level = |v: u8| -> u8 { (u16::from(v) * 5 / 255) as u8 };
            Color::Indexed(16 + 36 * level(r) + 6 * level(g) + level(b))
        }
        (other, _) => other,
    }
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub name: &'static str,
    pub background: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub bloom: Color,
    pub glow: Color,
    pub petals: [Color; 5],
}

pub fn palette(name: &str, mode: ColorMode) -> Palette {
    let raw = match name.to_ascii_lowercase().as_str() {
        "aurora" => Palette {
            name: "aurora",
            background: Color::Reset,
            text: Color::Rgb(220, 250, 255),
            muted: Color::Rgb(82, 148, 172),
            accent: Color::Rgb(64, 226, 214),
            bloom: Color::Rgb(118, 164, 255),
            glow: Color::Rgb(26, 108, 138),
            petals: [
                Color::Rgb(64, 226, 214),
                Color::Rgb(88, 190, 255),
                Color::Rgb(140, 150, 255),
                Color::Rgb(196, 130, 255),
                Color::Rgb(236, 200, 255),
            ],
        },
        "ember" => Palette {
            name: "ember",
            background: Color::Reset,
            text: Color::Rgb(255, 238, 220),
            muted: Color::Rgb(148, 96, 82),
            accent: Color::Rgb(255, 140, 72),
            bloom: Color::Rgb(255, 92, 120),
            glow: Color::Rgb(120, 34, 52),
            petals: [
                Color::Rgb(255, 196, 92),
                Color::Rgb(255, 140, 72),
                Color::Rgb(255, 92, 120),
                Color::Rgb(226, 64, 156),
                Color::Rgb(255, 214, 190),
            ],
        },
        _ => Palette {
            name: "blossom",
            background: Color::Reset,
            text: Color::Rgb(255, 232, 246),
            muted: Color::Rgb(126, 82, 132),
            accent: Color::Rgb(255, 105, 180),
            bloom: Color::Rgb(196, 92, 255),
            glow: Color::Rgb(94, 34, 118),
            petals: [
                Color::Rgb(255, 105, 180),
                Color::Rgb(236, 96, 220),
                Color::Rgb(196, 92, 255),
                Color::Rgb(138, 108, 255),
                Color::Rgb(255, 186, 224),
            ],
        },
    };
    Palette {
        name: raw.name,
        background: adapt(raw.background, mode),
        text: adapt(raw.text, mode),
        muted: adapt(raw.muted, mode),
        accent: adapt(raw.accent, mode),
        bloom: adapt(raw.bloom, mode),
        glow: adapt(raw.glow, mode),
        petals: raw.petals.map(|c| adapt(c, mode)),
    }
}

pub fn petal_for(palette: &Palette, depth: usize, tick: u64) -> Color {
    let index = (depth + (tick / 6) as usize) % palette.petals.len();
    palette.petals[index]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rgb_to_ansi256_and_resets_for_no_color() {
        assert!(matches!(
            adapt(Color::Rgb(255, 105, 180), ColorMode::Ansi256),
            Color::Indexed(_)
        ));
        assert_eq!(
            adapt(Color::Rgb(255, 105, 180), ColorMode::NoColor),
            Color::Reset
        );
        assert_eq!(
            adapt(Color::Rgb(1, 2, 3), ColorMode::True),
            Color::Rgb(1, 2, 3)
        );
    }
}
