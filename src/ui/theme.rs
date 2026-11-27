use ratatui::style::Color;

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

pub fn palette(name: &str) -> Palette {
    match name.to_ascii_lowercase().as_str() {
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
    }
}

pub fn petal_for(palette: &Palette, depth: usize, tick: u64) -> Color {
    let index = (depth + (tick / 6) as usize) % palette.petals.len();
    palette.petals[index]
}
