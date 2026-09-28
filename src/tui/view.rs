use super::{brand, clean, theme::*, App, InputMode};
use crate::core::Kind;
use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .title(title.into())
        .title_style(text().add_modifier(Modifier::BOLD))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(LINE))
        .style(text().bg(SURFACE))
}
fn label(f: &mut Frame, value: impl Into<String>, area: Rect, bold: bool) {
    f.render_widget(
        Paragraph::new(value.into()).style(if bold {
            text().add_modifier(Modifier::BOLD)
        } else {
            text()
        }),
        area,
    );
}
fn inset(a: Rect, x: u16, y: u16) -> Rect {
    Rect::new(
        a.x + x.min(a.width),
        a.y + y.min(a.height),
        a.width.saturating_sub(x * 2),
        a.height.saturating_sub(y * 2),
    )
}
fn bytes(value: u64) -> String {
    for (unit, scale) in [("GiB", 1073741824.), ("MiB", 1048576.), ("KiB", 1024.)] {
        if value as f64 >= scale {
            return format!("{:.1} {unit}", value as f64 / scale);
        }
    }
    format!("{value} B")
}
fn phase(app: &App) -> f64 {
    if std::env::var_os("DOCKER_GOO_STATIC").is_some() {
        return 0.;
    }
    let t = app.started.elapsed().as_secs_f64();
    t / 3.
}
pub(super) fn draw(f: &mut Frame, app: &mut App, endpoint: &str) {
    let a = f.area();
    fill(f, a, BASE);
    app.nav_areas = [Rect::default(); 5];
    app.logo_area = Rect::default();
    app.table_area = Rect::default();
    app.hits.clear();
    if a.width < 50 || a.height < 15 {
        label(
            f,
            "DOCKER-GOO\nResize to at least 50 x 15\nq Quit",
            inset(a, 1, 1),
            false,
        );
        return;
    }
    if app.home {
        home(f, app, endpoint);
    } else {
        dashboard(f, app, endpoint);
    }
    overlays(f, app);
    super::dialogs::draw(f, app);
    super::theme::apply(f, app.theme_index);
    if std::env::var_os("NO_COLOR").is_some() {
        for cell in &mut f.buffer_mut().content {
            cell.set_fg(Color::White).set_bg(Color::Reset);
        }
    }
}
fn home(f: &mut Frame, app: &mut App, endpoint: &str) {
    let a = f.area();
    gradient(f, Rect::new(a.x, a.y, a.width, 1), BLUE, PURPLE);
    let x = a.x + (a.width / 20).clamp(2, 8);
    let available = a.width.saturating_sub((x - a.x) * 2);
    let width = available
        .min(126)
        .min(a.height.saturating_sub(14) * 7)
        .max(28)
        .min(available);
    let height = if a.height < 18 {
        2
    } else {
        ((width as u32 * 65 / 240).div_ceil(2) as u16).max(3)
    };
    let gap = if a.height >= 32 { 2 } else { 1 };
    let total = height + 3 + 5 * gap;
    let top = a.y + a.height.saturating_sub(total + 4) / 2 + 1;
    let logo = Rect::new(x, top, width.saturating_sub(18), height);
    app.logo_area = logo;
    brand::draw(f, logo, phase(app), BASE);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("made by ", Style::default().fg(BABY)),
            Span::styled(
                "Kafeyn",
                Style::default().fg(BABY).add_modifier(Modifier::BOLD),
            ),
        ])),
        Rect::new(a.right().saturating_sub(18), brand::baseline(logo), 16, 1),
    );
    label(
        f,
        "DOCKER-GOO  /  YOUR ENGINE. YOUR TERMINAL.",
        Rect::new(x, top + height + 1, available, 1),
        true,
    );
    let menu_y = top + height + 3;
    for (i, kind) in Kind::ALL.iter().enumerate() {
        let rect = Rect::new(x, menu_y + i as u16 * gap, available.min(43), 1);
        app.nav_areas[i] = rect;
        if app.nav_index == i {
            gradient(f, rect, BLUE, PURPLE);
        }
        let count = if app.online {
            app.snapshot.resources(*kind).len().to_string()
        } else {
            "—".into()
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" [{}]", ['c', 'i', 'v', 't', 'o'][i]),
                    Style::default().fg(BABY).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" - {:<17} {:>5}", kind.title(), count),
                    if app.nav_index == i {
                        text().add_modifier(Modifier::BOLD)
                    } else {
                        text()
                    },
                ),
            ])),
            rect,
        );
    }
    if available >= 115 && a.height >= 32 {
        let card = Rect::new(
            x + 59,
            menu_y.saturating_sub(1),
            available.saturating_sub(61).min(60),
            9,
        );
        f.render_widget(panel(" ENGINE OVERVIEW "), card);
        let running = app
            .snapshot
            .containers
            .iter()
            .filter(|r| r.state == "running")
            .count();
        f.render_widget(
            Paragraph::new(format!(
                "{}\n\n{} containers running\n{} local images · {} volumes\n\n{}",
                if app.online {
                    "Connected to Docker Engine"
                } else {
                    "Waiting for Docker Engine"
                },
                running,
                app.snapshot.images.len(),
                app.snapshot.volumes.len(),
                clean(endpoint)
            ))
            .style(text())
            .wrap(Wrap { trim: false }),
            inset(card, 2, 1),
        );
    }
    let footer = Rect::new(a.x, a.bottom() - 3, a.width, 3);
    fill(f, footer, SIDEBAR);
    label(
        f,
        format!(
            "  {} · {}",
            if app.online {
                "ENGINE CONNECTED"
            } else {
                "ENGINE OFFLINE"
            },
            clean(endpoint)
        ),
        Rect::new(footer.x, footer.y, footer.width, 1),
        false,
    );
    let about = Rect::new(a.right().saturating_sub(15), footer.y + 1, 13, 1);
    nav_about(f, about);
    app.hits.push((about, KeyCode::Char('a')));
    let value = if app.input == Some(InputMode::Command) {
        format!("  $ {}", app.command)
    } else {
        "[↑↓] - Choose  [Enter] - Open  [n] - New  [$] - Commands  [Shift+T] - Themes  [?] - Help  [q] - Quit".into()
    };
    label(
        f,
        value,
        Rect::new(footer.x, footer.y + 2, footer.width, 1),
        false,
    );
}
fn nav_about(f: &mut Frame, area: Rect) {
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "[a]",
                Style::default().fg(BABY).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" - About", text()),
        ])),
        area,
    );
}
fn drawer(f: &mut Frame, app: &mut App, a: Rect) {
    fill(f, a, SIDEBAR);
    let logo = Rect::new(a.x + 2, a.y + 1, a.width.saturating_sub(4), 5);
    brand::draw(f, logo, phase(app), SIDEBAR);
    app.logo_area = Rect::new(a.x, a.y, a.width, 7);
    let start = a.y + if a.height >= 24 { 10 } else { 8 };
    let step = if a.height >= 24 { 2 } else { 1 };
    if a.height >= 24 {
        label(
            f,
            "WORKSPACE",
            Rect::new(a.x + 2, start - 2, a.width - 3, 1),
            true,
        );
    }
    for (i, kind) in Kind::ALL.iter().enumerate() {
        let rect = Rect::new(
            a.x + 1,
            start + i as u16 * step,
            a.width.saturating_sub(2),
            1,
        );
        app.nav_areas[i] = rect;
        let active = if app.drawer_focus {
            app.nav_index == i
        } else {
            app.tab == i
        };
        if active {
            gradient(f, rect, BLUE, PURPLE);
        }
        let shortcut = ['c', 'i', 'v', 't', 'o'][i];
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" [{shortcut}]"),
                    Style::default().fg(BABY).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" - {}", kind.title()),
                    if active {
                        text().add_modifier(Modifier::BOLD)
                    } else {
                        text()
                    },
                ),
            ])),
            rect,
        );
    }
    if a.height >= 24 {
        label(
            f,
            if app.online {
                "● Engine connected"
            } else {
                "○ Engine offline"
            },
            Rect::new(a.x + 2, a.bottom() - 6, a.width - 3, 1),
            false,
        );
        label(
            f,
            "Local workspace",
            Rect::new(a.x + 2, a.bottom() - 5, a.width - 3, 1),
            false,
        );
    }
    nav_about(f, Rect::new(a.x + 2, a.bottom() - 1, a.width - 3, 1));
    app.hits.push((
        Rect::new(a.x + 2, a.bottom() - 1, a.width - 3, 1),
        KeyCode::Char('a'),
    ));
    label(
        f,
        "[h] - Home",
        Rect::new(a.x + 2, a.bottom() - 4, a.width - 3, 1),
        false,
    );
    label(
        f,
        if a.width >= 19 {
            "[Shift+T] - Themes"
        } else {
            "[Shift+T]"
        },
        Rect::new(a.x + 1, a.bottom() - 2, a.width - 1, 1),
        false,
    );
    label(
        f,
        "[?] - Help",
        Rect::new(a.x + 2, a.bottom() - 3, a.width - 3, 1),
        false,
    );
    for (offset, key) in [(4, 'h'), (3, '?'), (2, 'T')] {
        app.hits.push((
            Rect::new(a.x + 1, a.bottom() - offset, a.width.saturating_sub(2), 1),
            KeyCode::Char(key),
        ));
    }
}
fn dashboard(f: &mut Frame, app: &mut App, endpoint: &str) {
    let a = f.area();
    let sidebar = if a.width >= 115 {
        34
    } else if a.width >= 80 {
        22
    } else {
        15
    };
    let columns = Layout::horizontal([Constraint::Length(sidebar), Constraint::Min(20)]).split(a);
    drawer(f, app, columns[0]);
    let content = inset(columns[1], 1, 0);
    let cards = if a.height >= 30 && content.width >= 62 {
        3
    } else {
        0
    };
    let r = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(cards),
        Constraint::Length(3),
        Constraint::Min(4),
        Constraint::Length(3),
    ])
    .split(content);
    label(
        f,
        format!(
            "{}   /{}",
            app.kind().title(),
            app.kind().title().to_lowercase()
        ),
        Rect::new(r[0].x + 1, r[0].y + 1, r[0].width.saturating_sub(20), 1),
        true,
    );
    let badge = Rect::new(r[0].right().saturating_sub(18), r[0].y + 1, 17, 1);
    gradient(f, badge, BLUE, if app.online { BLUE } else { PURPLE });
    label(
        f,
        if app.online {
            "  ENGINE ONLINE"
        } else {
            "  ENGINE OFFLINE"
        },
        badge,
        true,
    );
    if cards > 0 {
        summary(f, app, r[1]);
    }
    let filter = if app.filter.is_empty() {
        "Search by name, ID, image or state".into()
    } else {
        clean(&app.filter)
    };
    let query = if app.input == Some(InputMode::Search) {
        format!(" / {}", app.filter)
    } else {
        format!(" /  {filter}")
    };
    f.render_widget(
        Paragraph::new(query).style(text()).block(panel(" Search ")),
        r[2],
    );
    app.hits.push((r[2], KeyCode::Char('/')));
    let has_detail = app.kind() == Kind::Containers && r[3].height >= 10;
    let (table_area, detail_area) = if has_detail {
        let parts =
            Layout::vertical([Constraint::Percentage(43), Constraint::Percentage(57)]).split(r[3]);
        (parts[0], parts[1])
    } else {
        (r[3], Rect::default())
    };
    resource_table(f, app, table_area);
    if has_detail {
        detail(f, app, detail_area);
    }
    footer(f, app, endpoint, r[4]);
}
fn summary(f: &mut Frame, app: &App, a: Rect) {
    let running = app
        .snapshot
        .containers
        .iter()
        .filter(|r| {
            r.state == "running"
                && app
                    .project
                    .as_ref()
                    .is_none_or(|p| app.snapshot.compose_members.get(&r.id) == Some(p))
        })
        .count();
    let total = if app.project.is_some() {
        app.snapshot
            .containers
            .iter()
            .filter(|r| app.snapshot.compose_members.get(&r.id) == app.project.as_ref())
            .count()
    } else {
        app.snapshot.resources(app.kind()).len()
    };
    let values = if app.kind() == Kind::Containers {
        [
            ("CONTAINERS", total.to_string()),
            ("RUNNING", running.to_string()),
            (
                "SELECTED CPU",
                app.stats
                    .as_ref()
                    .map(|s| format!("{:.1}%", s.cpu))
                    .unwrap_or("—".into()),
            ),
            (
                "SELECTED RAM",
                app.stats
                    .as_ref()
                    .map(|s| bytes(s.memory))
                    .unwrap_or("—".into()),
            ),
        ]
    } else {
        [
            ("RESOURCES", total.to_string()),
            ("VISIBLE", app.rows().len().to_string()),
            (
                "ENGINE",
                if app.online { "Connected" } else { "Offline" }.into(),
            ),
            ("SCOPE", "Local engine".into()),
        ]
    };
    let cells = Layout::horizontal([Constraint::Ratio(1, 4); 4]).split(a);
    for (i, (name, value)) in values.into_iter().enumerate() {
        let rect = inset(cells[i], 1, 0);
        fill(f, rect, RAISED);
        label(
            f,
            name,
            Rect::new(rect.x + 1, rect.y, rect.width.saturating_sub(1), 1),
            false,
        );
        label(
            f,
            value,
            Rect::new(rect.x + 1, rect.y + 1, rect.width.saturating_sub(1), 1),
            true,
        );
    }
}
fn resource_table(f: &mut Frame, app: &mut App, a: Rect) {
    app.table_area = a;
    let rows = app.rows();
    let count = rows.len();
    let wide = a.width >= 78;
    let headers = match app.kind() {
        Kind::Containers => ["NAME", "STATUS", "IMAGE", "PORTS"],
        Kind::Images => ["IMAGE / TAG", "TYPE", "SIZE", "ID"],
        Kind::Volumes => ["VOLUME", "TYPE", "DRIVER", "MOUNTPOINT"],
        Kind::Networks => ["NETWORK", "SCOPE", "DRIVER", "SUBNET"],
        Kind::Compose => ["PROJECT", "STATUS", "CONTAINERS", ""],
    };
    let items = rows.into_iter().enumerate().map(|(index, r)| {
        let name = if r.name.is_empty() {
            r.id.chars().take(20).collect()
        } else {
            clean(&r.name)
        };
        let extra = if app.kind() == Kind::Images {
            r.id.trim_start_matches("sha256:")
                .chars()
                .take(12)
                .collect()
        } else {
            clean(&r.extra)
        };
        let mut cells = vec![
            Cell::from(name),
            Cell::from(clean(&r.state)).style(Style::default().fg(match r.state.as_str() {
                "running" => Color::Rgb(78, 230, 160),
                "paused" | "restarting" => Color::Rgb(255, 204, 102),
                "exited" | "dead" => Color::Rgb(255, 124, 151),
                _ => Color::Rgb(112, 190, 255),
            })),
            Cell::from(clean(&r.detail)),
        ];
        if wide {
            cells.push(Cell::from(extra));
        }
        Row::new(cells).style(text().bg(if index % 2 == 0 { SURFACE } else { RAISED }))
    });
    let widths = if wide {
        vec![
            Constraint::Percentage(30),
            Constraint::Length(12),
            Constraint::Percentage(28),
            Constraint::Min(10),
        ]
    } else {
        vec![
            Constraint::Percentage(43),
            Constraint::Length(10),
            Constraint::Min(6),
        ]
    };
    let table = Table::new(items, widths)
        .header(
            Row::new(headers.into_iter().take(if wide { 4 } else { 3 }))
                .style(text().bg(RAISED).add_modifier(Modifier::BOLD)),
        )
        .block(panel(format!(
            " {} · {count}{} ",
            app.project
                .as_ref()
                .map(|p| format!("Compose / {p}"))
                .unwrap_or_else(|| app.kind().title().into()),
            if app.online { "" } else { " · cached" }
        )))
        .row_highlight_style(Style::default().add_modifier(Modifier::BOLD))
        .highlight_symbol("› ");
    f.render_stateful_widget(table, a, &mut app.table);
    if let Some(index) = app.table.selected() {
        if index < count && index >= app.table.offset() {
            let y = a.y + 2 + (index - app.table.offset()) as u16;
            if y < a.bottom().saturating_sub(1) {
                gradient(
                    f,
                    Rect::new(a.x + 1, y, a.width.saturating_sub(2), 1),
                    Color::Rgb(22, 66, 145),
                    Color::Rgb(72, 40, 120),
                );
            }
        }
    }
    if count == 0 && a.height > 3 {
        let msg = if app.online {
            "No matching resources. Esc clears the search."
        } else {
            "Docker is offline. Check the daemon or selected context. Retrying…"
        };
        f.render_widget(
            Paragraph::new(msg).style(text()).wrap(Wrap { trim: true }),
            Rect::new(
                a.x + 2,
                a.y + 3,
                a.width.saturating_sub(4),
                a.height.saturating_sub(4),
            ),
        );
    }
}
fn button(f: &mut Frame, app: &mut App, a: Rect, title: &str, key: KeyCode, active: bool) {
    let a = Rect::new(
        a.x,
        a.y,
        a.width.min(title.chars().count() as u16 + 6),
        a.height,
    );
    let (shortcut, caption) = title.split_once(' ').unwrap_or((title, ""));
    let character = if let KeyCode::Char(c) = key { c } else { '?' };
    let color = action_color(character);
    fill(f, a, color);
    let fg = if matches!(character, 'w' | 'r' | 'p') {
        Color::Rgb(12, 18, 28)
    } else {
        WHITE
    };
    let style = Style::default().fg(fg).bg(color);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" [{shortcut}] - "),
                style.add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                caption.to_owned(),
                if active {
                    style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
                } else {
                    style
                },
            ),
        ])),
        a,
    );
    app.hits.push((a, key));
}
fn detail(f: &mut Frame, app: &mut App, a: Rect) {
    let title = app
        .selected()
        .map(|r| clean(&r.name))
        .unwrap_or("Select a container".into());
    f.render_widget(panel(format!(" {title} ")), a);
    let inner = inset(a, 1, 1);
    if inner.height < 3 {
        return;
    }
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);
    let tabs = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Length(14),
        Constraint::Length(16),
        Constraint::Min(14),
    ])
    .split(parts[0]);
    for (i, (title, key)) in [
        ("l Logs", 'l'),
        ("g Stats", 'g'),
        ("e Inspect", 'e'),
        ("b Shell", 'b'),
    ]
    .into_iter()
    .enumerate()
    {
        button(
            f,
            app,
            tabs[i],
            title,
            KeyCode::Char(key),
            i == app.detail_tab,
        );
    }
    let content = parts[2];
    if app.detail_tab == 1 {
        let metrics=app.stats.as_ref().map(|s|format!("CPU usage   {:.1}%\nMemory      {} / {}\nNetwork RX  {}\nNetwork TX  {}\n\nNetwork totals since container start",s.cpu,bytes(s.memory),bytes(s.limit),bytes(s.rx),bytes(s.tx))).unwrap_or_else(||"Live metrics appear when the selected container is running.".into());
        f.render_widget(
            Paragraph::new(metrics)
                .style(text())
                .wrap(Wrap { trim: true }),
            inset(content, 1, 0),
        );
    } else {
        let end = app.logs.len().saturating_sub(app.log_scroll);
        let start = end.saturating_sub(content.height as usize);
        let lines = if app.logs.is_empty() {
            vec![Line::raw(" Waiting for output from this container…")]
        } else {
            app.logs
                .iter()
                .skip(start)
                .take(end - start)
                .map(|s| Line::raw(format!(" {}", clean(s))))
                .collect()
        };
        f.render_widget(Paragraph::new(lines).style(text()), content);
    }
}
fn footer(f: &mut Frame, app: &mut App, endpoint: &str, a: Rect) {
    fill(f, a, BASE);
    let value = if let Some(mode) = app.input {
        match mode {
            InputMode::Command => format!("$ {}", app.command),
            InputMode::Search => format!("/{}", app.filter),
        }
    } else {
        clean(&app.status)
    };
    label(f, value, Rect::new(a.x, a.y, a.width, 1), false);
    if app.kind() == Kind::Containers && a.width >= 64 {
        let mut x = a.x;
        for (title, key, w) in [
            ("n New", 'n', 12),
            ("w Web", 'w', 12),
            ("s Start", 's', 13),
            ("x Stop", 'x', 13),
            ("r Restart", 'r', 16),
            ("p Pause", 'p', 14),
            ("u Resume", 'u', 15),
            ("d Delete", 'd', 15),
        ] {
            if x + w > a.right() {
                break;
            }
            button(
                f,
                app,
                Rect::new(x, a.y + 1, w - 1, 1),
                title,
                KeyCode::Char(key),
                false,
            );
            x += w;
        }
    } else if app.kind() == Kind::Compose {
        button(
            f,
            app,
            Rect::new(a.x, a.y + 1, a.width.min(26), 1),
            "y Run existing YAML",
            KeyCode::Char('y'),
            false,
        );
        label(
            f,
            "[Enter] - Monitor project",
            Rect::new(a.x + 27, a.y + 1, a.width.saturating_sub(27), 1),
            false,
        );
    } else {
        label(
            f,
            "[n] - New  [e] - Inspect  [/] - Search  [b] - Shell  [h] - Home",
            Rect::new(a.x, a.y + 1, a.width, 1),
            false,
        );
    }
    label(
        f,
        format!(
            "[f] - Refresh  [$] - Commands  [n] - New  [?] - Help  ·  {}",
            clean(endpoint)
        ),
        Rect::new(a.x, a.y + 2, a.width, 1),
        false,
    );
    for (offset, width, key) in [(0, 13, 'f'), (15, 14, '$'), (31, 9, 'n'), (42, 10, '?')] {
        if offset + width <= a.width {
            let rect = Rect::new(a.x + offset, a.y + 2, width, 1);
            app.hits.push((rect, KeyCode::Char(key)));
            if let Some(cell) = f.buffer_mut().cell_mut((rect.x + 1, rect.y)) {
                cell.set_fg(action_color(key));
            }
        }
    }
}
fn overlays(f: &mut Frame, app: &App) {
    let a = f.area();
    if let Some(r) = &app.confirm {
        popup(f," Delete container ",&format!("Delete {}?\n\nContainer writable-layer data will be lost.\nVolumes are kept. No forced deletion.\n\ny Delete   n / Esc Cancel",clean(&r.name)),66,10,0);
    }
    if let Some(v) = &app.inspect {
        popup(
            f,
            " Inspect · ↑↓ / PgUp / PgDn · Esc Close ",
            v,
            a.width.saturating_sub(4),
            a.height.saturating_sub(4),
            app.inspect_scroll,
        );
    }
    if app.help {
        popup(f," Docker-Goo / Keyboard shortcuts ","c Containers · i Images · v Volumes · t Networks · o Compose\n↑↓ / j k Select · Enter Inspect · Tab Drawer\nh Home · a About · Shift+T Themes · / Search · f Refresh\nn New container (also available from Images)\ns Start · x Stop · r Restart · p Pause · u Resume\nd Delete (stopped containers, with confirmation)\nl Logs · g Stats · e Inspect\nw Open published web port · b Container shell\n$ Docker command console with contextual help\nCtrl+D Detach container terminal\nPgUp / PgDn Scroll output\n? Help · q / Ctrl+C Quit\n\nCreate: Tab fields, Ctrl+P Service/Shell, Enter Create.\nConsole: Enter Run, Ctrl+C Cancel, Esc Close.\nMouse: click sections, tabs and action buttons.\nCompose: y Run YAML · Enter Monitor project.",84,23,0);
    }
}
fn popup(f: &mut Frame, title: &str, value: &str, width: u16, height: u16, scroll: u16) {
    let a = f.area();
    let width = width.min(a.width);
    let height = height.min(a.height);
    let rect = Rect::new(
        a.x + (a.width - width) / 2,
        a.y + (a.height - height) / 2,
        width,
        height,
    );
    f.render_widget(Clear, rect);
    fill(f, rect, SURFACE);
    let lines = value
        .lines()
        .map(|l| Line::raw(clean(l)))
        .collect::<Vec<_>>();
    f.render_widget(
        Paragraph::new(lines)
            .style(text())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0))
            .block(panel(title.to_string())),
        rect,
    );
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn drawer_home_and_help_have_mouse_targets() {
        let mut app = App::new();
        app.change_tab(0);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|f| draw(f, &mut app, "unix:///missing.sock"))
            .unwrap();
        for key in ['h', '?', 'T'] {
            assert!(app.hits.iter().any(|(_, k)| *k == KeyCode::Char(key)));
        }
        app.input = Some(InputMode::Command);
        assert_eq!(super::super::console::suggestions(&app).0, vec!["docker "]);
    }
    #[test]
    fn renders_small_standard_and_wide_terminals() {
        for (w, h) in [(20, 5), (50, 15), (80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            let mut app = App::new();
            for stage in 0..8 {
                if stage == 1 {
                    app.change_tab(0);
                }
                if stage == 2 {
                    app.change_tab(1);
                }
                if stage == 3 {
                    app.help = true;
                }
                if stage == 4 {
                    app.help = false;
                    app.dialogs.about = true;
                }
                if stage == 5 {
                    app.dialogs.about = false;
                    app.dialogs.form = Some(super::super::dialogs::Form::new("ubuntu".into()));
                }
                if stage == 6 {
                    app.dialogs.form = None;
                    app.input = Some(InputMode::Command);
                }
                if stage == 7 {
                    app.input = None;
                    app.dialogs.urls = Some(vec!["http://127.0.0.1:8080".into()]);
                }
                terminal
                    .draw(|f| draw(f, &mut app, "unix:///var/run/docker.sock"))
                    .unwrap();
            }
        }
    }
}

