use super::*;
use crossterm::event::MouseButton;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

#[derive(Default)]
pub(super) struct Output {
    regions: Vec<Region>,
    panes: Vec<Pane>,
    selection: Option<Selection>,
    pub notice: String,
}

type Link = (Vec<Position>, String);

struct Region {
    area: Rect,
    links: Vec<Link>,
}

#[derive(Clone)]
struct Pane {
    buffer: Buffer,
    links: Vec<Link>,
}

struct Selection {
    pane: Pane,
    start: Position,
    end: Position,
    dragged: bool,
}

pub(super) enum Gesture {
    Handled,
    Copy(String),
    Open(String),
}

impl Output {
    pub fn begin(&mut self) {
        self.regions.clear();
    }

    pub fn register(&mut self, area: Rect, source: &str, scroll: u16) {
        let mut links: Vec<Link> = Vec::new();
        let lines = source
            .lines()
            .map(|line| {
                let text = clean(line);
                let urls = urls(&text);
                let mut matches = urls
                    .iter()
                    .flat_map(|url| text.match_indices(url).map(move |(start, _)| (start, url)))
                    .collect::<Vec<_>>();
                matches.sort_by_key(|(start, _)| *start);
                let mut spans = Vec::new();
                let mut cursor = 0;
                for (start, url) in matches {
                    if start < cursor {
                        continue;
                    }
                    spans.push(Span::raw(text[cursor..start].to_owned()));
                    let id = links.len() + 1;
                    let marker = Color::Rgb((id >> 16) as u8, (id >> 8) as u8, id as u8);
                    spans.push(Span::styled(url.clone(), Style::default().fg(marker)));
                    links.push((Vec::new(), url.clone()));
                    cursor = start + url.len();
                }
                spans.push(Span::raw(text[cursor..].to_owned()));
                Line::from(spans)
            })
            .collect::<Vec<_>>();
        // Let the same widget map source links through wrapping and scrolling.
        // Colors are IDs in this private buffer; they never reach the terminal.
        let mut markers = Buffer::empty(area);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .render(area, &mut markers);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Color::Rgb(r, g, b) = markers[(x, y)].fg {
                    let id = ((r as usize) << 16) | ((g as usize) << 8) | b as usize;
                    if let Some((positions, _)) = id.checked_sub(1).and_then(|i| links.get_mut(i)) {
                        positions.push(Position::new(x, y));
                    }
                }
            }
        }
        self.regions.push(Region { area, links });
    }

    // Log rows are clipped, so map visible cells to the complete source URL.
    pub fn register_lines(&mut self, area: Rect, lines: &[Line<'_>]) {
        let mut links = Vec::new();
        for (row, line) in lines.iter().take(area.height as usize).enumerate() {
            let text = line.to_string();
            for url in urls(&text) {
                for (start, _) in text.match_indices(&url) {
                    let x = Line::raw(&text[..start]).width().min(area.width as usize) as u16;
                    let end =
                        (x as usize + Line::raw(&url).width()).min(area.width as usize) as u16;
                    let positions = (x..end)
                        .map(|x| Position::new(area.x + x, area.y + row as u16))
                        .collect();
                    links.push((positions, url.clone()));
                }
            }
        }
        self.regions.push(Region { area, links });
    }

    pub fn finish(&mut self, buffer: &mut Buffer) {
        if self
            .selection
            .as_ref()
            .is_some_and(|s| !self.regions.iter().any(|r| r.area == s.pane.buffer.area))
        {
            self.selection = None;
        }
        self.panes = self
            .regions
            .iter()
            .map(|region| {
                if let Some(selection) = &self.selection {
                    if selection.pane.buffer.area == region.area {
                        return selection.pane.clone();
                    }
                }
                Pane::capture(buffer, region)
            })
            .collect();
        for pane in &self.panes {
            for (positions, _) in &pane.links {
                for pos in positions {
                    buffer[*pos].modifier.insert(Modifier::UNDERLINED);
                }
            }
        }
        // Freeze just the selected pane while streaming continues in the background.
        if let Some(selection) = &self.selection {
            let area = selection.pane.buffer.area;
            let (start, end) = selection.bounds();
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    let pos = Position::new(x, y);
                    buffer[pos] = selection.pane.buffer[pos].clone();
                    if selection.dragged && (start..=end).contains(&selection.index(pos)) {
                        buffer[pos].modifier.insert(Modifier::REVERSED);
                    }
                }
            }
        }
    }

    pub fn event(&mut self, event: &Event) -> Option<Gesture> {
        match event {
            Event::Mouse(mouse) => {
                let pos = Position::new(mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.selection = self
                            .panes
                            .iter()
                            .find(|p| p.buffer.area.contains(pos))
                            .map(|pane| Selection {
                                pane: pane.clone(),
                                start: pos,
                                end: pos,
                                dragged: false,
                            });
                        self.selection.as_ref().map(|_| Gesture::Handled)
                    }
                    MouseEventKind::Drag(MouseButton::Left)
                    | MouseEventKind::Up(MouseButton::Left) => {
                        let selection = self.selection.as_mut()?;
                        let area = selection.pane.buffer.area;
                        selection.end = Position::new(
                            pos.x.clamp(area.x, area.right() - 1),
                            pos.y.clamp(area.y, area.bottom() - 1),
                        );
                        selection.dragged |= selection.start != selection.end;
                        if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                            if selection.dragged {
                                return Some(Gesture::Copy(selection.text()));
                            }
                            let link = selection
                                .pane
                                .links
                                .iter()
                                .find(|(positions, _)| positions.contains(&pos))
                                .map(|(_, url)| url.clone());
                            self.selection = None;
                            return Some(link.map_or(Gesture::Handled, Gesture::Open));
                        }
                        Some(Gesture::Handled)
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        self.selection = None;
                        None
                    }
                    _ => None,
                }
            }
            Event::Key(key) if key.kind != KeyEventKind::Release => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    if let Some(selection) = &self.selection {
                        if selection.dragged {
                            return Some(Gesture::Copy(selection.text()));
                        }
                    }
                }
                self.selection = None;
                None
            }
            Event::Resize(..) | Event::Paste(_) => {
                self.selection = None;
                None
            }
            _ => None,
        }
    }
}

