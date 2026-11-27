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
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub border: Color,
}

pub fn palette(mode: ColorMode) -> Palette {
    let raw = [
        Color::Rgb(232, 234, 237), // text
        Color::Rgb(120, 126, 134), // muted
        Color::Rgb(86, 180, 233),  // accent
        Color::Rgb(106, 204, 140), // success
        Color::Rgb(224, 180, 92),  // warning
        Color::Rgb(232, 112, 112), // error
        Color::Rgb(62, 68, 76),    // border
    ];
    let [text, muted, accent, success, warning, error, border] =
        raw.map(|color| adapt(color, mode));
    Palette {
        text,
        muted,
        accent,
        success,
        warning,
        error,
        border,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_modes_degrade_gracefully() {
        assert!(matches!(
            adapt(Color::Rgb(86, 180, 233), ColorMode::Ansi256),
            Color::Indexed(_)
        ));
        assert_eq!(adapt(Color::Rgb(1, 2, 3), ColorMode::NoColor), Color::Reset);
        assert_eq!(
            adapt(Color::Rgb(1, 2, 3), ColorMode::True),
            Color::Rgb(1, 2, 3)
        );
    }
}
