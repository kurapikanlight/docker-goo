use super::*;
use crate::core::commands;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt};

pub(super) fn insert(text: &mut String, cursor: &mut usize, value: &str) {
    *cursor = (*cursor).min(text.len());
    while !text.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
    let value: String = value
        .chars()
        .filter(|c| !c.is_control() || *c == '\t')
        .take(16384usize.saturating_sub(text.len()))
        .collect();
    text.insert_str(*cursor, &value);
    *cursor += value.len();
}
pub(super) fn edit(text: &mut String, cursor: &mut usize, code: KeyCode) -> bool {
    *cursor = (*cursor).min(text.len());
    while !text.is_char_boundary(*cursor) {
        *cursor -= 1;
    }
    let prev = || {
        text[..*cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    };
    let next = || {
        text[*cursor..]
            .chars()
            .next()
            .map_or(*cursor, |c| *cursor + c.len_utf8())
    };
    match code {
        KeyCode::Left => *cursor = prev(),
        KeyCode::Right => *cursor = next(),
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = text.len(),
        KeyCode::Backspace if *cursor > 0 => {
            let p = prev();
            text.drain(p..*cursor);
            *cursor = p;
        }
        KeyCode::Delete if *cursor < text.len() => {
            let n = next();
            text.drain(*cursor..n);
        }
        _ => return false,
    }
    true
}
pub(super) fn append(app: &mut App, text: &str) {
    app.dialogs.output.push_str(text);
    if app.dialogs.output.len() > 1_048_576 {
        let mut cut = app.dialogs.output.len() - 900_000;
        while !app.dialogs.output.is_char_boundary(cut) {
            cut += 1;
        }
        let cut = app.dialogs.output[cut..]
            .find('\n')
            .map_or(app.dialogs.output.len(), |n| cut + n + 1);
        app.dialogs.output.drain(..cut);
    }
}
pub(super) fn cwd(app: &App) -> PathBuf {
    app.dialogs
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
}
fn expand(path: &str) -> PathBuf {
    if path == "~" {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
    } else if let Some(rest) = path.strip_prefix("~/") {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest)
    } else {
        PathBuf::from(path)
    }
}
pub(super) fn file_suggestions(line: &str, cwd: &Path) -> Vec<String> {
    let at = line.rfind(char::is_whitespace).map_or(0, |n| n + 1);
    let (base, prefix) = line.split_at(at);
    let Ok(words) = shell_words::split(base) else {
        return vec![];
    };
    let last = words.last().map(String::as_str).unwrap_or("");
    let compose = words.iter().any(|s| s == "compose") && matches!(last, "-f" | "--file");
    let build = words.iter().any(|s| s == "build") && matches!(last, "-f" | "--file");
    let cd = words.first().is_some_and(|s| s == "cd") && words.len() == 1;
    if !compose && !build && !cd {
        return vec![];
    }
    let path = expand(prefix);
    let dir = if prefix.ends_with('/') {
        path.clone()
    } else {
        path.parent().unwrap_or(Path::new("")).into()
    };
    let name = if prefix.ends_with('/') {
        ""
    } else {
        path.file_name().and_then(|s| s.to_str()).unwrap_or("")
    };
    let Ok(entries) = std::fs::read_dir(cwd.join(&dir)) else {
        return vec![];
    };
    let mut matches = vec![];
    for entry in entries.flatten().take(2048) {
        let filename = entry.file_name().to_string_lossy().into_owned();
        let directory = entry.path().is_dir();
        if !filename.starts_with(name) {
            continue;
        }
        if !directory
            && (cd
                || (compose && !filename.ends_with(".yaml") && !filename.ends_with(".yml"))
                || (build && !filename.to_lowercase().contains("dockerfile")))
        {
            continue;
        }
        let raw = dir.join(&filename).to_string_lossy().into_owned();
        let raw = if directory { format!("{raw}/") } else { raw };
        matches.push(format!(
            "{base}{}{}",
            shell_words::quote(&raw),
            if directory { "" } else { " " }
        ));
    }
    matches.sort();
    matches
}
pub(super) fn suggestions(app: &App) -> (Vec<String>, bool) {
    if app.dialogs.suggestions_hidden || app.dialogs.command_cursor != app.command.len() {
        return (vec![], false);
    }
    let files = file_suggestions(&app.command, &cwd(app));
    if !files.is_empty() {
        return (files, true);
    }
    if app.command.trim() == "docker compose" && app.command.ends_with(' ') {
        let mut choices = file_suggestions("docker compose -f ", &cwd(app));
        choices.retain(|s| s.ends_with(' '));
        for s in &mut choices {
            s.push_str("up -d");
        }
        choices.extend(commands::suggestions(&app.command));
        return (choices, true);
    }
    let choices = commands::suggestions(&app.command);
    if !choices.is_empty() {
        return (choices, false);
    }
    (
        commands::resource_suggestions(&app.command, &app.snapshot),
        true,
    )
}
async fn output(
    mut stream: impl AsyncRead + Unpin,
    id: u64,
    tx: mpsc::Sender<Message>,
) -> Result<()> {
    let mut buffer = [0u8; 4096];
    let mut parser = log_text::LogText::default();
    let mut pending = Vec::new();
    loop {
        let n = stream.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        pending.extend_from_slice(&buffer[..n]);
        let valid = match std::str::from_utf8(&pending) {
            Ok(_) => pending.len(),
            Err(e) => e.valid_up_to(),
        };
        if valid > 0 {
            let value = String::from_utf8_lossy(&pending[..valid]).replace('\r', "\n");
            let value = parser.feed(&value);
            pending.drain(..valid);
            if tx.send(Message::ConsoleOutput(id, value)).await.is_err() {
                return Ok(());
            }
        } else if pending.len() > 4 {
            pending.remove(0);
        }
    }
    Ok(())
}
async fn stream(command: std::process::Command, id: u64, tx: mpsc::Sender<Message>) -> Result<()> {
    let mut command = tokio::process::Command::from(command);
    // A process group lets cancellation stop host child processes too.
    command.process_group(0);
    let mut child = command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    struct Group(u32);
    impl Drop for Group {
        fn drop(&mut self) {
            unsafe {
                if self.0 > 0 {
                    libc::kill(-(self.0 as i32), libc::SIGKILL);
                }
            }
        }
    }
    let group = Group(child.id().unwrap_or(0));
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(1800), async {
        let (_, _, status) = tokio::try_join!(
            output(stdout, id, tx.clone()),
            output(stderr, id, tx),
            async { Ok::<_, anyhow::Error>(child.wait().await?) }
        )?;
        anyhow::ensure!(
            status.success(),
            "Command exited with {}",
            status.code().unwrap_or(-1)
        );
        Ok(())
    })
    .await;
    drop(group);
    result.map_err(|_| {
        anyhow::anyhow!("Command timed out after 30 minutes; check Docker resource state")
    })?
}
pub(super) fn execute(app: &mut App, engine: Arc<dyn Engine>, tx: &mpsc::Sender<Message>) {
    if app
        .dialogs
        .command_job
        .as_ref()
        .is_some_and(|j| !j.is_finished())
    {
        return;
    }
    let line = app.command.trim().to_string();
    if line.is_empty() {
        return;
    }
    let dir = cwd(app);
    append(
        app,
        &format!(
            "\n====================\n{} $ {}\n====================\n",
            dir.display(),
            line
        ),
    );
    app.dialogs.follow_output = true;
    app.dialogs.history.push(line.clone());
    if app.dialogs.history.len() > 200 {
        app.dialogs.history.remove(0);
    }
    app.dialogs.history_index = None;
    app.command.clear();
    app.dialogs.command_cursor = 0;
    app.dialogs.suggestions_hidden = true;
    let parsed = shell_words::split(&line);
    if let Ok(words) = &parsed {
        if words.first().is_some_and(|s| s == "cd") {
            if words.len() > 2 {
                append(app, "cd: use one quoted directory path\n");
                return;
            }
            let target = dir.join(expand(words.get(1).map(String::as_str).unwrap_or("~")));
            match target.canonicalize() {
                Ok(p) if p.is_dir() => {
                    app.dialogs.cwd = Some(p);
                    append(app, "[directory changed]\n");
                }
                _ => append(app, "cd: directory does not exist or is not accessible\n"),
            }
            return;
        }
        if words.first().is_some_and(|s| s == "exit") {
            app.input = None;
            return;
        }
    }
    let mut command;
    let docker = parsed
        .as_ref()
        .ok()
        .is_some_and(|w| w.first().is_some_and(|s| s == "docker"));
    let mut interactive = false;
    if docker {
        let words = match parsed {
            Ok(w) => w,
            Err(e) => {
                append(app, &format!("{e}\n"));
                return;
            }
        };
        let args = &words[1..];
        if args.first().is_some_and(|s| {
            s == "login"
                || s == "logout"
                || s.starts_with('-') && !matches!(s.as_str(), "--help" | "--version" | "-h" | "-v")
        }) {
            append(
                app,
                "Account and engine-override flags are not supported here.\n",
            );
            return;
        }
        interactive = args
            .iter()
            .any(|s| matches!(s.as_str(), "-it" | "-ti" | "-i" | "--interactive"))
            && !args.iter().any(|s| s == "--help");
        command = engine.docker_cli();
        command.args(args);
    } else {
        command = std::process::Command::new("/bin/sh");
        command.args(["-c", &line]);
    }
    command.current_dir(dir);
    if interactive {
        let _ = tx.try_send(Message::Terminal(command));
        return;
    }
    app.dialogs.command_id = app.dialogs.command_id.wrapping_add(1);
    let id = app.dialogs.command_id;
    let tx = tx.clone();
    app.dialogs.command_job = Some(tokio::spawn(async move {
        let result = stream(command, id, tx.clone()).await;
        let _ = tx.send(Message::ConsoleDone(id, result)).await;
    }));
}
pub(super) fn key(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    engine: Arc<dyn Engine>,
    tx: &mpsc::Sender<Message>,
) -> bool {
    if app.input != Some(InputMode::Command) {
        return false;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('s') if ctrl => {
            app.dialogs.suggestions_hidden = !app.dialogs.suggestions_hidden;
            app.dialogs.suggestion_index = 0;
            return true;
        }
        KeyCode::Char('c') if ctrl => {
            if let Some(j) = app.dialogs.command_job.take() {
                j.abort();
                app.dialogs.command_id = app.dialogs.command_id.wrapping_add(1);
                append(
                    app,
                    "\n[Cancelled; Docker daemon operations may continue]\n",
                );
            }
            return true;
        }
        KeyCode::Char('o') if ctrl => {
            let mut c = std::process::Command::new(
                std::env::var_os("SHELL").unwrap_or_else(|| "/bin/sh".into()),
            );
            c.current_dir(cwd(app));
            let _ = tx.try_send(Message::Terminal(c));
            return true;
        }
        KeyCode::Char('a') if ctrl => {
            app.dialogs.command_cursor = 0;
            return true;
        }
        KeyCode::Char('e') if ctrl => {
            app.dialogs.command_cursor = app.command.len();
            return true;
        }
        KeyCode::Char('u') if ctrl => {
            app.command.drain(..app.dialogs.command_cursor);
            app.dialogs.command_cursor = 0;
            return true;
        }
        KeyCode::Char('j') if ctrl => {
            app.dialogs.help_scroll = app.dialogs.help_scroll.saturating_add(3);
            return true;
        }
        KeyCode::Char('k') if ctrl => {
            app.dialogs.help_scroll = app.dialogs.help_scroll.saturating_sub(3);
            return true;
        }
        KeyCode::PageUp => {
            app.dialogs.follow_output = false;
            app.dialogs.output_scroll = app.dialogs.output_scroll.saturating_sub(10);
            return true;
        }
        KeyCode::PageDown => {
            app.dialogs.follow_output = false;
            app.dialogs.output_scroll = app.dialogs.output_scroll.saturating_add(10);
            return true;
        }
        KeyCode::Esc => {
            app.input = None;
            return true;
        }
        _ => {}
    }
    let (choices, _) = suggestions(app);
    match key.code {
        KeyCode::Down if !choices.is_empty() => {
            app.dialogs.suggestion_index = (app.dialogs.suggestion_index + 1) % choices.len()
        }
        KeyCode::Up if !choices.is_empty() => {
            app.dialogs.suggestion_index =
                (app.dialogs.suggestion_index + choices.len() - 1) % choices.len()
        }
        KeyCode::Enter | KeyCode::Tab if !choices.is_empty() => {
            app.command = choices[app.dialogs.suggestion_index.min(choices.len() - 1)].clone();
            app.dialogs.command_cursor = app.command.len();
            app.dialogs.suggestion_index = 0;
        }
        KeyCode::Up | KeyCode::Down => {
            if !app.dialogs.history.is_empty() {
                let len = app.dialogs.history.len();
                let index = if key.code == KeyCode::Up {
                    app.dialogs.history_index.unwrap_or(len).saturating_sub(1)
                } else {
                    (app.dialogs.history_index.unwrap_or(len) + 1).min(len)
                };
                app.dialogs.history_index = Some(index);
                app.command = app.dialogs.history.get(index).cloned().unwrap_or_default();
                app.dialogs.command_cursor = app.command.len();
            }
        }
        KeyCode::Enter => execute(app, engine, tx),
        KeyCode::Char(c) if !ctrl => {
            insert(
                &mut app.command,
                &mut app.dialogs.command_cursor,
                &c.to_string(),
            );
            app.dialogs.suggestion_index = 0;
        }
        code => {
            edit(&mut app.command, &mut app.dialogs.command_cursor, code);
            app.dialogs.suggestion_index = 0;
        }
    }
    true
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_edit_and_paste_do_not_execute() {
        let mut s = "echo café".to_string();
        let mut c = s.len();
        edit(&mut s, &mut c, KeyCode::Left);
        edit(&mut s, &mut c, KeyCode::Delete);
        assert_eq!(s, "echo caf");
        insert(&mut s, &mut c, "é\nls");
        assert_eq!(s, "echo caféls");
        edit(&mut s, &mut c, KeyCode::Home);
        insert(&mut s, &mut c, "x");
        assert_eq!(s, "xecho caféls");
    }
    #[test]
    fn files_only_in_file_slots() {
        let t = tempfile::tempdir().unwrap();
        std::fs::write(t.path().join("compose.yaml"), "").unwrap();
        assert_eq!(
            file_suggestions("docker compose -f ", t.path()),
            ["docker compose -f compose.yaml "]
        );
        assert!(file_suggestions("docker exec ", t.path()).is_empty());
        std::fs::write(t.path().join("Dockerfile"), "FROM scratch").unwrap();
        assert_eq!(
            file_suggestions("docker build -f ", t.path()),
            ["docker build -f Dockerfile "]
        );
        assert_eq!(
            file_suggestions("docker build --file Doc", t.path()),
            ["docker build --file Dockerfile "]
        );
    }
}
