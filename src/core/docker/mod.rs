#[cfg(test)]
#[path = "tests.rs"]
mod api_tests;
mod create;
mod ssh;
mod web;

use super::*;
use crate::config::Endpoint;
use anyhow::{bail, Context};
use bollard::{
    container::*, exec::*, image::ListImagesOptions, network::ListNetworksOptions,
    volume::ListVolumesOptions, Docker, API_DEFAULT_VERSION,
};
use futures_util::StreamExt;
use std::collections::BTreeMap;
use tokio::sync::{Mutex, OnceCell};

pub struct DockerEngine {
    endpoint: Endpoint,
    connection: OnceCell<Connection>,
    negotiated: Mutex<bool>,
}
struct Connection {
    client: Docker,
    _ssh: Option<ssh::Proxy>,
}
impl DockerEngine {
    // Defer IO to snapshot so an absent/stopped daemon still opens the TUI.
    pub async fn connect(endpoint: Endpoint) -> Result<Self> {
        Ok(Self {
            endpoint,
            connection: OnceCell::new(),
            negotiated: Mutex::new(false),
        })
    }
    async fn client(&self) -> Result<&Docker> {
        Ok(&self
            .connection
            .get_or_try_init(|| Connection::open(self.endpoint.clone()))
            .await?
            .client)
    }
}
impl Connection {
    async fn open(endpoint: Endpoint) -> Result<Self> {
        let mut host = endpoint.host.clone();
        let proxy = if host.starts_with("ssh://") {
            let proxy = ssh::Proxy::start(&host).await?;
            host = format!("unix://{}", proxy.socket.display());
            Some(proxy)
        } else {
            None
        };
        let client = if let Some(certs) = endpoint.certificates {
            Docker::connect_with_ssl(
                &host.replace("tcp://", "https://"),
                &certs.join("key.pem"),
                &certs.join("cert.pem"),
                &certs.join("ca.pem"),
                10,
                API_DEFAULT_VERSION,
            )?
        } else if host.starts_with("unix://") {
            Docker::connect_with_unix(&host, 10, API_DEFAULT_VERSION)?
        } else if host.starts_with("tcp://") || host.starts_with("http://") {
            Docker::connect_with_http(&host.replace("tcp://", "http://"), 10, API_DEFAULT_VERSION)?
        } else {
            bail!("Unsupported Docker endpoint: {host}");
        };
        Ok(Self {
            client,
            _ssh: proxy,
        })
    }
}
#[async_trait]
impl Engine for DockerEngine {
    fn endpoint(&self) -> &str {
        &self.endpoint.host
    }
    async fn snapshot(&self) -> Result<Snapshot> {
        let client = self.client().await?;
        {
            let mut ready = self.negotiated.lock().await;
            if !*ready {
                client.clone().negotiate_version().await.context(
                    "Docker Engine unavailable. Check endpoint, daemon and socket permissions",
                )?;
                *ready = true;
            }
        }
        let (info, containers, images, volumes, networks) = tokio::try_join!(
            client.info(),
            client.list_containers(Some(ListContainersOptions::<String> {
                all: true,
                ..Default::default()
            })),
            client.list_images(Some(ListImagesOptions::<String> {
                all: true,
                ..Default::default()
            })),
            client.list_volumes(None::<ListVolumesOptions<String>>),
            client.list_networks(None::<ListNetworksOptions<String>>),
        )?;
        let mut projects = BTreeMap::<String, (usize, usize)>::new();
        let mut members = std::collections::HashMap::new();
        let mut snapshot = Snapshot {
            compose_members: Default::default(),
            engine: format!(
                "{} | Docker {} | {} CPUs | {:.1} GiB",
                info.name.unwrap_or_default(),
                info.server_version.unwrap_or_default(),
                info.ncpu.unwrap_or_default(),
                info.mem_total.unwrap_or_default() as f64 / 1073741824.0
            ),
            containers: containers
                .into_iter()
                .map(|c| {
                    let state = c.state.unwrap_or_default();
                    if let Some(project) = c
                        .labels
                        .as_ref()
                        .and_then(|l| l.get("com.docker.compose.project"))
                    {
                        members.insert(c.id.clone().unwrap_or_default(), project.clone());
                        let counts = projects.entry(project.clone()).or_default();
                        counts.0 += 1;
                        counts.1 += usize::from(state == "running");
                    }
                    let ports = c
                        .ports
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| match p.public_port {
                            Some(public) => format!(
                                "{}:{}->{}",
                                p.ip.unwrap_or_else(|| "0.0.0.0".into()),
                                public,
                                p.private_port
                            ),
                            None => format!("{}", p.private_port),
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    Resource {
                        id: c.id.unwrap_or_default(),
                        name: c
                            .names
                            .unwrap_or_default()
                            .join(", ")
                            .trim_start_matches('/')
                            .into(),
                        detail: c.image.unwrap_or_default(),
                        state,
                        extra: ports,
                    }
                })
                .collect(),
            images: images
                .into_iter()
                .map(|i| Resource {
                    name: i.repo_tags.join(", "),
                    id: i.id,
                    detail: format!("{:.1} MiB", i.size as f64 / 1048576.0),
                    state: "image".into(),
                    extra: String::new(),
                })
                .collect(),
            volumes: volumes
                .volumes
                .unwrap_or_default()
                .into_iter()
                .map(|v| Resource {
                    id: v.name.clone(),
                    name: v.name,
                    detail: v.driver,
                    state: "volume".into(),
                    extra: v.mountpoint,
                })
                .collect(),
            networks: networks
                .into_iter()
                .map(|n| Resource {
                    id: n.id.unwrap_or_default(),
                    name: n.name.unwrap_or_default(),
                    detail: n.driver.unwrap_or_default(),
                    state: n.scope.unwrap_or_default(),
                    extra: n
                        .ipam
                        .and_then(|p| p.config)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|c| c.subnet)
                        .collect::<Vec<_>>()
                        .join(", "),
                })
                .collect(),
            compose: Vec::new(),
        };
        snapshot.compose_members = members;
        snapshot.compose = projects
            .into_iter()
            .map(|(name, (total, running))| Resource {
                id: name.clone(),
                name,
                detail: format!("{total} containers"),
                state: format!("{running} running"),
                extra: "Detected from Compose labels".into(),
            })
            .collect();
        for list in [
            &mut snapshot.containers,
            &mut snapshot.images,
            &mut snapshot.volumes,
            &mut snapshot.networks,
        ] {
            list.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        }
        Ok(snapshot)
    }
    async fn action(&self, id: &str, action: Action) -> Result<()> {
        let mut client = self.client().await?.clone();
        client.set_timeout(std::time::Duration::from_secs(30));
        match action {
            Action::Start => {
                client
                    .start_container(id, None::<StartContainerOptions<String>>)
                    .await?
            }
            Action::Stop => {
                client
                    .stop_container(id, Some(StopContainerOptions { t: 10 }))
                    .await?
            }
            Action::Restart => {
                client
                    .restart_container(id, Some(RestartContainerOptions { t: 10 }))
                    .await?
            }
            Action::Pause => client.pause_container(id).await?,
            Action::Resume => client.unpause_container(id).await?,
            // Never force deletion, never delete volumes implicitly.
            Action::Delete => {
                client
                    .remove_container(
                        id,
                        Some(RemoveContainerOptions {
                            force: false,
                            v: false,
                            link: false,
                        }),
                    )
                    .await?
            }
        }
        Ok(())
    }
    async fn logs(
        &self,
        id: &str,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<String>>> {
        let client = self.client().await?;
        Ok(client
            .logs(
                id,
                Some(LogsOptions::<String> {
                    follow: true,
                    stdout: true,
                    stderr: true,
                    tail: "200".into(),
                    timestamps: true,
                    ..Default::default()
                }),
            )
            .map(|item| item.map(|data| data.to_string()).map_err(Into::into))
            .boxed())
    }
    async fn stats(
        &self,
        id: &str,
    ) -> Result<futures_util::stream::BoxStream<'static, Result<Metrics>>> {
        let client = self.client().await?;
        Ok(client
            .stats(
                id,
                Some(StatsOptions {
                    stream: true,
                    one_shot: false,
                }),
            )
            .map(|item| item.map(|s| metrics(&s)).map_err(Into::into))
            .boxed())
    }
    async fn inspect(&self, kind: Kind, id: &str) -> Result<String> {
        let client = self.client().await?;
        let value = match kind {
            Kind::Containers => serde_json::to_value(client.inspect_container(id, None).await?)?,
            Kind::Images => serde_json::to_value(client.inspect_image(id).await?)?,
            Kind::Volumes => serde_json::to_value(client.inspect_volume(id).await?)?,
            Kind::Networks => serde_json::to_value(
                client
                    .inspect_network(id, None::<bollard::network::InspectNetworkOptions<String>>)
                    .await?,
            )?,
            Kind::Compose => {
                let mut filters = std::collections::HashMap::new();
                filters.insert(
                    "label".to_string(),
                    vec![format!("com.docker.compose.project={id}")],
                );
                serde_json::to_value(
                    client
                        .list_containers(Some(ListContainersOptions {
                            all: true,
                            filters,
                            ..Default::default()
                        }))
                        .await?,
                )?
            }
        };
        Ok(serde_json::to_string_pretty(&value)?)
    }
    async fn shell(&self, id: &str, command: &str, cols: u16, rows: u16) -> Result<ShellSession> {
        let client = self.client().await?;
        let info = client.inspect_container(id, None).await?;
        let running = info.state.as_ref().and_then(|s| s.running).unwrap_or(false);
        let paused = info.state.as_ref().and_then(|s| s.paused).unwrap_or(false);
        anyhow::ensure!(
            !paused,
            "Resume the paused container with u before opening its terminal"
        );
        if !running {
            let config = info.config.unwrap_or_default();
            anyhow::ensure!(config.tty==Some(true)&&config.open_stdin==Some(true),"This stopped container has no persistent terminal. Use n and the Shell preset to create one; existing data is not changed.");
            let attached = client
                .attach_container(
                    id,
                    Some(AttachContainerOptions::<String> {
                        stdin: Some(true),
                        stdout: Some(true),
                        stderr: Some(true),
                        stream: Some(true),
                        logs: Some(false),
                        detach_keys: None,
                    }),
                )
                .await?;
            client
                .start_container(id, None::<StartContainerOptions<String>>)
                .await?;
            return Ok(ShellSession {
                id: format!("container:{id}"),
                input: attached.input,
                output: attached
                    .output
                    .map(|item| item.map(|o| o.into_bytes().to_vec()).map_err(Into::into))
                    .boxed(),
            });
        }
        let exec = client
            .create_exec(
                id,
                CreateExecOptions {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    attach_stdin: Some(true),
                    tty: Some(true),
                    cmd: Some(if command == "auto" {
                        vec![
                            "/bin/sh".into(),
                            "-c".into(),
                            "if command -v bash >/dev/null 2>&1; then exec bash; else exec sh; fi"
                                .into(),
                        ]
                    } else {
                        shell_words::split(command).context("Invalid terminal command")?
                    }),
                    env: Some(vec![format!(
                        "TERM={}",
                        std::env::var("TERM").unwrap_or_else(|_| "xterm-256color".into())
                    )]),
                    ..Default::default()
                },
            )
            .await?;
        match client
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: true,
                    output_capacity: None,
                }),
            )
            .await?
        {
            StartExecResults::Attached { output, input } => {
                let _ = self.resize_shell(&exec.id, cols, rows).await;
                Ok(ShellSession {
                    id: exec.id,
                    input,
                    output: output
                        .map(|item| item.map(|o| o.into_bytes().to_vec()).map_err(Into::into))
                        .boxed(),
                })
            }
            _ => bail!("Docker did not attach the exec session"),
        }
    }
    async fn compose_up(&self, path: &str) -> Result<String> {
        crate::core::commands::compose_up(&self.endpoint, path).await
    }
    async fn create(&self, spec: RunSpec) -> Result<String> {
        create::run(self, spec).await
    }
    async fn web_urls(&self, id: &str) -> Result<Vec<String>> {
        let info = self.client().await?.inspect_container(id, None).await?;
        let ports = info
            .network_settings
            .and_then(|n| n.ports)
            .unwrap_or_default();
        web::urls(&ports, &self.endpoint.host)
    }
    fn docker_cli(&self) -> std::process::Command {
        crate::core::commands::docker_cli(&self.endpoint)
    }
    async fn docker_command(&self, command: &str) -> Result<String> {
        crate::core::commands::execute(&self.endpoint, command).await
    }
    async fn resize_shell(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let client = self.client().await?;
        if let Some(container) = id.strip_prefix("container:") {
            client
                .resize_container_tty(
                    container,
                    ResizeContainerTtyOptions {
                        width: cols,
                        height: rows,
                    },
                )
                .await?;
            return Ok(());
        }
        client
            .resize_exec(
                id,
                ResizeExecOptions {
                    width: cols,
                    height: rows,
                },
            )
            .await?;
        Ok(())
    }
}
fn cpu_percent(cpu: u64, previous_cpu: u64, system: u64, previous_system: u64, cpus: u64) -> f64 {
    let system_delta = system.saturating_sub(previous_system);
    if system_delta == 0 {
        return 0.0;
    }
    cpu.saturating_sub(previous_cpu) as f64 / system_delta as f64 * cpus as f64 * 100.0
}
fn metrics(s: &Stats) -> Metrics {
    let cpu = cpu_percent(
        s.cpu_stats.cpu_usage.total_usage,
        s.precpu_stats.cpu_usage.total_usage,
        s.cpu_stats.system_cpu_usage.unwrap_or_default(),
        s.precpu_stats.system_cpu_usage.unwrap_or_default(),
        s.cpu_stats.online_cpus.unwrap_or_else(|| {
            s.cpu_stats
                .cpu_usage
                .percpu_usage
                .as_ref()
                .map_or(1, |v| v.len() as u64)
        }),
    );
    // Docker CLI's Linux working-set convention, cgroup v1 and v2.
    let cache = s
        .memory_stats
        .stats
        .as_ref()
        .map(|m| match m {
            MemoryStatsStats::V1(m) => m.total_inactive_file,
            MemoryStatsStats::V2(m) => m.inactive_file,
        })
        .unwrap_or_default();
    let (rx, tx) = s
        .networks
        .as_ref()
        .map(|n| {
            n.values()
                .fold((0, 0), |(rx, tx), v| (rx + v.rx_bytes, tx + v.tx_bytes))
        })
        .unwrap_or_default();
    Metrics {
        cpu,
        memory: s
            .memory_stats
            .usage
            .unwrap_or_default()
            .saturating_sub(cache),
        limit: s.memory_stats.limit.unwrap_or_default(),
        rx,
        tx,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cpu_handles_multiple_cores_and_counter_reset() {
        assert_eq!(cpu_percent(300, 100, 1000, 600, 4), 200.0);
        assert_eq!(cpu_percent(1, 100, 1000, 600, 4), 0.0);
        assert_eq!(cpu_percent(300, 100, 600, 600, 4), 0.0);
    }
}
