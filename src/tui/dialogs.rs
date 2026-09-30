use super::*;
use crate::core::{commands, RunSpec};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

pub(super) struct Form {
    pub fields: Vec<String>,
    pub focus: usize,
    pub shell: bool,
    pub cursors: [usize; 7],
}
impl Form {
    fn toggle_interactive(&mut self) {
        self.shell = !self.shell;
    }

    pub fn new(image: String) -> Self {
        Self {
            cursors: [image.len(), 0, 0, 0, 0, 0, 0],
            fields: vec![
                image,
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                String::new(),
            ],
            focus: 0,
            shell: false,
        }
    }
}
#[derive(Default)]
pub(super) struct Dialogs {
    pub about: bool,
    pub cwd: Option<std::path::PathBuf>,
    pub command_cursor: usize,
    pub command_id: u64,
    pub history: Vec<String>,
    pub history_index: Option<usize>,
    pub follow_output: bool,
    pub form_error: String,
    pub create_area: Option<Rect>,
    pub interactive_area: Option<Rect>,
    pub native_area: Option<Rect>,
    pub form_areas: Vec<Rect>,
    pub suggestion_rows: Vec<(Rect, usize)>,
    pub themes: Option<usize>,
    pub suggestion_index: usize,
    pub suggestions_hidden: bool,
    pub console_controls: Option<Rect>,
    pub picker: bool,
    pub image_index: usize,
    pub image_query: String,
    pub compose_path: Option<String>,
    pub compose_running: bool,
    pub compose_error: String,
    pub form: Option<Form>,
    pub shell: Option<(Resource, String)>,
    pub urls: Option<Vec<String>>,
    pub url_index: usize,
    pub output: String,
    pub guide: String,
    pub topic: String,
    pub output_scroll: u16,
    pub help_scroll: u16,
    pub command_job: Option<JoinHandle<()>>,
}
impl Drop for Dialogs {
    fn drop(&mut self) {
        if let Some(job) = self.command_job.take() {
            job.abort();
        }
    }
}
pub(super) async fn open_url(url: String) -> Result<()> {
    anyhow::ensure!(
        url.starts_with("http://") || url.starts_with("https://"),
        "Only HTTP/HTTPS links can be opened"
    );
    let status = tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new("xdg-open")
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .status(),
    )
    .await??;
    anyhow::ensure!(
        status.success(),
        "Browser launch failed; check your default browser / xdg-open"
    );
    Ok(())
}
pub(super) fn active(app: &App) -> bool {
    app.dialogs.themes.is_some()
        || app.dialogs.picker
        || app.dialogs.compose_path.is_some()
        || app.dialogs.about
        || app.dialogs.form.is_some()
        || app.dialogs.shell.is_some()
        || app.dialogs.urls.is_some()
}
pub(super) fn key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    engine: Arc<dyn Engine>,
    tx: &mpsc::Sender<Message>,
    jobs: &mut Jobs,
    refresh: &Notify,
    default_shell: &str,
) -> bool {
    let code = key.code;
    if let Some(previous) = app.dialogs.themes {
        match code {
            KeyCode::Esc => {
                app.theme_index = previous;
                app.dialogs.themes = None;
            }
            KeyCode::Down | KeyCode::Right => app.theme_index = (app.theme_index + 1) % 3,
            KeyCode::Up | KeyCode::Left => app.theme_index = (app.theme_index + 2) % 3,
            KeyCode::Char(c @ '1'..='3') => app.theme_index = c as usize - '1' as usize,
            KeyCode::Enter => {
                app.status = match theme::save(app.theme_index) {
                    Ok(()) => format!("Theme: {}", theme::NAMES[app.theme_index]),
                    Err(e) => format!("Theme applied but could not save: {e}"),
                };
                app.dialogs.themes = None;
            }
            _ => {}
        }
        return true;
    }
    if app.dialogs.picker {
        let images = image_choices(app);
        match code {
            KeyCode::Esc => {
                app.dialogs.picker = false;
            }
            KeyCode::Down => {
                app.dialogs.image_index =
                    (app.dialogs.image_index + 1).min(images.len().saturating_sub(1))
            }
            KeyCode::Up => app.dialogs.image_index = app.dialogs.image_index.saturating_sub(1),
            KeyCode::Enter => {
                if let Some(image) = images.get(app.dialogs.image_index) {
                    choose_image(app, image.clone());
                }
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                choose_image(app, String::new())
            }
            KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                refresh.notify_one()
            }
            KeyCode::Backspace => {
                app.dialogs.image_query.pop();
                app.dialogs.image_index = 0;
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.dialogs.image_query.push(c);
                app.dialogs.image_index = 0;
            }
            _ => {}
        }
        return true;
    }
    if let Some(path) = app.dialogs.compose_path.as_mut() {
        if app.dialogs.compose_running {
            return true;
        }
        match code {
            KeyCode::Esc => app.dialogs.compose_path = None,
            KeyCode::Backspace => {
                path.pop();
            }
            KeyCode::Char(c) => path.push(c),
            KeyCode::Enter if !app.busy => {
                let path = path.clone();
                app.dialogs.compose_error.clear();
                app.dialogs.compose_running = true;
                app.busy = true;
                let tx = tx.clone();
                jobs.0.push(tokio::spawn(async move {
                    let _ = tx
                        .send(Message::Compose(engine.compose_up(&path).await))
                        .await;
                }));
            }
            _ => {}
        }
        return true;
    }
    if app.dialogs.about {
        if matches!(code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a')) {
            app.dialogs.about = false;
        }
        if code == KeyCode::Char('w') {
            let tx = tx.clone();
            jobs.0.push(tokio::spawn(async move {
                let _ = tx
                    .send(Message::Action(
                        open_url("https://github.com/kurapikanlight".into()).await,
                        "Open GitHub".into(),
                    ))
                    .await;
            }));
        }
        return true;
    }
    if let Some(form) = app.dialogs.form.as_mut() {
        if app.busy {
            return true;
        }
        if form.focus < 7
            && super::console::edit(
                &mut form.fields[form.focus],
                &mut form.cursors[form.focus],
                code,
            )
        {
            return true;
        }
        match code {
            KeyCode::Esc => app.dialogs.form = None,
            KeyCode::Tab | KeyCode::Down => form.focus = (form.focus + 1) % 9,
            KeyCode::BackTab | KeyCode::Up => form.focus = (form.focus + 8) % 9,
            KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.toggle_interactive();
            }
            KeyCode::Enter | KeyCode::Char(' ') if form.focus == 7 => {
                form.toggle_interactive();
            }
            KeyCode::Enter if form.focus == 8 => {
                if app.busy || !app.online {
                    app.dialogs.form_error = "An online engine is required".into();
                    return true;
                }
                let f = &form.fields;
                let spec = RunSpec {
                    image: f[0].clone(),
                    name: f[1].clone(),
                    command: f[2].clone(),
                    ports: f[3].clone(),
                    env: f[4].clone(),
                    mounts: f[5].clone(),
                    network: f[6].clone(),
                    interactive: form.shell,
                };
                app.busy = true;
                app.status = "Pulling image if needed, then creating and starting…".into();
                app.dialogs.form_error.clear();
                let tx = tx.clone();
                jobs.0.push(tokio::spawn(async move {
                    let result = engine.create(spec).await;
                    let _ = tx.send(Message::Created(result)).await;
                }));
            }
            KeyCode::Enter if form.focus == 0 => {
                app.dialogs.picker = true;
                app.dialogs.image_query.clear();
                app.dialogs.image_index = 0;
            }
            KeyCode::Enter => form.focus = (form.focus + 1) % 9,
            KeyCode::Backspace if form.focus < 7 => {
                form.fields[form.focus].pop();
            }
            KeyCode::Char(c)
                if form.focus < 7
                    && !key.modifiers.contains(KeyModifiers::CONTROL)
                    && form.fields[form.focus].len() < 4096 =>
            {
                super::console::insert(
                    &mut form.fields[form.focus],
                    &mut form.cursors[form.focus],
                    &c.to_string(),
                );
            }
            _ => {}
        }
        return true;
    }
    if let Some((resource, command)) = app.dialogs.shell.as_mut() {
        if code == KeyCode::Enter && key.modifiers.contains(KeyModifiers::CONTROL) {
            let mut process = engine.docker_cli();
            if resource.state == "running" {
                process.args(["exec", "-it", &resource.id]);
                if command == "auto" {
                    process.args([
                        "/bin/sh",
                        "-c",
                        "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi",
                    ]);
                } else {
                    match shell_words::split(command) {
                        Ok(args) => {
                            process.args(args);
                        }
                        Err(e) => {
                            app.status = e.to_string();
                            return true;
                        }
                    }
                }
            } else {
                process.args(["start", "-ai", &resource.id]);
            }
            let _ = tx.try_send(Message::Terminal(process));
            app.dialogs.shell = None;
            return true;
        }
        match code {
            KeyCode::Esc => app.dialogs.shell = None,
            KeyCode::Tab => {
                *command = match command.as_str() {
                    "auto" => "/bin/bash",
                    "/bin/bash" => "/bin/sh",
                    _ => "auto",
                }
                .into()
            }
            KeyCode::Backspace => {
                command.pop();
            }
            KeyCode::Char(c) => command.push(c),
            KeyCode::Enter => {
                let resource = resource.clone();
                let command = command.clone();
                app.dialogs.shell = None;
                app.busy = true;
                app.status = "Opening container terminal…".into();
                let tx = tx.clone();
                let (cols, rows) = crossterm::terminal::size().unwrap_or((80, 24));
                jobs.0.push(tokio::spawn(async move {
                    let _ = tx
                        .send(Message::Shell(
                            engine.shell(&resource.id, &command, cols, rows).await,
                        ))
                        .await;
                }));
            }
            _ => {}
        }
        return true;
    }
    if let Some(urls) = app.dialogs.urls.as_mut() {
        match code {
            KeyCode::Esc => app.dialogs.urls = None,
            KeyCode::Down => app.dialogs.url_index = (app.dialogs.url_index + 1) % urls.len(),
            KeyCode::Up => app.dialogs.url_index = app.dialogs.url_index.saturating_sub(1),
            KeyCode::Char('s') => {
                let url = &mut urls[app.dialogs.url_index];
                *url = if url.starts_with("https:") {
                    url.replacen("https:", "http:", 1)
                } else {
                    url.replacen("http:", "https:", 1)
                };
            }
            KeyCode::Enter => {
                let url = urls[app.dialogs.url_index].clone();
                app.dialogs.urls = None;
                let tx = tx.clone();
                jobs.0.push(tokio::spawn(async move {
                    let _ = tx
                        .send(Message::Action(open_url(url).await, "Open browser".into()))
                        .await;
                }));
            }
            _ => {}
        }
        return true;
    }
    if app.input == Some(InputMode::Command) {
        let handled = super::console::key(app, key, engine.clone(), tx);
        let topic = if app.command.trim_start().starts_with("docker") {
            commands::topic(&app.command)
        } else {
            String::new()
        };
        if !app.command.is_empty() && topic != app.dialogs.topic {
            app.dialogs.topic = topic.clone();
            app.dialogs.help_scroll = 0;
            if topic.is_empty() {
                app.dialogs.guide="Host commands run in the displayed local directory.\ncd changes this console's directory.\nCtrl+O opens your native host shell.\nUse docker compose -f for YAML files.\nUse docker build -f for Dockerfiles.\nUse docker exec -it NAME sh for an interactive container terminal.".into();
            } else {
                let tx = tx.clone();
                jobs.0.push(tokio::spawn(async move {
                    let result = engine.docker_command(&topic).await;
                    let _ = tx.send(Message::Guide(topic, result)).await;
                }));
            }
        }
        return handled;
    }
    if app.input.is_some() || app.confirm.is_some() || app.inspect.is_some() || app.help {
        return false;
    }
    match code {
        KeyCode::Char('c') if !key.modifiers.contains(KeyModifiers::CONTROL) => app.change_tab(0),
        KeyCode::Char('i') => app.change_tab(1),
        KeyCode::Char('v') => app.change_tab(2),
        KeyCode::Char('t') => app.change_tab(3),
        KeyCode::Char('o') => app.change_tab(4),
        KeyCode::Char('a') => app.dialogs.about = true,
        KeyCode::Char('T') => app.dialogs.themes = Some(app.theme_index),
        KeyCode::Char('f') => {
            refresh.notify_one();
            app.stream_key = None;
            app.status = "Refreshing resources…".into();
        }
        KeyCode::Char('$') => {
            app.input = Some(InputMode::Command);
            app.dialogs.command_cursor = app.command.len();
            
            app.dialogs.guide = commands::GUIDE.into();
            app.dialogs.topic.clear();
        }
        KeyCode::Char('y') if app.kind() == Kind::Compose && !app.home => {
            app.dialogs.compose_path = Some(String::new());
            app.dialogs.compose_error.clear();
        }
        KeyCode::Char('n') if app.kind() == Kind::Compose && !app.home => {
            app.dialogs.compose_path = Some(String::new());
            app.dialogs.compose_error.clear();
        }
        KeyCode::Char('n') => {
            app.dialogs.form_error.clear();
            app.dialogs.picker = true;
            app.dialogs.image_query.clear();
            app.dialogs.image_index = 0;
        }
        KeyCode::Char('b') if app.kind() == Kind::Containers && !app.home => {
            if !app.busy {
                if let Some(r) = app.selected() {
                    app.dialogs.shell = Some((r, default_shell.into()));
                }
            }
        }
        KeyCode::Char('w') if app.kind() == Kind::Containers && !app.home => {
            if let Some(r) = app.selected() {
                let tx = tx.clone();
                jobs.0.push(tokio::spawn(async move {
                    let _ = tx.send(Message::Urls(engine.web_urls(&r.id).await)).await;
                }));
            }
        }
        _ => return false,
    }
    true
}
fn console_suggestions(app: &App) -> (Vec<String>, bool) {
    super::console::suggestions(app)
}
fn highlight(f: &mut ratatui::Frame, area: Rect) {
    theme::gradient(f, area, theme::BLUE, theme::PURPLE);
    for x in area.x..area.right() {
        if let Some(cell) = f.buffer_mut().cell_mut((x, area.y)) {
            cell.set_fg(theme::WHITE)
                .set_style(Style::default().add_modifier(ratatui::style::Modifier::BOLD));
        }
    }
}
fn input_line(
    f: &mut ratatui::Frame,
    area: Rect,
    prefix: &str,
    value: &str,
    cursor: usize,
    active: bool,
) {
    let cursor = cursor.min(value.len());
    let cursor = if value.is_char_boundary(cursor) {
        cursor
    } else {
        value.len()
    };
    let prefix_width = ratatui::text::Line::raw(prefix).width();
    let available = (area.width as usize).saturating_sub(prefix_width + 1);
    let mut first = 0;
    if active {
        while first < cursor && ratatui::text::Line::raw(&value[first..cursor]).width() > available
        {
            first += value[first..].chars().next().unwrap().len_utf8();
        }
    }
    f.render_widget(
        Paragraph::new(format!("{prefix}{}", &value[first..])).style(theme::text()),
        area,
    );
    if active && area.width > 0 {
        let x = (prefix_width + ratatui::text::Line::raw(&value[first..cursor]).width())
            .min(area.width.saturating_sub(1) as usize);
        f.set_cursor_position((area.x + x as u16, area.y));
    }
}
fn console_panel(f: &mut ratatui::Frame, area: Rect, title: &str, content: String, scroll: u16) {
    panel(f, area, title, content, scroll);
    for y in area.y..area.bottom() {
        for x in area.x..area.right() {
            if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
                cell.set_bg(theme::BASE);
            }
        }
    }
}
fn panel(f: &mut ratatui::Frame, a: Rect, title: &str, text: String, scroll: u16) {
    f.render_widget(Clear, a);
    f.render_widget(
        Paragraph::new(text)
            .style(theme::text().bg(theme::SURFACE))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_owned())
                    .border_style(Style::default().fg(theme::BLUE)),
            )
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        a,
    );
}
pub(super) fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let screen = f.area();
    let w = screen.width.saturating_sub(4).min(100);
    let h = screen.height.saturating_sub(4).min(25);
    let a = Rect::new((screen.width - w) / 2, (screen.height - h) / 2, w, h);
    if app.dialogs.about {
        panel(f, a, " About Docker-Goo · Esc close ", String::new(), 0);
        app.hits.clear();
        let logo_h = a.height.saturating_sub(12).clamp(2, 9);
        brand::draw(
            f,
            Rect::new(a.x + 3, a.y + 1, a.width.saturating_sub(6), logo_h),
            app.started.elapsed().as_secs_f64() / 3.,
            theme::SURFACE,
        );
        let y = a.y + logo_h + 2;
        use ratatui::text::{Line, Span};
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("made by ", Style::default().fg(theme::BABY)),
                Span::styled(
                    "Kafeyn",
                    Style::default()
                        .fg(theme::BABY)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ])),
            Rect::new(a.x + 3, y, a.width.saturating_sub(6), 1),
        );
        f.render_widget(Paragraph::new("A Linux-first TUI for Docker.\nThe Docker Desktop experience in your terminal.\nYour engine. No account or cloud.\n\ngithub.com/kurapikanlight").style(theme::text()).wrap(Wrap{trim:false}),Rect::new(a.x+3,y+2,a.width.saturating_sub(6),a.bottom().saturating_sub(y+4)));
        let link = Rect::new(
            a.x + 3,
            a.bottom().saturating_sub(2),
            a.width.saturating_sub(6),
            1,
        );
        f.render_widget(
            Paragraph::new("[ w ] GitHub     Esc Close").style(
                Style::default()
                    .fg(theme::BABY)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
            link,
        );
        app.hits
            .push((Rect::new(link.x, link.y, 5, 1), KeyCode::Char('w')));
    }
    app.dialogs.create_area = None;
    app.dialogs.interactive_area = None;
    app.dialogs.form_areas.clear();
    if let Some(form) = &app.dialogs.form {
        panel(f, a, " New container ", String::new(), 0);
        let labels = [
            "Image",
            "Name",
            "Command",
            "Ports",
            "Environment",
            "Mounts",
            "Network",
        ];
        let top = Rect::new(a.x + 1, a.y + 1, a.width.saturating_sub(2), 1);
        f.render_widget(
            Paragraph::new(format!(
                "{} · [Ctrl+P] - Toggle -it · [Tab] - Next",
                if form.shell { "Shell / TTY" } else { "Service" }
            ))
            .style(theme::text()),
            top,
        );
        let count = a.height.saturating_sub(9).clamp(1, 7) as usize;
        let first = form.focus.min(6).saturating_sub(count - 1);
        app.dialogs.form_areas = vec![Rect::default(); 7];
        for (i, label) in labels.iter().enumerate().skip(first).take(count) {
            let row = Rect::new(
                a.x + 1,
                a.y + 3 + (i - first) as u16,
                a.width.saturating_sub(2),
                1,
            );
            app.dialogs.form_areas[i] = row;
            let prefix = format!("{label}: ");
            input_line(
                f,
                row,
                &prefix,
                &form.fields[i],
                form.cursors[i],
                form.focus == i && !app.dialogs.picker,
            );
            if form.focus == i {
                highlight(f, row);
            }
        }
        let interactive = Rect::new(
            a.x + 1,
            a.bottom().saturating_sub(5),
            a.width.saturating_sub(2),
            1,
        );
        f.render_widget(
            Paragraph::new(format!(
                "[{}] Interactive terminal (-it) · [Space] - Toggle",
                if form.shell { "x" } else { " " }
            ))
            .style(theme::text()),
            interactive,
        );
        app.dialogs.interactive_area = Some(interactive);
        if form.focus == 7 {
            highlight(f, interactive);
        }
        let create = Rect::new(
            a.x + 1,
            a.bottom().saturating_sub(4),
            a.width.saturating_sub(2),
            1,
        );
        f.render_widget(
            Paragraph::new(if app.busy {
                "Pulling / creating / starting…"
            } else {
                "[Enter] - CREATE AND START"
            })
            .style(theme::text()),
            create,
        );
        app.dialogs.create_area = Some(create);
        if form.focus == 8 {
            highlight(f, create);
        }
        let status = if app.dialogs.form_error.is_empty() {
            "[Esc] - Cancel · Right-click / Ctrl+V paste · Errors keep fields"
        } else {
            &app.dialogs.form_error
        };
        f.render_widget(
            Paragraph::new(status)
                .style(Style::default().fg(if app.dialogs.form_error.is_empty() {
                    theme::BABY
                } else {
                    ratatui::style::Color::Rgb(255, 124, 151)
                }))
                .wrap(Wrap { trim: false }),
            Rect::new(
                a.x + 1,
                a.bottom().saturating_sub(3),
                a.width.saturating_sub(2),
                2,
            ),
        );
    }
    if app.dialogs.picker {
        let images = image_choices(app);
        let rows = a.height.saturating_sub(6).max(1) as usize;
        let start = app.dialogs.image_index.saturating_sub(rows - 1);
        let mut lines = vec![
            format!("Search: {}", app.dialogs.image_query),
            "↑↓ / click Select · Enter Use · Ctrl+R Refresh".into(),
            "Ctrl+N Type another image · Esc Back".into(),
            String::new(),
        ];
        if images.is_empty() {
            lines.push("No local images match. Ctrl+N to enter an image to pull.".into());
        }
        for (i, image) in images.iter().enumerate().skip(start).take(rows) {
            lines.push(format!(
                "{} {}",
                if i == app.dialogs.image_index {
                    "›"
                } else {
                    " "
                },
                clean(image)
            ));
        }
        panel(
            f,
            a,
            " Containers / New / Choose image ",
            lines.join("\n"),
            0,
        );
        if !images.is_empty() {
            highlight(
                f,
                Rect::new(
                    a.x + 1,
                    a.y + 5 + app.dialogs.image_index.saturating_sub(start) as u16,
                    a.width.saturating_sub(2),
                    1,
                ),
            );
        }
    }
    if let Some(path) = &app.dialogs.compose_path {
        panel(f,a," Compose / Run existing YAML ",format!("Path to an existing .yaml or .yml file:\n\n{}\n\nEnter Validate and run · Esc Back\n\nThe file is used as written; Docker-Goo does not generate YAML.\nRelative mounts and .env resolve through Compose.\nRequires the installed Docker Compose plugin.\n\n{}",clean(path),if app.dialogs.compose_running {"Validating / starting project… (up to 5 minutes)".into()}else{app.dialogs.compose_error.lines().map(clean).collect::<Vec<_>>().join("\n")}),0);
    }
    app.dialogs.native_area = None;
    if let Some((r, command)) = &app.dialogs.shell {
        panel(f,a," Container terminal ",format!("{} · {}\n\n$ {}\n\nTab cycles auto / Bash / sh.\nBackspace and type any executable, e.g. spark-shell.\nauto prefers Bash and falls back to sh.\n\n[Enter] - Shell · [Ctrl+Enter] - Native -it\n[Esc] - Cancel · [Ctrl+D] - Detach (API shell)\n\nStopped containers must already have an interactive TTY;\nthey start and attach their existing main process.\nUse n → Ctrl+P for a new interactive container.",clean(&r.name),r.state,command),0);
    }
    if app.dialogs.shell.is_some() {
        let button = Rect::new(
            a.x + 1,
            a.bottom().saturating_sub(2),
            a.width.saturating_sub(2),
            1,
        );
        f.render_widget(
            Paragraph::new("[Ctrl+Enter] - Native terminal (-it)").style(theme::text()),
            button,
        );
        highlight(f, button);
        app.dialogs.native_area = Some(button);
    }
    if let Some(urls) = &app.dialogs.urls {
        panel(f,a," Open published port ",format!("Choose a web port (not every TCP port serves HTTP).\n↑↓ Select · s HTTP/HTTPS · Enter Open · Esc Cancel\n\n{}",urls.iter().enumerate().map(|(i,u)|format!("{} {u}",if i==app.dialogs.url_index{"›"}else{" "})).collect::<Vec<_>>().join("\n")),0);
    }
    if app.input == Some(InputMode::Command) {
        app.hits.clear();
        theme::fill(f, screen, theme::BASE);
        let a = screen;
        let chunks = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(a);
        let cols = if a.width >= 85 {
            Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[0])
        } else {
            Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(chunks[0])
        };
        let lines = app
            .dialogs
            .output
            .lines()
            .map(|line| {
                ratatui::text::Line::raw(line)
                    .width()
                    .max(1)
                    .div_ceil(cols[0].width.saturating_sub(2).max(1) as usize)
            })
            .sum::<usize>();
        let max_scroll = lines
            .saturating_sub(cols[0].height.saturating_sub(2) as usize)
            .min(u16::MAX as usize) as u16;
        if app.dialogs.follow_output {
            app.dialogs.output_scroll = max_scroll;
        } else {
            app.dialogs.output_scroll = app.dialogs.output_scroll.min(max_scroll);
        }
        console_panel(
            f,
            cols[0],
            &format!(
                " Host: {} · [Ctrl+O] - Native terminal ",
                super::console::cwd(app).display()
            ),
            app.dialogs
                .output
                .lines()
                .map(clean)
                .collect::<Vec<_>>()
                .join("\n"),
            app.dialogs.output_scroll,
        );
        console_panel(
            f,
            cols[1],
            " Docker help ",
            app.dialogs
                .guide
                .lines()
                .map(clean)
                .collect::<Vec<_>>()
                .join("\n"),
            app.dialogs.help_scroll,
        );
        app.dialogs.suggestion_rows.clear();
        let (choices, resources) = console_suggestions(app);
        if !choices.is_empty() {
            let height = (choices.len().min(5) as u16 + 2).min(chunks[0].height);
            let start = app.dialogs.suggestion_index.saturating_sub(4);
            let content = choices
                .iter()
                .enumerate()
                .skip(start)
                .take(5)
                .map(|(i, s)| {
                    format!(
                        "{} {}",
                        if i == app.dialogs.suggestion_index {
                            "›"
                        } else {
                            " "
                        },
                        s
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            let suggestion_area = Rect::new(
                cols[0].x,
                chunks[1].y.saturating_sub(height),
                cols[0].width,
                height,
            );
            panel(
                f,
                suggestion_area,
                if resources {
                    " Files / images / containers "
                } else {
                    " Commands "
                },
                content,
                0,
            );
            if resources {
                for y in suggestion_area.y + 1..suggestion_area.bottom().saturating_sub(1) {
                    for x in suggestion_area.x + 1..suggestion_area.right().saturating_sub(1) {
                        if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
                            cell.set_fg(ratatui::style::Color::Rgb(78, 230, 160));
                        }
                    }
                }
            }
            for (i, _) in choices.iter().enumerate().skip(start).take(5) {
                let row = Rect::new(
                    suggestion_area.x + 1,
                    suggestion_area.y + 1 + (i - start) as u16,
                    suggestion_area.width.saturating_sub(2),
                    1,
                );
                if row.y < suggestion_area.bottom().saturating_sub(1) {
                    app.dialogs.suggestion_rows.push((row, i));
                    if i == app.dialogs.suggestion_index {
                        highlight(f, row);
                    }
                }
            }
        }
        app.dialogs.console_controls = Some(chunks[1]);
        panel(
            f,
            chunks[1],
            if app.dialogs.suggestions_hidden {
                " [Ctrl+S] - Suggestions: off  [Esc] - Quit "
            } else {
                " [Ctrl+S] - Suggestions: on   [Esc] - Quit "
            },
            String::new(),
            0,
        );
        input_line(
            f,
            Rect::new(
                chunks[1].x + 1,
                chunks[1].y + 1,
                chunks[1].width.saturating_sub(2),
                1,
            ),
            "$ ",
            &app.command,
            app.dialogs.command_cursor,
            true,
        );
    }
    if app.dialogs.themes.is_some() {
        app.hits.clear();
        let w = screen.width.saturating_sub(4).min(58);
        let h = 11.min(screen.height);
        let r = Rect::new((screen.width - w) / 2, (screen.height - h) / 2, w, h);
        panel(
            f,
            r,
            " Themes · ↑↓ Preview · Enter Save · Esc Cancel ",
            String::new(),
            0,
        );
        for (i, name) in theme::NAMES.iter().enumerate() {
            let row = Rect::new(
                r.x + 2,
                r.y + 2 + i as u16 * 2,
                r.width.saturating_sub(4),
                1,
            );
            f.render_widget(
                Paragraph::new(format!(
                    "{} {}  {}",
                    if app.theme_index == i { "›" } else { " " },
                    i + 1,
                    name
                ))
                .style(theme::text()),
                row,
            );
            app.hits
                .push((row, KeyCode::Char(char::from(b'1' + i as u8))));
        }
    }
}

