mod brand;
mod clipboard;
mod console;
mod dialogs;
mod log_text;
mod shell;
mod theme;
mod view;

use crate::core::{Action, Engine, Kind, Metrics, Resource, ShellSession, Snapshot};
use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, EventStream, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use ratatui::{backend::CrosstermBackend, widgets::TableState, Terminal};
use std::{
    collections::VecDeque,
    io::{self, Stdout},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, Notify},
    task::JoinHandle,
};

type Screen = Terminal<CrosstermBackend<Stdout>>;
struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> Result<Self> {
        let guard = Self;
        enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            EnableBracketedPaste
        )?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
    }
}
fn restore() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
        crossterm::cursor::Show
    );
}

#[derive(Default)]
struct Jobs(Vec<JoinHandle<()>>);
impl Drop for Jobs {
    fn drop(&mut self) {
        for job in &self.0 {
            job.abort();
        }
    }
}

enum Message {
    Clipboard(u64, Result<String>),
    Compose(Result<String>),
    ConsoleOutput(u64, String),
    ConsoleDone(u64, Result<()>),
    Created(Result<String>),
    Terminal(std::process::Command),
    Guide(String, Result<String>),
    Urls(Result<Vec<String>>),
    Snapshot(Result<Snapshot>),
    Action(Result<()>, String),
    Logs(u64, String),
    Stats(u64, Metrics),
    StreamError(u64, String),
    Inspect(u64, Result<String>),
    Shell(Result<ShellSession>),
}
#[derive(Clone, Copy, PartialEq)]
enum InputMode {
    Search,
    Command,
}
struct App {
    clipboard_epoch: u64,
    dialogs: dialogs::Dialogs,
    snapshot: Snapshot,
    project: Option<String>,
    theme_index: usize,
    online: bool,
    daemon: String,
    tab: usize,
    home: bool,
    drawer_focus: bool,
    nav_index: usize,
    table: TableState,
    filter: String,
    input: Option<InputMode>,
    command: String,
    status: String,
    logs: VecDeque<String>,
    log_scroll: usize,
    stats: Option<Metrics>,
    stream_key: Option<(String, String)>,
    epoch: u64,
    streams: Jobs,
    busy: bool,
    confirm: Option<Resource>,
    inspect: Option<String>,
    inspect_epoch: u64,
    inspect_scroll: u16,
    help: bool,
    table_area: ratatui::layout::Rect,
    nav_areas: [ratatui::layout::Rect; 5],
    logo_area: ratatui::layout::Rect,
    hits: Vec<(ratatui::layout::Rect, KeyCode)>,
    detail_tab: usize,
    started: std::time::Instant,
}
impl App {
    fn new() -> Self {
        Self {
            clipboard_epoch: 0,
            dialogs: dialogs::Dialogs::default(),
            snapshot: Snapshot::default(),
            project: None,
            theme_index: theme::load(),
            online: false,
            daemon: "Connecting to Docker Engine...".into(),
            tab: 0,
            home: true,
            drawer_focus: true,
            nav_index: 0,
            table: TableState::default().with_selected(Some(0)),
            filter: String::new(),
            input: None,
            command: String::new(),
            status: "Press ? for shortcuts".into(),
            logs: VecDeque::new(),
            log_scroll: 0,
            stats: None,
            stream_key: None,
            epoch: 0,
            streams: Jobs::default(),
            busy: false,
            confirm: None,
            inspect: None,
            inspect_epoch: 0,
            inspect_scroll: 0,
            help: false,
            table_area: Default::default(),
            nav_areas: [Default::default(); 5],
            logo_area: Default::default(),
            hits: Vec::new(),
            detail_tab: 0,
            started: std::time::Instant::now(),
        }
    }
    fn kind(&self) -> Kind {
        Kind::ALL[self.tab]
    }
    fn rows(&self) -> Vec<Resource> {
        let query = self.filter.to_lowercase();
        self.snapshot
            .resources(self.kind())
            .iter()
            .filter(|r| {
                self.project
                    .as_ref()
                    .is_none_or(|p| self.snapshot.compose_members.get(&r.id) == Some(p))
            })
            .filter(|r| {
                format!("{} {} {} {}", r.name, r.id, r.detail, r.state)
                    .to_lowercase()
                    .contains(&query)
            })
            .cloned()
            .collect()
    }
    fn selected(&self) -> Option<Resource> {
        self.table
            .selected()
            .and_then(|i| self.rows().get(i).cloned())
    }
    fn move_selection(&mut self, delta: isize) {
        let count = self.rows().len();
        self.table.select(if count == 0 {
            None
        } else {
            Some(
                self.table
                    .selected()
                    .unwrap_or(0)
                    .saturating_add_signed(delta)
                    .min(count - 1),
            )
        });
    }
    fn change_tab(&mut self, tab: usize) {
        self.project = None;
        self.tab = tab % Kind::ALL.len();
        self.nav_index = self.tab;
        self.home = false;
        self.drawer_focus = false;
        self.table.select(Some(0));
        self.filter.clear();
    }
    fn go_home(&mut self) {
        self.home = true;
        self.drawer_focus = true;
        self.nav_index = self.tab;
        self.filter.clear();
    }
    fn navigate(&mut self, delta: isize) {
        self.nav_index =
            (self.nav_index as isize + delta).rem_euclid(Kind::ALL.len() as isize) as usize;
    }
    fn update_snapshot(&mut self, snapshot: Snapshot) {
        let selected = self.selected().map(|r| r.id);
        self.snapshot = snapshot;
        let rows = self.rows();
        let index = selected.and_then(|id| rows.iter().position(|r| r.id == id));
        self.table.select(index.or_else(|| {
            (!rows.is_empty()).then_some(
                self.table
                    .selected()
                    .unwrap_or(0)
                    .min(rows.len().saturating_sub(1)),
            )
        }));
    }
    fn start_streams(&mut self, engine: Arc<dyn Engine>, tx: mpsc::Sender<Message>) {
        let selected = self
            .selected()
            .filter(|_| !self.home && self.online && self.kind() == Kind::Containers);
        let key = selected.as_ref().map(|r| (r.id.clone(), r.state.clone()));
        if key == self.stream_key {
            return;
        }
        self.streams = Jobs::default();
        self.stream_key = key;
        self.epoch += 1;
        self.logs.clear();
        self.stats = None;
        self.log_scroll = 0;
        let Some(resource) = selected else {
            return;
        };
        let epoch = self.epoch;
        let log_engine = engine.clone();
        let log_tx = tx.clone();
        let id = resource.id.clone();
        self.streams.0.push(tokio::spawn(async move {
            let mut logs = match log_engine.logs(&id).await {
                Ok(stream) => stream,
                Err(error) => {
                    let _ = log_tx
                        .send(Message::StreamError(epoch, format!("Logs: {error}")))
                        .await;
                    return;
                }
            };
            let mut text = log_text::LogText::default();
            while let Some(item) = logs.next().await {
                let message = match item {
                    Ok(chunk) => Message::Logs(epoch, text.feed(&chunk)),
                    Err(e) => Message::StreamError(epoch, format!("Logs: {e}")),
                };
                if log_tx.send(message).await.is_err() {
                    break;
                }
            }
        }));
        if resource.state == "running" {
            self.streams.0.push(tokio::spawn(async move {
                let mut stats = match engine.stats(&resource.id).await {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = tx
                            .send(Message::StreamError(epoch, format!("Stats: {error}")))
                            .await;
                        return;
                    }
                };
                while let Some(item) = stats.next().await {
                    let message = match item {
                        Ok(value) => Message::Stats(epoch, value),
                        Err(e) => Message::StreamError(epoch, format!("Stats: {e}")),
                    };
                    if tx.send(message).await.is_err() {
                        break;
                    }
                }
            }));
        }
    }
}