impl Pane {
    fn capture(buffer: &Buffer, region: &Region) -> Self {
        let area = region.area;
        let mut copy = Buffer::empty(area);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let pos = Position::new(x, y);
                copy[pos] = buffer[pos].clone();
            }
        }
        Self {
            buffer: copy,
            links: region.links.clone(),
        }
    }
}

impl Selection {
    fn index(&self, pos: Position) -> usize {
        let area = self.pane.buffer.area;
        (pos.y - area.y) as usize * area.width as usize + (pos.x - area.x) as usize
    }

    fn bounds(&self) -> (usize, usize) {
        let (a, b) = (self.index(self.start), self.index(self.end));
        (a.min(b), a.max(b))
    }

    fn text(&self) -> String {
        let area = self.pane.buffer.area;
        let (start, end) = self.bounds();
        // Copy a whole selected link from its source, without display wrapping.
        for (positions, url) in &self.pane.links {
            if let (Some(first), Some(last)) = (positions.first(), positions.last()) {
                if start == self.index(*first) && end == self.index(*last) {
                    return url.clone();
                }
            }
        }
        let mut rows = Vec::new();
        for row in start / area.width as usize..=end / area.width as usize {
            let mut text = String::new();
            let mut skip_until = area.x;
            for x in area.x..area.right() {
                let pos = Position::new(x, area.y + row as u16);
                if x < skip_until {
                    continue;
                }
                let symbol = self.pane.buffer[pos].symbol();
                skip_until = x + Line::raw(symbol).width().max(1) as u16;
                if (start..=end).contains(&self.index(pos)) {
                    text.push_str(symbol);
                }
            }
            rows.push(text.trim_end().to_owned());
        }
        rows.join("\n")
    }
}