#[cfg(test)]
mod visual_preview {
    use super::*;
    use crate::core::{Metrics, Resource};
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    #[ignore = "Explicit renderer preview with sample data"]
    fn export_preview() {
        let out = std::env::var("DOCKER_GOO_PREVIEW_DIR").expect("Set preview output directory");
        std::fs::create_dir_all(&out).unwrap();
        let mut app = App::new();
        app.online = true;
        app.daemon = "Design preview engine".into();
        app.status = "DESIGN PREVIEW · SAMPLE DATA".into();
        app.snapshot.containers = [
            ("api-server", "running", "node:22-alpine", "3000 → 3000"),
            ("postgres", "running", "postgres:17", "5432 → 5432"),
            ("redis", "running", "redis:7-alpine", "6379 → 6379"),
            ("worker", "exited", "docker-goo/worker:dev", ""),
            ("hello-world", "exited", "hello-world:latest", ""),
        ]
        .into_iter()
        .map(|(name, state, detail, extra)| Resource {
            id: name.into(),
            name: name.into(),
            state: state.into(),
            detail: detail.into(),
            extra: extra.into(),
        })
        .collect();
        app.snapshot.images = [
            ("node:22-alpine", "158 MiB"),
            ("postgres:17", "438 MiB"),
            ("redis:7-alpine", "41 MiB"),
            ("hello-world:latest", "10 KiB"),
        ]
        .into_iter()
        .map(|(name, size)| Resource {
            id: "sha256:9fcdca130e55".into(),
            name: name.into(),
            state: "image".into(),
            detail: size.into(),
            extra: String::new(),
        })
        .collect();
        app.stats = Some(Metrics {
            cpu: 2.4,
            memory: 81788928,
            limit: 536870912,
            rx: 1100000,
            tx: 620000,
        });
        app.logs = [
            "2026-09-27 22:41:01  INFO  Starting API server",
            "2026-09-27 22:41:01  INFO  Connected to PostgreSQL",
            "2026-09-27 22:41:02  INFO  Listening on 0.0.0.0:3000",
            "2026-09-27 22:41:06  GET   /health                 200   1ms",
            "2026-09-27 22:41:10  GET   /api/projects           200   8ms",
            "2026-09-27 22:41:12  POST  /api/tasks              201   5ms",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        for (name, tab) in [
            ("home", None),
            ("containers", Some(0)),
            ("images", Some(1)),
            ("about", Some(0)),
            ("create", Some(0)),
            ("picker", Some(0)),
            ("compose", Some(4)),
            ("black", Some(0)),
            ("white", Some(0)),
            ("themes", Some(0)),
            ("console", Some(0)),
            ("console-white", Some(0)),
            ("console-black", Some(0)),
        ] {
            if let Some(tab) = tab {
                app.change_tab(tab);
            }
            app.theme_index = match name {
                "black" | "console-black" => 1,
                "white" | "console-white" => 2,
                _ => 0,
            };
            app.input = None;
            app.dialogs.themes = (name == "themes").then_some(0);
            app.dialogs.picker = name == "picker";
            app.dialogs.compose_path =
                (name == "compose").then(|| "~/Projects/example/compose.yaml".into());
            app.dialogs.about = name == "about";
            app.dialogs.form =
                (name == "create").then(|| super::super::dialogs::Form::new("nginx:alpine".into()));
            if name.starts_with("console") {
                app.input = Some(InputMode::Command);
                app.command = if name == "console" {
                    "docker p"
                } else {
                    "docker stop "
                }
                .into();
                app.dialogs.command_cursor = app.command.len();
                app.dialogs.cwd = Some(std::path::PathBuf::from(
                    "/home/anass05/Projects/docker-goo",
                ));
                app.dialogs.guide = crate::core::commands::GUIDE.into();
            }
            let mut terminal = Terminal::new(TestBackend::new(140, 42)).unwrap();
            terminal
                .draw(|f| draw(f, &mut app, "unix:///var/run/docker.sock"))
                .unwrap();
            let color = |c: Color| match c {
                Color::Rgb(r, g, b) => vec![r, g, b],
                Color::White => vec![255, 255, 255],
                Color::Black => vec![0, 0, 0],
                _ => vec![7, 18, 42],
            };
            let cells=terminal.backend().buffer().content.iter().map(|c|serde_json::json!({"s":c.symbol(),"fg":color(c.fg),"bg":color(c.bg),"bold":c.modifier.contains(Modifier::BOLD)})).collect::<Vec<_>>();
            std::fs::write(
                format!("{out}/{name}.json"),
                serde_json::to_vec(&serde_json::json!({"width":140,"height":42,"cells":cells}))
                    .unwrap(),
            )
            .unwrap();
        }
    }
}
