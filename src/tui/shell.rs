use super::{Engine, Screen, ShellSession};
use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, Event, EventStream, KeyCode, KeyEvent,
        KeyEventKind, KeyModifiers,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::StreamExt;
use std::{io, sync::Arc};
use tokio::io::AsyncWriteExt;

pub(super) async fn run(
    terminal: &mut Screen,
    events: &mut EventStream,
    engine: Arc<dyn Engine>,
    mut session: ShellSession,
) -> Result<()> {
    execute!(
        io::stdout(),
        DisableMouseCapture,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    let result = async {
        let mut stdout = tokio::io::stdout();
        stdout.write_all(b"\r\nDocker-Goo shell | exit closes shell | Ctrl+D detaches\r\n").await?;
        stdout.flush().await?;
        loop {
            tokio::select! {
                output = session.output.next() => {
                    match output {
                        Some(data) => { stdout.write_all(&data?).await?; stdout.flush().await?; }
                        None => break,
                    }
                }
                event = events.next() => {
                    match event {
                        Some(Ok(Event::Key(key))) if key.kind != KeyEventKind::Release => {
                            if is_detach(key) { break; }
                            let bytes = encode_key(key);
                            if !bytes.is_empty() { session.input.write_all(&bytes).await?; session.input.flush().await?; }
                        }
                        Some(Ok(Event::Resize(cols,rows))) => { engine.resize_shell(&session.id, cols, rows).await?; }
                        Some(Ok(Event::Paste(text))) => { session.input.write_all(text.as_bytes()).await?; session.input.flush().await?; }
                        Some(Err(error)) => return Err(error.into()),
                        None => break,
                        _ => {}
                    }
                }
            }
        }
        Ok(())
    }.await;
    // Restore the TUI even when exec, stdin or stdout fails.
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    terminal.clear()?;
    result
}
fn is_detach(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL)
}
fn encode_key(key: KeyEvent) -> Vec<u8> {
    let mut bytes = match key.code {
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) && c.is_ascii() => {
            vec![(c.to_ascii_uppercase() as u8) & 0x1f]
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => vec![b'\r'],
        KeyCode::Backspace => vec![127],
        KeyCode::Tab => vec![b'\t'],
        KeyCode::Esc => vec![27],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        KeyCode::Insert => b"\x1b[2~".to_vec(),
        KeyCode::PageUp => b"\x1b[5~".to_vec(),
        KeyCode::PageDown => b"\x1b[6~".to_vec(),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::F(n @ 1..=4) => vec![27, b'O', b'P' + n - 1],
        KeyCode::F(n @ 5..=12) => format!(
            "\x1b[{}~",
            [15, 17, 18, 19, 20, 21, 23, 24][(n - 5) as usize]
        )
        .into_bytes(),
        _ => Vec::new(),
    };
    if key.modifiers.contains(KeyModifiers::ALT) && !bytes.is_empty() {
        bytes.insert(0, 27);
    }
    bytes
}
pub(super) async fn host(terminal: &mut Screen, command: std::process::Command) -> Result<()> {
    use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
    execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    )?;
    crossterm::terminal::disable_raw_mode()?;
    // Catch SIGINT in the TUI parent while the foreground child receives it normally.
    let _sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = tokio::process::Command::from(command)
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .kill_on_drop(true)
        .status()
        .await;
    crossterm::terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    terminal.clear()?;
    anyhow::ensure!(result?.success(), "Terminal command exited with an error");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwards_unicode_and_control_keys() {
        assert!(is_detach(KeyEvent::new(
            KeyCode::Char('d'),
            KeyModifiers::CONTROL
        )));
        assert!(!is_detach(KeyEvent::new(
            KeyCode::Char(']'),
            KeyModifiers::CONTROL
        )));
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            vec![3]
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Char('é'), KeyModifiers::NONE)),
            "é".as_bytes()
        );
        assert_eq!(
            encode_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE)),
            b"\x1b[A"
        );
    }
}

// Use the terminal the user already launched us in: no desktop/emulator dependency.
