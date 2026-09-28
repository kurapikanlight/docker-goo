use crate::config::Endpoint;
use anyhow::{bail, Context, Result};
use std::{process::Stdio, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

pub const GUIDE:&str="Docker command help\n\nType docker followed by a command:\n\nps -a              All containers\nimages             Local images\nrun --help         Create and run\nlogs NAME          Container output\ninspect NAME       Resource details\npull IMAGE         Download image\nvolume ls          Volumes\nnetwork ls         Networks\ncompose --help     Compose commands\n\nChoose a suggestion to insert it before execution.\n\nUse b or docker exec -it for container terminals.\nExplicit Docker commands use the selected engine.\nHost commands run locally in the displayed directory.\nCtrl+O opens your native host shell.";

pub fn topic(line: &str) -> String {
    let words = line.split_whitespace().collect::<Vec<_>>();
    let start = usize::from(words.first() == Some(&"docker"));
    let Some(first) = words.get(start) else {
        return "docker --help".into();
    };
    let known = [
        "run",
        "create",
        "ps",
        "images",
        "logs",
        "inspect",
        "pull",
        "exec",
        "start",
        "stop",
        "restart",
        "rm",
        "rmi",
        "stats",
        "build",
        "info",
        "version",
        "container",
        "image",
        "volume",
        "network",
        "compose",
        "system",
    ];
    if !known.contains(first) {
        return "docker --help".into();
    }
    let groups = [
        "container",
        "image",
        "volume",
        "network",
        "compose",
        "system",
    ];
    let sub = words.get(start + 1).filter(|s| {
        [
            "ls", "inspect", "create", "rm", "prune", "up", "down", "logs", "ps", "start", "stop",
            "exec", "run", "build",
        ]
        .contains(s)
    });
    if groups.contains(first) {
        if let Some(sub) = sub {
            return format!("docker {first} {sub} --help");
        }
    }
    format!("docker {first} --help")
}
pub fn arguments(line: &str) -> Result<Vec<String>> {
    let mut args = shell_words::split(line).context("Unclosed quote in command")?;
    if args.first().map(String::as_str) == Some("docker") {
        args.remove(0);
    }
    if args == ["man", "docker"] || args.is_empty() {
        args = vec!["--help".into()];
    }
    if args.first().is_some_and(|s| {
        s.starts_with('-') && !["--help", "-h", "--version", "-v"].contains(&s.as_str())
    }) {
        bail!("Start with a Docker command. Engine/context flags are managed by Docker-Goo.");
    }
    if args
        .iter()
        .any(|a| matches!(a.as_str(), "|" | "||" | "&&" | ";" | ">" | "<"))
    {
        bail!("Use one Docker command; shell pipes and redirects are not supported.");
    }
    if args
        .first()
        .is_some_and(|s| matches!(s.as_str(), "login" | "logout"))
    {
        bail!("Docker account commands are outside this application.");
    }
    if !args.iter().any(|s| s == "--help" || s == "-h")
        && args
            .iter()
            .any(|s| matches!(s.as_str(), "-it" | "-ti" | "--interactive" | "-i"))
    {
        bail!("Use b for an interactive container terminal.");
    }
    Ok(args)
}
async fn capture(mut stream: impl AsyncRead + Unpin) -> Result<String> {
    let mut saved = Vec::new();
    let mut buf = [0u8; 4096];
    let mut truncated = false;
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let keep = n.min(131072usize.saturating_sub(saved.len()));
        saved.extend_from_slice(&buf[..keep]);
        truncated |= keep < n;
    }
    let mut text = String::from_utf8_lossy(&saved).into_owned();
    if truncated {
        text.push_str("\n[Output truncated at 128 KiB]");
    }
    Ok(text)
}
pub async fn execute(endpoint: &Endpoint, line: &str) -> Result<String> {
    let args = arguments(line)?;
    let timeout = if args
        .first()
        .is_some_and(|s| matches!(s.as_str(), "pull" | "build"))
    {
        600
    } else {
        60
    };
    let (out, err) = run_cli(endpoint, args, timeout).await?;
    Ok(format!("{out}{err}"))
}
pub fn docker_cli(endpoint: &Endpoint) -> std::process::Command {
    let mut command = std::process::Command::new("docker");
    command
        .arg("--host")
        .arg(&endpoint.host)
        .env_remove("DOCKER_CONTEXT")
        .env_remove("DOCKER_HOST")
        .env_remove("DOCKER_TLS_VERIFY")
        .env_remove("DOCKER_TLS")
        .env_remove("DOCKER_CERT_PATH");
    if let Some(dir) = &endpoint.certificates {
        command
            .arg("--tlsverify")
            .arg("--tlscacert")
            .arg(dir.join("ca.pem"))
            .arg("--tlscert")
            .arg(dir.join("cert.pem"))
            .arg("--tlskey")
            .arg(dir.join("key.pem"));
    }
    command
}
async fn run_cli(endpoint: &Endpoint, args: Vec<String>, timeout: u64) -> Result<(String, String)> {
    let mut command = Command::from(docker_cli(endpoint));
    let mut child = command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context(
            "Install the Docker CLI to use the $ console; API controls still work without it",
        )?;
    let stdout = child.stdout.take().context("No stdout")?;
    let stderr = child.stderr.take().context("No stderr")?;
    let result = tokio::time::timeout(Duration::from_secs(timeout), async {
        let (out, err, status) = tokio::try_join!(capture(stdout), capture(stderr), async {
            Ok::<_, anyhow::Error>(child.wait().await?)
        })?;
        Ok::<_, anyhow::Error>((out, err, status))
    })
    .await;
    match result {
        Ok(value) => {
            let (out, err, status) = value?;
            anyhow::ensure!(
                status.success(),
                "{out}{err}\n[exit {}]",
                status.code().unwrap_or(-1)
            );
            Ok((out, err))
        }
        Err(_) => {
            let _ = child.kill().await;
            bail!("Command stopped after {timeout} seconds. Check resource state; daemon operations may continue.");
        }
    }
}
pub async fn compose_up(endpoint: &Endpoint, path: &str) -> Result<String> {
    let expanded = if let Some(rest) = path.trim().strip_prefix("~/") {
        std::path::PathBuf::from(std::env::var("HOME").context("HOME unavailable")?).join(rest)
    } else {
        path.trim().into()
    };
    let file = tokio::fs::canonicalize(expanded)
        .await
        .context("Choose an existing Compose YAML file")?;
    anyhow::ensure!(
        tokio::fs::metadata(&file).await?.is_file(),
        "Compose path must be a file"
    );
    anyhow::ensure!(
        matches!(
            file.extension().and_then(|x| x.to_str()),
            Some("yaml" | "yml")
        ),
        "Choose a .yaml or .yml file"
    );
    let prefix = vec![
        "compose".to_string(),
        "--ansi".into(),
        "never".into(),
        "-f".into(),
        file.to_string_lossy().into_owned(),
    ];
    let mut args = prefix.clone();
    args.extend(["config".into(), "--format".into(), "json".into()]);
    let (config, _) = run_cli(endpoint, args, 60)
        .await
        .context("Compose validation failed")?;
    let value: serde_json::Value =
        serde_json::from_str(&config).context("Cannot read Compose project configuration")?;
    let project = value
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .context("Compose did not return a project name; update the Docker Compose plugin")?
        .to_string();
    let mut args = prefix;
    args.extend(["-p".into(), project.clone(), "up".into(), "-d".into()]);
    run_cli(endpoint, args, 300)
        .await
        .context("Compose up failed")?;
    Ok(project)
}