pub async fn run(
    engine: Arc<dyn Engine>,
    shell_command: String,
    open_console: bool,
    cwd: Option<std::path::PathBuf>,
) -> Result<()> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        hook(info);
    }));
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.clear()?;
    let mut events = EventStream::new();
    let (tx, mut rx) = mpsc::channel(256);
    let refresh = Arc::new(Notify::new());
    let mut jobs = Jobs::default();
    let (poll_engine, poll_tx, poll_refresh) = (engine.clone(), tx.clone(), refresh.clone());
    jobs.0.push(tokio::spawn(async move {
        loop {
            let result = poll_engine.snapshot().await;
            if poll_tx.send(Message::Snapshot(result)).await.is_err() { break; }
            tokio::select! { _ = tokio::time::sleep(Duration::from_secs(3)) => {}, _ = poll_refresh.notified() => {} }
        }
    }));
    let mut app = App::new();
    if let Some(dir) = cwd {
        let dir = dir.canonicalize()?;
        anyhow::ensure!(dir.is_dir(), "--cwd must point to a directory");
        app.dialogs.cwd = Some(dir);
    }
    if open_console {
        app.input = Some(InputMode::Command);
        app.dialogs.guide = crate::core::commands::GUIDE.into();
    }
    let mut redraw = tokio::time::interval(Duration::from_millis(100));
    loop {
        app.start_streams(engine.clone(), tx.clone());
        jobs.0.retain(|job| !job.is_finished());
        tokio::select! {
            _ = redraw.tick() => { terminal.draw(|f| view::draw(f, &mut app, engine.endpoint()))?; }
            message = rx.recv() => {
                let Some(message) = message else { break; };
                match message {
                    Message::Clipboard(epoch,result) if epoch==app.clipboard_epoch=>{
                        match result {
                            Ok(text)=>dialogs::paste(&mut app,&text),
                            Err(e)=>{let error=format!("{e:#}");app.status=error.clone();if app.dialogs.form.is_some(){app.dialogs.form_error=error.clone();}
                            if app.input==Some(InputMode::Command){console::append(&mut app,&format!("\n{error}\n"));}}
                        }
                    }
                    Message::Compose(result)=>{app.busy=false;app.dialogs.compose_running=false;match result{Ok(project)=>{app.dialogs.compose_path=None;app.change_tab(0);app.project=Some(project.clone());app.status=format!("Compose {project} started · o Projects");},Err(e)=>app.dialogs.compose_error=format!("{e:#}")};refresh.notify_one();}
                    Message::ConsoleOutput(id,text) if id==app.dialogs.command_id => console::append(&mut app,&text),
                    Message::ConsoleDone(id,result) if id==app.dialogs.command_id => {console::append(&mut app,&match result{Ok(())=>"\n[exit 0]\n".into(),Err(e)=>format!("\n[error: {e:#}]\n")});app.dialogs.command_job=None;refresh.notify_one();}
                    Message::Created(result)=>{app.busy=false;match result {Ok(_)=>{app.dialogs.form=None;app.status="Container created and started".into();},Err(e)=>{app.dialogs.form_error=format!("{e:#}");app.status=app.dialogs.form_error.clone();}}refresh.notify_one();}
                    Message::Terminal(command)=>{drop(events);let result=shell::host(&mut terminal,command).await;events=EventStream::new();console::append(&mut app,&match result{Ok(())=>"\n[terminal session ended]\n".into(),Err(e)=>format!("\n[terminal: {e:#}]\n")});refresh.notify_one();}
                    Message::Guide(topic,result) if topic==app.dialogs.topic => { app.dialogs.guide=result.unwrap_or_else(|_|crate::core::commands::GUIDE.into()); }
                    Message::Urls(result) => {match result {Ok(urls) if !urls.is_empty()=>{app.dialogs.urls=Some(urls);app.dialogs.url_index=0;},Ok(_)=>app.status="No reachable published TCP ports. Publish a web port when creating the container; remote loopback ports need a tunnel.".into(),Err(e)=>app.status=format!("Web ports: {e:#}")}}

                    Message::Snapshot(Ok(snapshot)) => { app.online = true; app.daemon = snapshot.engine.clone(); app.update_snapshot(snapshot); }
                    Message::Snapshot(Err(error)) => { app.online = false; app.daemon = format!("Offline: {error:#} | retrying"); }
                    Message::Action(result, label) => {
                        app.busy = false; app.status = match result { Ok(()) => format!("{label} completed"), Err(e) => format!("{label}: {e:#}") }; refresh.notify_one();
                    }
                    Message::Logs(epoch, text) if epoch == app.epoch => {
                        for line in text.lines() { app.logs.push_back(clean(line).chars().take(16384).collect()); }
                        while app.logs.len() > 1000 { app.logs.pop_front(); }
                    }
                    Message::Stats(epoch, value) if epoch == app.epoch => app.stats = Some(value),
                    Message::StreamError(epoch, error) if epoch == app.epoch => { app.logs.push_back(clean(&error)); while app.logs.len() > 1000 { app.logs.pop_front(); } }
                    Message::Inspect(epoch, value) if epoch == app.inspect_epoch && app.inspect.is_some() => {
                        app.inspect = Some(match value { Ok(text) => text, Err(e) => format!("Inspect failed: {e:#}") });
                    }
                    Message::Shell(result) => {
                        app.busy = false;
                        match result {
                            Ok(session) => {
                                app.status = match shell::run(&mut terminal, &mut events, engine.clone(), session).await { Ok(()) => "Shell closed".into(), Err(e) => format!("Shell: {e:#}") };
                                refresh.notify_one();
                            }
                            Err(e) => app.status = format!("Cannot open shell: {e:#}. Try --shell /bin/bash if needed."),
                        }
                    }
                    _ => {}
                }
            }
            event = events.next() => {
                let Some(event) = event else { break; };
                let mut event = event?;
                // Ignore a delayed clipboard result after the user edits or switches targets.
                if matches!(&event,Event::Key(_)|Event::Paste(_)|Event::Mouse(crossterm::event::MouseEvent{kind:MouseEventKind::Down(_),..})) {app.clipboard_epoch=app.clipboard_epoch.wrapping_add(1);}
                if clipboard::request(&mut app,&event,&tx,&mut jobs){continue;}
                if let Event::Paste(text)=&event { dialogs::paste(&mut app,text); continue; }
                if let Event::Mouse(mouse)=&event {
                    if app.dialogs.shell.is_some() && mouse.kind==MouseEventKind::Down(crossterm::event::MouseButton::Left) && app.dialogs.native_area.is_some_and(|a|a.contains((mouse.column,mouse.row).into())) {
                        event=Event::Key(crossterm::event::KeyEvent::new(KeyCode::Enter,KeyModifiers::CONTROL));
                    } else if app.dialogs.form.is_some() && mouse.kind==MouseEventKind::Down(crossterm::event::MouseButton::Left) && app.dialogs.create_area.is_some_and(|a|a.contains((mouse.column,mouse.row).into())) {
                        if let Some(form)=app.dialogs.form.as_mut(){form.focus=7;}
                        event=Event::Key(crossterm::event::KeyEvent::new(KeyCode::Enter,KeyModifiers::NONE));
                    } else if dialogs::mouse(&mut app,*mouse) {continue;}
                }
                if app.dialogs.about || app.dialogs.themes.is_some() || (app.input.is_none() && app.confirm.is_none() && app.inspect.is_none() && !app.help && !dialogs::active(&app)) {
                    if let Event::Mouse(mouse) = &event {
                        if mouse.kind == MouseEventKind::Down(crossterm::event::MouseButton::Left) {
                            if let Some((_, key)) = app.hits.iter().find(|(rect,_)| rect.contains((mouse.column,mouse.row).into())) {
                                let key = *key;
                                if !app.home { app.drawer_focus = false; }
                                event = Event::Key(crossterm::event::KeyEvent::new(key, KeyModifiers::NONE));
                            }
                        }
                    }
                }
                match event {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        if dialogs::key(&mut app,key,engine.clone(),&tx,&mut jobs,&refresh,&shell_command) { continue; }
                        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) { break; }
                        if app.confirm.is_some() {
                            if key.code == KeyCode::Char('y') {
                                let selected = app.confirm.take().unwrap();
                                dispatch_action(&mut app, engine.clone(), &tx, &mut jobs, selected, Action::Delete);
                            } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) { app.confirm = None; }
                            continue;
                        }
                        if app.help { if matches!(key.code, KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')) { app.help = false; } continue; }
                        if app.inspect.is_some() {
                            match key.code {
                                KeyCode::Esc | KeyCode::Char('q') => { app.inspect = None; app.inspect_epoch += 1; }
                                KeyCode::Down | KeyCode::Char('j') => app.inspect_scroll = app.inspect_scroll.saturating_add(1),
                                KeyCode::Up | KeyCode::Char('k') => app.inspect_scroll = app.inspect_scroll.saturating_sub(1),
                                KeyCode::PageDown => app.inspect_scroll = app.inspect_scroll.saturating_add(20),
                                KeyCode::PageUp => app.inspect_scroll = app.inspect_scroll.saturating_sub(20),
                                _ => {}
                            }
                            continue;
                        }
                        let code = key.code;
                        if app.input==Some(InputMode::Search) {
                            match code {KeyCode::Esc=>{app.input=None;app.filter.clear();},KeyCode::Enter=>app.input=None,KeyCode::Backspace=>{app.filter.pop();},KeyCode::Char(c) if app.filter.len()<256=>{app.filter.push(c);},_=>{}}
                            app.table.select(Some(0));continue;
                        }
                        if app.home || app.drawer_focus {
                            match code {
                                KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => app.navigate(1),
                                KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => app.navigate(-1),
                                KeyCode::Enter | KeyCode::Right => app.change_tab(app.nav_index),
                                KeyCode::Char(c @ '1'..='5') => app.change_tab(c as usize - '1' as usize),
                                KeyCode::Char('q') => break,
                                KeyCode::Char('?') => app.help = true,

                                KeyCode::Char('h') | KeyCode::Home => app.go_home(),
                                KeyCode::Esc if !app.home => app.drawer_focus = false,
                                KeyCode::F(5) => refresh.notify_one(),
                                _ => {}
                            }
                            continue;
                        }
                        match code {
                            KeyCode::Char('q') => break,
                            KeyCode::Char('?') => app.help = true,
                            KeyCode::Tab | KeyCode::BackTab | KeyCode::Left => { app.drawer_focus = true; app.nav_index = app.tab; }
                            KeyCode::Char('h') | KeyCode::Home => app.go_home(),
                            KeyCode::Char(c @ '1'..='5') => app.change_tab(c as usize - '1' as usize),
                            KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
                            KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
                            KeyCode::Char('/') => { app.input = Some(InputMode::Search); }

                            KeyCode::Esc => { if app.filter.is_empty() { app.drawer_focus = true; } else { app.filter.clear(); } },
                            KeyCode::F(5) => { refresh.notify_one(); app.stream_key = None; }
                            KeyCode::PageUp => app.log_scroll = (app.log_scroll + 10).min(app.logs.len().saturating_sub(1)),
                            KeyCode::PageDown => app.log_scroll = app.log_scroll.saturating_sub(10),
                            KeyCode::Char('l') | KeyCode::End => { app.log_scroll = 0; app.detail_tab = 0; }
                            KeyCode::Char('g') => app.detail_tab = 1,
                            KeyCode::Char('e') | KeyCode::Enter => {
                                if code==KeyCode::Enter && app.kind()==Kind::Compose { if let Some(r)=app.selected(){app.change_tab(0);app.project=Some(r.id);}continue;}
                                if let Some(resource) = app.selected().filter(|_| app.online) {
                                    app.inspect_epoch += 1; let epoch = app.inspect_epoch;
                                    app.inspect = Some("Loading inspect...".into()); app.inspect_scroll = 0;
                                    let (engine, tx, kind) = (engine.clone(), tx.clone(), app.kind());
                                    jobs.0.push(tokio::spawn(async move { let _ = tx.send(Message::Inspect(epoch, engine.inspect(kind, &resource.id).await)).await; }));
                                }
                            }
                            KeyCode::Char(c @ ('s' | 'x' | 'r' | 'p' | 'u' | 'd')) => {
                                if app.kind() != Kind::Containers { app.status = "Container actions are available in Containers".into(); continue; }
                                if app.busy || !app.online { app.status = "Wait for the current operation and an online daemon".into(); continue; }
                                let Some(resource) = app.selected() else { continue; };
                                if c == 'd' {
                                    if matches!(resource.state.as_str(), "running" | "paused" | "restarting") { app.status = "Stop the container before deleting it".into(); }
                                    else { app.confirm = Some(resource); }
                                } else {
                                    let action = match c { 's' => Action::Start, 'x' => Action::Stop, 'r' => Action::Restart, 'p' => Action::Pause, _ => Action::Resume };
                                    dispatch_action(&mut app, engine.clone(), &tx, &mut jobs, resource, action);
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Mouse(mouse) if app.input.is_none() && app.confirm.is_none() && app.inspect.is_none() && !app.help && !dialogs::active(&app) => {
                        match mouse.kind {
                            MouseEventKind::ScrollDown => { if app.home || app.drawer_focus { app.navigate(1); } else { app.move_selection(1); } }
                            MouseEventKind::ScrollUp => { if app.home || app.drawer_focus { app.navigate(-1); } else { app.move_selection(-1); } }
                            MouseEventKind::Down(crossterm::event::MouseButton::Left) => {
                                let point = (mouse.column, mouse.row).into();
                                if app.logo_area.contains(point) { app.go_home(); }
                                else if let Some(index) = app.nav_areas.iter().position(|area| area.contains(point)) {
                                    app.change_tab(index);
                                } else if app.table_area.contains((mouse.column, mouse.row).into()) && mouse.row >= app.table_area.y + 2 && mouse.row < app.table_area.bottom().saturating_sub(1) {
                                    let index = app.table.offset() + (mouse.row - app.table_area.y - 2) as usize;
                                    if index < app.rows().len() { app.table.select(Some(index)); app.drawer_focus = false; }
                                }
                            }
                            _ => {}
                        }
                    }
                    Event::Resize(_, _) => { terminal.autoresize()?; }
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
fn dispatch_action(
    app: &mut App,
    engine: Arc<dyn Engine>,
    tx: &mpsc::Sender<Message>,
    jobs: &mut Jobs,
    resource: Resource,
    action: Action,
) {
    if app.busy || !app.online {
        return;
    }
    app.busy = true;
    let label = format!("{action:?} {}", clean(&resource.name));
    app.status = format!("{label}...");
    let tx = tx.clone();
    jobs.0.push(tokio::spawn(async move {
        let _ = tx
            .send(Message::Action(
                engine.action(&resource.id, action).await,
                label,
            ))
            .await;
    }));
}
fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .collect()
}

#[cfg(test)]
mod navigation_tests {
    use super::*;
    #[test]
    fn home_drawer_and_resource_selection_stay_separate() {
        let mut app = App::new();
        assert!(app.home);
        app.navigate(-1);
        assert_eq!(app.nav_index, 4);
        app.change_tab(1);
        assert!(!app.home && !app.drawer_focus);
        assert_eq!(app.kind(), Kind::Images);
        app.drawer_focus = true;
        app.navigate(1);
        assert_eq!(app.kind(), Kind::Images);
        assert_eq!(app.nav_index, 2);
        app.change_tab(app.nav_index);
        assert_eq!(app.kind(), Kind::Volumes);
        app.go_home();
        assert!(app.home && app.drawer_focus);
        app.snapshot.containers = vec![
            Resource {
                id: "id1".into(),
                name: "web".into(),
                detail: String::new(),
                state: "running".into(),
                extra: String::new(),
            },
            Resource {
                id: "id2".into(),
                name: "web-other".into(),
                detail: String::new(),
                state: "running".into(),
                extra: String::new(),
            },
        ];
        app.snapshot
            .compose_members
            .insert("id1".into(), "demo".into());
        app.change_tab(0);
        app.project = Some("demo".into());
        assert_eq!(app.rows().len(), 1);
        assert_eq!(app.rows()[0].id, "id1");
        app.change_tab(0);
        assert_eq!(app.rows().len(), 2);
    }
}
