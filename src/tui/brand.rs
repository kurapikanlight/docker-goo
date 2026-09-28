use super::theme::*;
use ratatui::{layout::Rect, style::Color, Frame};
use std::sync::OnceLock;
static WORD: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
static ICON: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
fn color(pixel: u8, x: f64, phase: f64) -> Color {
    match pixel {
        b'w' => WHITE,
        b'g' => {
            let t = (x * 2. + phase).rem_euclid(2.);
            blend(
                Color::Rgb(29, 99, 237),
                PURPLE,
                if t <= 1. { t } else { 2. - t },
            )
        }
        _ => Color::Rgb(29, 99, 237),
    }
}
pub fn draw(frame: &mut Frame, area: Rect, phase: f64, bg: Color) {
    let word = WORD.get_or_init(|| {
        include_str!("../../assets/wordmark.mask")
            .lines()
            .map(|s| s.as_bytes().to_vec())
            .collect()
    });
    let icon = ICON.get_or_init(|| {
        include_str!("../../assets/whale.mask")
            .lines()
            .map(|s| s.as_bytes().to_vec())
            .collect()
    });
    if area.width < 40 {
        render(frame, area, phase, bg, word);
        return;
    }
    let iw = (area.width / 5).min(area.height * 2).max(1);
    render(
        frame,
        Rect::new(area.x, area.y, iw, area.height),
        phase,
        bg,
        icon,
    );
    render(
        frame,
        Rect::new(
            area.x + iw + 1,
            area.y,
            area.width.saturating_sub(iw + 1),
            area.height,
        ),
        phase,
        bg,
        word,
    );
}
fn render(frame: &mut Frame, a: Rect, phase: f64, bg: Color, mask: &[Vec<u8>]) {
    let sw = mask[0].len();
    let sh = mask.len();
    let compact = a.width < 45;
    let sx = if compact { 2 } else { 1 };
    let sy = if compact { 4 } else { 2 };
    let w = (a.width as usize * sx).min(a.height as usize * sy * sw / sh);
    if w == 0 {
        return;
    }
    let h = (w * sh / sw).max(1);
    let rows = h.div_ceil(sy);
    let y0 = a.y + a.height.saturating_sub(rows as u16) / 2;
    let sample = |x: usize, y: usize| {
        if x < w && y < h {
            mask[y * sh / h][x * sw / w]
        } else {
            b'0'
        }
    };
    for y in 0..rows {
        for x in 0..w.div_ceil(sx) {
            let (ch, fg, back) = if compact {
                let mut bits = 0;
                let mut pixel = b'd';
                for (dy, pair) in [[0, 3], [1, 4], [2, 5], [6, 7]].iter().enumerate() {
                    for (dx, bit) in pair.iter().enumerate() {
                        let p = sample(x * 2 + dx, y * 4 + dy);
                        if p != b'0' {
                            bits |= 1 << bit;
                            pixel = p;
                        }
                    }
                }
                (
                    char::from_u32(0x2800 + bits).unwrap_or(' '),
                    color(pixel, x as f64 * 2. / w as f64, phase),
                    bg,
                )
            } else {
                let p = sample(x, y * 2);
                let q = sample(x, y * 2 + 1);
                let c = |p| color(p, x as f64 / w as f64, phase);
                match (p != b'0', q != b'0') {
                    (true, true) if std::env::var_os("NO_COLOR").is_some() => ('█', WHITE, bg),
                    (true, true) => ('▀', c(p), c(q)),
                    (true, false) => ('▀', c(p), bg),
                    (false, true) => ('▄', c(q), bg),
                    _ => (' ', WHITE, bg),
                }
            };
            if let Some(cell) = frame.buffer_mut().cell_mut((a.x + x as u16, y0 + y as u16)) {
                cell.set_char(ch).set_fg(fg).set_bg(back);
            }
        }
    }
}

pub fn baseline(area: Rect) -> u16 {
    let Some(mask) = WORD.get() else {
        return area.bottom().saturating_sub(1);
    };
    let iw = (area.width / 5).min(area.height * 2).max(1);
    let width = area.width.saturating_sub(iw + 1) as usize;
    let w = width.min(area.height as usize * 2 * mask[0].len() / mask.len());
    let h = (w * mask.len() / mask[0].len()).max(1).div_ceil(2) as u16;
    area.y + area.height.saturating_sub(h) / 2 + h.saturating_sub(1)
}