fn image_choices(app: &App) -> Vec<String> {
    let query = app.dialogs.image_query.to_lowercase();
    let mut images = app
        .snapshot
        .images
        .iter()
        .flat_map(|r| {
            let tags = r
                .name
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "<none>:<none>")
                .map(str::to_owned)
                .collect::<Vec<_>>();
            if tags.is_empty() {
                vec![r.id.clone()]
            } else {
                tags
            }
        })
        .filter(|s| s.to_lowercase().contains(&query))
        .collect::<Vec<_>>();
    images.sort();
    images.dedup();
    images
}
fn choose_image(app: &mut App, image: String) {
    let form = app
        .dialogs
        .form
        .get_or_insert_with(|| Form::new(String::new()));
    form.cursors[0] = image.len();
    form.fields[0] = image;
    form.focus = usize::from(!form.fields[0].is_empty());
    app.dialogs.picker = false;
}
pub(super) fn mouse(app: &mut App, mouse: crossterm::event::MouseEvent) -> bool {
    if app.input == Some(InputMode::Command) {
        if matches!(
            mouse.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            app.dialogs.follow_output = false;
            app.dialogs.output_scroll = if mouse.kind == MouseEventKind::ScrollUp {
                app.dialogs.output_scroll.saturating_sub(3)
            } else {
                app.dialogs.output_scroll.saturating_add(3)
            };
            return true;
        }
        if mouse.kind == MouseEventKind::Down(crossterm::event::MouseButton::Left) {
            if let Some((_, index)) = app
                .dialogs
                .suggestion_rows
                .iter()
                .find(|(r, _)| r.contains((mouse.column, mouse.row).into()))
            {
                let index = *index;
                if let Some(value) = console_suggestions(app).0.get(index) {
                    app.command = value.clone();
                    app.dialogs.command_cursor = app.command.len();
                    app.dialogs.suggestion_index = 0;
                }
                return true;
            }
            if let Some(area) = app.dialogs.console_controls {
                if mouse.row == area.y {
                    if mouse.column > area.x && mouse.column < area.x + 27 {
                        app.dialogs.suggestions_hidden = !app.dialogs.suggestions_hidden;
                        app.dialogs.suggestion_index = 0;
                    } else if mouse.column >= area.x + 28 && mouse.column < area.x + 40 {
                        app.input = None;
                    }
                }
            }
        }
        return true;
    }
    if !app.dialogs.picker && app.dialogs.form.is_none() {
        return false;
    }
    if mouse.kind != MouseEventKind::Down(crossterm::event::MouseButton::Left) {
        return true;
    }
    let (width, height) = crossterm::terminal::size().unwrap_or((80, 24));
    let w = width.saturating_sub(4).min(100);
    let h = height.saturating_sub(4).min(25);
    let x = (width - w) / 2;
    let y = (height - h) / 2;
    if mouse.column <= x || mouse.column >= x + w.saturating_sub(1) {
        return true;
    }
    if app.dialogs.picker {
        if mouse.row >= y + 5 && mouse.row < y + h.saturating_sub(1) {
            let rows = h.saturating_sub(6).max(1) as usize;
            let start = app.dialogs.image_index.saturating_sub(rows - 1);
            if let Some(image) = image_choices(app).get(start + (mouse.row - y - 5) as usize) {
                choose_image(app, image.clone());
            }
        }
    } else if !app.busy {
        if app.dialogs.interactive_area
            .is_some_and(|r| r.contains((mouse.column, mouse.row).into()))
        {
            if let Some(form) = app.dialogs.form.as_mut() {
                form.focus = 7;
                form.toggle_interactive();
            }
            return true;
        }
        if let Some(i) = app
            .dialogs
            .form_areas
            .iter()
            .position(|r| r.contains((mouse.column, mouse.row).into()))
        {
            if let Some(form) = app.dialogs.form.as_mut() {
                form.focus = i;
                form.cursors[i] = form.fields[i].len();
            }
            if i == 0 {
                app.dialogs.picker = true;
                app.dialogs.image_query.clear();
                app.dialogs.image_index = 0;
            }
        }
    }
    true
}

