use ratatui::{
    layout::Rect,
    style::{Color, Style},
    widgets::Block,
    Frame,
};
pub const WHITE: Color = Color::Rgb(255, 255, 255);
pub const BASE: Color = Color::Rgb(7, 18, 42);
pub const SIDEBAR: Color = Color::Rgb(10, 26, 57);
pub const SURFACE: Color = Color::Rgb(13, 33, 68);
pub const RAISED: Color = Color::Rgb(18, 43, 85);
pub const LINE: Color = Color::Rgb(44, 75, 127);
pub const BLUE: Color = Color::Rgb(0, 35, 102);
pub const PURPLE: Color = Color::Rgb(135, 17, 199);
pub fn text() -> Style {
    Style::default().fg(WHITE)
}
pub fn fill(frame: &mut Frame, area: Rect, bg: Color) {
    frame.render_widget(Block::default().style(text().bg(bg)), area);
}
pub fn blend(a: Color, b: Color, t: f64) -> Color {
    if let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) {
        let v = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t.clamp(0., 1.)) as u8;
        Color::Rgb(v(ar, br), v(ag, bg), v(ab, bb))
    } else {
        a
    }
}
pub fn gradient(frame: &mut Frame, area: Rect, a: Color, b: Color) {
    for x in area.x..area.right() {
        let color = blend(
            a,
            b,
            (x - area.x) as f64 / area.width.saturating_sub(1).max(1) as f64,
        );
        for y in area.y..area.bottom() {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, y)) {
                cell.set_bg(color);
            }
        }
    }
}

pub const BABY: Color = Color::Rgb(161, 202, 241);
pub const NAMES: [&str; 3] = [
    "Classic / Royal accents",
    "Midnight / Black",
    "Daylight / White",
];
pub fn load() -> usize {
    path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| s.trim().parse().ok())
        .filter(|n| *n < 3)
        .unwrap_or(0)
}
fn path() -> Option<std::path::PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| std::path::PathBuf::from(p).join(".config")))
        .map(|p| p.join("docker-goo/theme"))
}
pub fn save(index: usize) -> std::io::Result<()> {
    let p = path().ok_or_else(|| std::io::Error::other("No configuration directory"))?;
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(p, index.to_string())
}
// Convert semantic surface colors after layout so overlays receive the same theme.
pub fn apply(frame: &mut Frame, index: usize) {
    for cell in &mut frame.buffer_mut().content {
        let old = cell.bg;
        let body = old == BASE;
        cell.bg = if body {
            match index {
                1 => Color::Black,
                2 => Color::White,
                _ => old,
            }
        } else {
            old
        };
        if index == 2 && body {
            cell.fg = match cell.fg {
                WHITE | Color::White => Color::Black,
                BLUE | LINE => Color::Rgb(46, 79, 125),
                Color::Rgb(130, 204, 255) | Color::Rgb(112, 190, 255) => Color::Rgb(43, 100, 158),
                Color::Rgb(78, 230, 160) => Color::Rgb(0, 112, 73),
                Color::Rgb(255, 124, 151) => Color::Rgb(166, 30, 70),
                Color::Rgb(255, 204, 102) => Color::Rgb(135, 84, 0),
                other => other,
            };
        } else if cell.fg == LINE {
            cell.fg = LINE;
        } else if cell.fg == BLUE || cell.fg == Color::Rgb(130, 204, 255) {
            cell.fg = BABY;
        }
    }
}

pub fn action_color(key: char) -> Color {
    match key {
        'n' => Color::Rgb(70, 130, 180), // Steel blue
        's' => Color::Rgb(46, 139, 87),  // Sea green
        'w' => Color::Rgb(226, 99, 16),  // Metallic orange
        'x' => Color::Rgb(192, 57, 43),
        'r' => Color::Rgb(210, 153, 34),
        'p' => Color::Rgb(181, 145, 48),
        'u' => Color::Rgb(26, 135, 130),
        'd' => Color::Rgb(173, 48, 80),
        'b' => Color::Rgb(116, 86, 167),
        'e' => Color::Rgb(46, 122, 145),
        'g' => Color::Rgb(46, 139, 87),
        'l' | 'f' => Color::Rgb(70, 130, 180),
        '$' => Color::Rgb(116, 86, 167),
        _ => Color::Rgb(70, 130, 180),
    }
}