fn urls(text: &str) -> Vec<String> {
    let mut urls = Vec::new();
    for (start, _) in text
        .match_indices("http://")
        .chain(text.match_indices("https://"))
    {
        let candidate = &text[start..];
        let end = candidate
            .find(|c: char| {
                c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '"' | '\'' | '`')
            })
            .unwrap_or(candidate.len());
        let mut candidate = candidate[..end].trim_end_matches(['.', ',', ';', ':', '!']);
        for (open, close) in [('(', ')'), ('[', ']'), ('{', '}')] {
            while candidate.ends_with(close)
                && candidate.matches(close).count() > candidate.matches(open).count()
            {
                candidate = &candidate[..candidate.len() - 1];
            }
        }
        if url::Url::parse(candidate).is_ok_and(|url| url.host_str().is_some())
            && !urls.iter().any(|u| u == candidate)
        {
            urls.push(candidate.to_owned());
        }
    }
    urls
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{MouseButton, MouseEvent};
    use ratatui::{
        backend::TestBackend,
        widgets::{Paragraph, Wrap},
        Terminal,
    };

    fn mouse(kind: MouseEventKind, x: u16, y: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn output(text: &str, width: u16) -> Output {
        let mut output = Output::default();
        let mut terminal = Terminal::new(TestBackend::new(width, 4)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), f.area());
                output.register(f.area(), text, 0);
                output.finish(f.buffer_mut());
            })
            .unwrap();
        output
    }

    #[test]
    fn drag_copies_unicode_text_in_either_direction() {
        for (start, end) in [((0, 0), (3, 1)), ((3, 1), (0, 0))] {
            let mut output = output("café 界\nnext line", 20);
            output.event(&mouse(
                MouseEventKind::Down(MouseButton::Left),
                start.0,
                start.1,
            ));
            output.event(&mouse(
                MouseEventKind::Drag(MouseButton::Left),
                end.0,
                end.1,
            ));
            let result = output.event(&mouse(MouseEventKind::Up(MouseButton::Left), end.0, end.1));
            assert!(matches!(result, Some(Gesture::Copy(text)) if text == "café 界\nnext"));
        }
    }

    #[test]
    fn click_opens_complete_wrapped_link_but_drag_copies_it() {
        let text = "http://localhost:8080/a?token=abc";
        let mut output = output(text, 20);
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 2, 1));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 2, 1)), Some(Gesture::Open(url)) if url == text)
        );
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        output.event(&mouse(MouseEventKind::Drag(MouseButton::Left), 3, 0));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 3, 0)), Some(Gesture::Copy(text)) if text == "http")
        );
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 12, 1)), Some(Gesture::Copy(copied)) if copied == text)
        );
    }

    #[test]
    fn partially_visible_wrapped_link_opens_its_complete_url() {
        let url = format!("https://example.com/{}", "a".repeat(100));
        let mut output = output(&url, 20);
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 2, 0));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 2, 0)), Some(Gesture::Open(link)) if link == url)
        );
    }

    #[test]
    fn ignores_unsafe_schemes_and_surrounding_punctuation() {
        let mut output = output("(https://example.com/a(b)). file:///etc/passwd", 60);
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 5, 0));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 5, 0)), Some(Gesture::Open(url)) if url == "https://example.com/a(b)")
        );
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 32, 0));
        assert!(matches!(
            output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 32, 0)),
            Some(Gesture::Handled)
        ));
    }

    #[test]
    fn reselection_uses_visible_frozen_text_when_new_output_arrives() {
        let mut output = output("old output", 20);
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        output.event(&mouse(MouseEventKind::Drag(MouseButton::Left), 2, 0));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 4));
        buffer.set_string(0, 0, "new output", ratatui::style::Style::default());
        output.begin();
        output.register(buffer.area, "new output", 0);
        output.finish(&mut buffer);
        output.event(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0));
        assert!(
            matches!(output.event(&mouse(MouseEventKind::Up(MouseButton::Left), 2, 0)), Some(Gesture::Copy(text)) if text == "old")
        );
        output.begin();
        output.finish(&mut buffer);
        assert!(output
            .event(&mouse(MouseEventKind::Down(MouseButton::Left), 0, 0))
            .is_none());
    }
}