pub(super) fn paste(app: &mut App, value: &str) {
    // Pasting never submits a command or creates a container.
    let value = value.replace(['\r', '\n'], " ");
    if app.input == Some(InputMode::Command) {
        super::console::insert(&mut app.command, &mut app.dialogs.command_cursor, &value);
        app.dialogs.suggestion_index = 0;
    } else if !app.busy {
        if app.dialogs.picker {
            app.dialogs.image_query.push_str(&value);
        } else if let Some(form) = app.dialogs.form.as_mut() {
            if form.focus < 7 {
                super::console::insert(
                    &mut form.fields[form.focus],
                    &mut form.cursors[form.focus],
                    &value,
                );
            }
        } else if let Some(path) = app.dialogs.compose_path.as_mut() {
            path.push_str(&value);
        } else if let Some((_, cmd)) = app.dialogs.shell.as_mut() {
            cmd.push_str(&value);
        } else if app.input == Some(InputMode::Search) {
            app.filter.push_str(&value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paste_preserves_form_and_targets_open_picker() {
        let mut app = App::new();
        let mut form = Form::new("alpine".into());
        form.focus = 1;
        app.dialogs.form = Some(form);
        app.dialogs.form_error = "Invalid port".into();
        paste(&mut app, "my-name");
        assert_eq!(app.dialogs.form.as_ref().unwrap().fields[1], "my-name");
        assert_eq!(app.dialogs.form_error, "Invalid port");
        app.dialogs.picker = true;
        paste(&mut app, "ubuntu");
        assert_eq!(app.dialogs.image_query, "ubuntu");
        assert_eq!(app.dialogs.form.as_ref().unwrap().fields[1], "my-name");
    }
    #[test]
    fn image_picker_keeps_tags_and_existing_options() {
        let mut app = App::new();
        app.snapshot.images.push(Resource {
            id: "sha256:1".into(),
            name: "ubuntu:latest, ubuntu:24.04".into(),
            detail: String::new(),
            state: String::new(),
            extra: String::new(),
        });
        assert_eq!(image_choices(&app), ["ubuntu:24.04", "ubuntu:latest"]);
        app.dialogs.form = Some(Form::new("old".into()));
        app.dialogs.form.as_mut().unwrap().fields[1] = "my-container".into();
        choose_image(&mut app, "ubuntu:24.04".into());
        let form = app.dialogs.form.as_ref().unwrap();
        assert_eq!(form.fields[0], "ubuntu:24.04");
        assert_eq!(form.fields[1], "my-container");
    }
}