pub fn suggestions(line: &str) -> Vec<String> {
    let words = line.split_whitespace().collect::<Vec<_>>();
    let trailing = line.ends_with(char::is_whitespace);
    let mut tokens = words.clone();
    if trailing {
        tokens.push("");
    }
    if tokens.is_empty() {
        return vec!["docker ".into()];
    }
    if tokens.len() == 1 && "docker".starts_with(tokens[0]) {
        return vec!["docker ".into()];
    }
    let offset = usize::from(tokens.first() == Some(&"docker"));
    let candidates: &[&str] = if tokens.len() == offset + 1 {
        &[
            "build",
            "compose",
            "container",
            "create",
            "exec",
            "image",
            "images",
            "info",
            "inspect",
            "kill",
            "logs",
            "network",
            "pause",
            "ps",
            "pull",
            "push",
            "rename",
            "restart",
            "rm",
            "rmi",
            "run",
            "start",
            "stats",
            "stop",
            "system",
            "tag",
            "unpause",
            "version",
            "volume",
            "wait",
        ]
    } else if tokens.len() == offset + 2 {
        match tokens[offset] {
            "container" => &[
                "create", "exec", "inspect", "kill", "logs", "ls", "pause", "prune", "rename",
                "restart", "rm", "run", "start", "stats", "stop", "unpause",
            ],
            "image" => &[
                "build", "history", "inspect", "ls", "prune", "pull", "rm", "tag",
            ],
            "volume" => &["create", "inspect", "ls", "prune", "rm"],
            "network" => &[
                "connect",
                "create",
                "disconnect",
                "inspect",
                "ls",
                "prune",
                "rm",
            ],
            "compose" => &[
                "build", "config", "down", "exec", "logs", "ls", "ps", "pull", "restart", "run",
                "start", "stop", "up",
            ],
            "system" => &["df", "events", "info", "prune"],
            _ => &[],
        }
    } else {
        &[]
    };
    let prefix = tokens.last().copied().unwrap_or("");
    let base = tokens[..tokens.len() - 1].join(" ");
    candidates
        .iter()
        .filter(|s| s.starts_with(prefix))
        .map(|s| {
            if base.is_empty() {
                format!("{s} ")
            } else {
                format!("{base} {s} ")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsing_preserves_case_quotes_and_engine_selection() {
        assert_eq!(suggestions("docker pu"), ["docker pull ", "docker push "]);
        assert_eq!(suggestions("docker volume cr"), ["docker volume create "]);
        assert!(suggestions("docker pull ubuntu").is_empty());
        assert!(suggestions("docker pull ").is_empty());
        assert!(suggestions("docker container exec web ").is_empty());
        assert_eq!(
            arguments("docker inspect 'My Container'").unwrap(),
            ["inspect", "My Container"]
        );
        assert!(arguments("docker --host tcp://other ps").is_err());
        assert!(arguments("docker ps | head").is_err());
        assert_eq!(
            topic("docker container ls --all"),
            "docker container ls --help"
        );
    }
}

/// Complete resource arguments from the selected engine's current snapshot.
/// Unknown options are left alone rather than guessing which argument they consume.
pub fn resource_suggestions(line: &str, snapshot: &crate::core::Snapshot) -> Vec<String> {
    let split = line.rfind(char::is_whitespace).map_or(0, |i| i + 1);
    let (base, prefix) = line.split_at(split);
    if prefix.starts_with('-') {
        return vec![];
    }
    let Ok(mut words) = shell_words::split(base) else {
        return vec![];
    };
    if words.first().is_some_and(|w| w == "docker") {
        words.remove(0);
    }
    if words.is_empty() {
        return vec![];
    }
    let group = if matches!(words[0].as_str(), "container" | "image") {
        words.remove(0)
    } else {
        String::new()
    };
    if words.is_empty() {
        return vec![];
    }
    let command = words.remove(0);
    let images = matches!(
        command.as_str(),
        "run" | "create" | "rmi" | "tag" | "history" | "push" | "pull"
    ) || (group == "image" && matches!(command.as_str(), "rm" | "inspect"));
    let containers = !images
        && group != "image"
        && matches!(
            command.as_str(),
            "start"
                | "stop"
                | "restart"
                | "pause"
                | "unpause"
                | "kill"
                | "rm"
                | "logs"
                | "exec"
                | "attach"
                | "stats"
                | "wait"
                | "rename"
                | "inspect"
                | "commit"
                | "export"
                | "diff"
                | "top"
        );
    if !images && !containers {
        return vec![];
    }
    let mut positional = Vec::new();
    let mut args = words.iter();
    while let Some(arg) = args.next() {
        if arg == "--" {
            positional.extend(args);
            break;
        }
        if !arg.starts_with('-') {
            positional.push(arg);
            continue;
        }
        let flag = arg.split('=').next().unwrap_or(arg);
        let takes_value = matches!(
            flag,
            "--name"
                | "--network"
                | "--hostname"
                | "--entrypoint"
                | "--workdir"
                | "--user"
                | "--env"
                | "--env-file"
                | "--publish"
                | "--volume"
                | "--mount"
                | "--label"
                | "--restart"
                | "--platform"
                | "--memory"
                | "--cpus"
                | "--tail"
                | "--since"
                | "--until"
                | "--time"
                | "--signal"
                | "--format"
                | "--type"
                | "-p"
                | "-v"
                | "-e"
                | "-u"
                | "-w"
                | "-t"
        );
        // -t means TTY for run/create/exec, timeout for lifecycle commands.
        let tty = flag == "-t" && matches!(command.as_str(), "run" | "create" | "exec");
        if takes_value && !tty {
            if !arg.contains('=') && args.next().is_none() {
                return vec![];
            }
        } else if !tty
            && !matches!(
                flag,
                "-i" | "-it"
                    | "-ti"
                    | "-d"
                    | "-f"
                    | "-a"
                    | "--detach"
                    | "--interactive"
                    | "--tty"
                    | "--rm"
                    | "--force"
                    | "--follow"
                    | "--all"
                    | "--no-stream"
                    | "--timestamps"
                    | "--details"
                    | "--privileged"
            )
        {
            return vec![];
        }
    }
    let multiple = matches!(
        command.as_str(),
        "start"
            | "stop"
            | "restart"
            | "pause"
            | "unpause"
            | "kill"
            | "rm"
            | "rmi"
            | "inspect"
            | "stats"
            | "wait"
    );
    if !multiple && !positional.is_empty() {
        return vec![];
    }
    let mut names: Vec<String> = if images {
        snapshot
            .images
            .iter()
            .flat_map(|r| {
                let tags: Vec<_> = r
                    .name
                    .split(", ")
                    .filter(|s| !s.is_empty() && *s != "<none>:<none>")
                    .map(str::to_owned)
                    .collect();
                if tags.is_empty() {
                    vec![r.id.clone()]
                } else {
                    tags
                }
            })
            .collect()
    } else {
        snapshot
            .containers
            .iter()
            .map(|r| r.name.trim_start_matches('/').to_owned())
            .collect()
    };
    names.sort();
    names.dedup();
    names
        .into_iter()
        .filter(|name| name.starts_with(prefix) && !positional.contains(&name))
        .map(|name| format!("{base}{} ", shell_words::quote(&name)))
        .collect()
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use crate::core::{Resource, Snapshot};
    #[test]
    fn completes_resource_slots_without_overwriting_arguments() {
        let mut snapshot = Snapshot::default();
        snapshot.containers.push(Resource {
            id: "abc".into(),
            name: "web".into(),
            detail: String::new(),
            state: "running".into(),
            extra: String::new(),
        });
        snapshot.images.push(Resource {
            id: "sha256:123".into(),
            name: "nginx:alpine, nginx:latest".into(),
            detail: String::new(),
            state: String::new(),
            extra: String::new(),
        });
        assert_eq!(
            resource_suggestions("docker stop w", &snapshot),
            ["docker stop web "]
        );
        assert_eq!(
            resource_suggestions("docker container exec -it w", &snapshot),
            ["docker container exec -it web "]
        );
        assert_eq!(
            resource_suggestions("docker run -p 8080:80 nginx:a", &snapshot),
            ["docker run -p 8080:80 nginx:alpine "]
        );
        assert!(resource_suggestions("docker run --name ", &snapshot).is_empty());
        assert!(resource_suggestions("docker exec web ", &snapshot).is_empty());
        assert!(resource_suggestions("docker run nginx:alpine ", &snapshot).is_empty());
        assert!(resource_suggestions("docker stop web ", &snapshot).is_empty());
        assert!(resource_suggestions("docker run --unknown ", &snapshot).is_empty());
    }
}
