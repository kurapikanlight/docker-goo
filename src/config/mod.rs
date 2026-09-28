use anyhow::{bail, Context, Result};
use clap::Parser;
use serde::Deserialize;
use std::{env, fs, path::PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "docker-goo",
    version,
    about = "Docker Engine, in your terminal"
)]
pub struct Args {
    /// Override Docker endpoint (unix://, tcp://, https://, ssh://)
    #[arg(long, conflicts_with = "context")]
    pub host: Option<String>,
    /// Use a named Docker context
    #[arg(long)]
    pub context: Option<String>,
    /// Executable inside the container, e.g. /bin/bash
    #[arg(long, default_value = "auto")]
    pub shell: String,
    /// Print daemon summary without opening the TUI
    #[arg(long)]
    pub check: bool,
    /// Open directly in the $ console, inheriting the host working directory
    #[arg(long)]
    pub console: bool,
    /// Initial local working directory for the console
    #[arg(long)]
    pub cwd: Option<PathBuf>,
}
#[derive(Clone, Debug)]
pub struct Endpoint {
    pub host: String,
    pub certificates: Option<PathBuf>,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DockerConfig {
    current_context: Option<String>,
}
#[derive(Deserialize)]
struct ContextFile {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Endpoints")]
    endpoints: ContextEndpoints,
}
#[derive(Deserialize)]
struct ContextEndpoints {
    docker: ContextDocker,
}
#[derive(Deserialize)]
struct ContextDocker {
    #[serde(rename = "Host")]
    host: String,
    #[serde(rename = "SkipTLSVerify", default)]
    skip_tls_verify: bool,
}
fn nonempty(name: &str) -> Option<String> {
    env::var(name).ok().filter(|s| !s.is_empty())
}

pub fn resolve(args: &Args) -> Result<Endpoint> {
    let root = nonempty("DOCKER_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(nonempty("HOME").unwrap_or_default()).join(".docker"));
    let context = args.context.clone().or_else(|| nonempty("DOCKER_CONTEXT"));
    if let Some(host) = &args.host {
        return environment_endpoint(host.clone());
    }
    if let Some(context) = context {
        return from_context(&root, &context);
    }
    if let Some(host) = nonempty("DOCKER_HOST") {
        return environment_endpoint(host);
    }
    let config = root.join("config.json");
    if config.exists() {
        let config: DockerConfig =
            serde_json::from_slice(&fs::read(&config)?).context("Invalid Docker config.json")?;
        if let Some(name) = config
            .current_context
            .filter(|n| n != "default" && !n.is_empty())
        {
            return from_context(&root, &name);
        }
    }
    let runtime = nonempty("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    let rootless = runtime.join("docker.sock");
    Ok(Endpoint {
        host: format!(
            "unix://{}",
            if rootless.exists() {
                rootless
            } else {
                PathBuf::from("/var/run/docker.sock")
            }
            .display()
        ),
        certificates: None,
    })
}
fn environment_endpoint(host: String) -> Result<Endpoint> {
    let tls = nonempty("DOCKER_TLS_VERIFY").is_some() || host.starts_with("https://");
    let certificates = if tls {
        Some(
            nonempty("DOCKER_CERT_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(nonempty("HOME").unwrap_or_default()).join(".docker")
                }),
        )
    } else {
        None
    };
    Ok(Endpoint { host, certificates })
}
fn from_context(root: &std::path::Path, name: &str) -> Result<Endpoint> {
    if name == "default" {
        return Ok(Endpoint {
            host: "unix:///var/run/docker.sock".into(),
            certificates: None,
        });
    }
    for entry in fs::read_dir(root.join("contexts/meta"))
        .with_context(|| format!("Cannot load Docker context {name:?}"))?
    {
        let entry = entry?;
        let path = entry.path().join("meta.json");
        if !path.is_file() {
            continue;
        }
        let context: ContextFile =
            serde_json::from_slice(&fs::read(path)?).context("Invalid Docker context metadata")?;
        if context.name != name {
            continue;
        }
        let certs = root
            .join("contexts/tls")
            .join(entry.file_name())
            .join("docker");
        if context.endpoints.docker.skip_tls_verify {
            bail!("Context {name:?} disables TLS verification; configure a trusted CA instead");
        }
        return Ok(Endpoint {
            host: context.endpoints.docker.host,
            certificates: certs.join("ca.pem").exists().then_some(certs),
        });
    }
    bail!("Docker context {name:?} not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_context_without_docker_cli() {
        let dir = tempfile::tempdir().unwrap();
        let meta = dir.path().join("contexts/meta/abc");
        fs::create_dir_all(&meta).unwrap();
        fs::write(meta.join("meta.json"), r#"{"Name":"rootless","Endpoints":{"docker":{"Host":"unix:///run/user/1000/docker.sock","SkipTLSVerify":false}}}"#).unwrap();
        assert_eq!(
            from_context(dir.path(), "rootless").unwrap().host,
            "unix:///run/user/1000/docker.sock"
        );
        assert!(from_context(dir.path(), "missing").is_err());
    }
}
