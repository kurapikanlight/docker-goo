use anyhow::{bail, Context, Result};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    net::UnixListener,
    process::Command,
    task::{JoinHandle, JoinSet},
};

// One authenticated SSH subprocess per HTTP connection. No shell interpolation,
// no local listener exposed on TCP, and all subprocesses die with the proxy.
pub struct Proxy {
    pub socket: PathBuf,
    task: JoinHandle<()>,
    _dir: tempfile::TempDir,
}
impl Proxy {
    pub async fn start(host: &str) -> Result<Self> {
        let url = url::Url::parse(host)?;
        if url.password().is_some()
            || !matches!(url.path(), "" | "/")
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("SSH endpoints must use ssh://[user@]host[:port]");
        }
        let hostname = url.host_str().context("Missing SSH hostname")?.to_string();
        let user = url.username().to_string();
        if hostname.starts_with('-') || user.starts_with('-') {
            bail!("Invalid SSH destination");
        }
        let port = url.port().unwrap_or(22).to_string();
        let dir = tempfile::tempdir()?;
        let socket = dir.path().join("docker.sock");
        let listener = UnixListener::bind(&socket)?;
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((stream, _)) = accepted else { break; };
                        let (hostname,user,port) = (hostname.clone(),user.clone(),port.clone());
                        connections.spawn(async move {
                            let mut cmd = Command::new("ssh");
                            cmd.args(["-T", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8", "-p", &port]);
                            if !user.is_empty() { cmd.args(["-l", &user]); }
                            let Ok(mut child) = cmd.arg("--").arg(hostname).args(["docker", "system", "dial-stdio"])
                                .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).kill_on_drop(true).spawn() else { return; };
                            let (mut read, mut write) = stream.into_split();
                            let (Some(mut input), Some(mut output)) = (child.stdin.take(),child.stdout.take()) else { return; };
                            tokio::select! {
                                _ = tokio::io::copy(&mut read, &mut input) => {},
                                _ = tokio::io::copy(&mut output, &mut write) => {},
                            }
                            let _ = child.kill().await;
                        });
                    }
                    _ = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Ok(Self {
            socket,
            task,
            _dir: dir,
        })
    }
}
impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}
