//! Read clipboard text only after an explicit paste gesture. No polling.
use super::*;
use tokio::io::AsyncReadExt;

async fn read_command(program: &str, args: &[&str]) -> Result<String> {
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .unwrap()
        .take(16_385)
        .read_to_end(&mut bytes)
        .await?;
    anyhow::ensure!(bytes.len() <= 16_384, "Clipboard text exceeds 16 KiB");
    anyhow::ensure!(child.wait().await?.success(), "Clipboard reader failed");
    Ok(String::from_utf8(bytes)?)
}
async fn read() -> Result<String> {
    let mut candidates: Vec<(&str, Vec<&str>)> = Vec::new();
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        candidates.push(("wl-paste", vec!["--no-newline", "--type", "text"]));
    }
    if std::env::var_os("DISPLAY").is_some() {
        candidates.push(("xclip", vec!["-selection", "clipboard", "-out"]));
        candidates.push(("xsel", vec!["--clipboard", "--output"]));
    }
    for (program, args) in candidates {
        if let Ok(Ok(value)) =
            tokio::time::timeout(Duration::from_secs(2), read_command(program, &args)).await
        {
            return Ok(value);
        }
    }
    anyhow::bail!("Clipboard unavailable. Install wl-clipboard (Wayland) or xclip (X11), or use your terminal's Paste shortcut (usually Ctrl+Shift+V).")
}
fn editable(app: &App) -> bool {
    app.input == Some(InputMode::Command)
        || (!app.busy
            && (app.dialogs.picker
                || app.dialogs.form.as_ref().is_some_and(|f| f.focus < 7)
                || app.dialogs.compose_path.is_some()
                || app.dialogs.shell.is_some()
                || app.input == Some(InputMode::Search)))
}
pub(super) fn request(
    app: &mut App,
    event: &Event,
    tx: &mpsc::Sender<Message>,
    jobs: &mut Jobs,
) -> bool {
    let requested = match event {
        Event::Mouse(m) if m.kind == MouseEventKind::Down(crossterm::event::MouseButton::Right) => {
            // Paste into the clicked form field without activating its image picker.
            if !app.dialogs.picker && !app.busy {
                if let Some(i) = app
                    .dialogs
                    .form_areas
                    .iter()
                    .position(|r| r.contains((m.column, m.row).into()))
                {
                    if let Some(form) = app.dialogs.form.as_mut() {
                        form.focus = i;
                        form.cursors[i] = form.fields[i].len();
                    }
                } else if app.dialogs.form.is_some() {
                    return true;
                }
            }
            true
        }
        Event::Key(k) if k.kind != KeyEventKind::Release => {
            (k.code == KeyCode::Char('v') && k.modifiers.contains(KeyModifiers::CONTROL))
                || (k.code == KeyCode::Insert && k.modifiers.contains(KeyModifiers::SHIFT))
        }
        _ => false,
    };
    if !requested || !editable(app) {
        return false;
    }
    let epoch = app.clipboard_epoch;
    let tx = tx.clone();
    jobs.0.push(tokio::spawn(async move {
        let result = read().await;
        let _ = tx.send(Message::Clipboard(epoch, result)).await;
    }));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn reader_preserves_text_and_rejects_failed_process() {
        assert_eq!(
            read_command("/bin/sh", &["-c", "printf 'hello world'"])
                .await
                .unwrap(),
            "hello world"
        );
        assert!(read_command("/bin/sh", &["-c", "exit 1"]).await.is_err());
    }
    #[test]
    fn create_button_is_not_an_editable_paste_target() {
        let mut app = App::new();
        let mut form = dialogs::Form::new("alpine".into());
        form.focus = 7;
        app.dialogs.form = Some(form);
        assert!(!editable(&app));
        app.dialogs.form.as_mut().unwrap().focus = 1;
        assert!(editable(&app));
        app.busy = true;
        assert!(!editable(&app));
    }
}
