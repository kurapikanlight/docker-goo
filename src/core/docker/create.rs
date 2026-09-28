use super::*;
use bollard::models::{HostConfig, PortBinding};
use std::collections::HashMap;
pub(super) fn config(spec: &RunSpec) -> Result<Config<String>> {
    anyhow::ensure!(!spec.image.trim().is_empty(), "Image is required");
    let mut exposed = HashMap::new();
    let mut bindings = HashMap::new();
    for entry in spec
        .ports
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let (proto, base) = entry
            .rsplit_once('/')
            .map(|(a, b)| (b, a))
            .unwrap_or(("tcp", entry));
        anyhow::ensure!(
            matches!(proto, "tcp" | "udp"),
            "Port protocol must be tcp or udp"
        );
        let fields = base.split(':').collect::<Vec<_>>();
        let (ip, host, container) = match fields.as_slice() {
            [host, container] => ("127.0.0.1", *host, *container),
            [ip, host, container] => (*ip, *host, *container),
            _ => bail!("Use host:container or IPv4:host:container for ports"),
        };
        ip.parse::<std::net::Ipv4Addr>()
            .context("Invalid bind IPv4 address")?;
        let host = host.parse::<u16>().context("Invalid host port")?;
        let container = container.parse::<u16>().context("Invalid container port")?;
        anyhow::ensure!(host > 0 && container > 0, "Ports must be 1–65535");
        let key = format!("{container}/{proto}");
        exposed.insert(key.clone(), HashMap::new());
        bindings
            .entry(key)
            .or_insert_with(Vec::new)
            .push(PortBinding {
                host_ip: Some(ip.into()),
                host_port: Some(host.to_string()),
            });
    }
    let mut env = Vec::new();
    for value in spec.env.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (key, _) = value
            .split_once('=')
            .context("Environment entries use KEY=value, separated by ;")?;
        anyhow::ensure!(
            !key.is_empty() && !key.chars().any(char::is_whitespace),
            "Invalid environment key"
        );
        env.push(value.to_string());
    }
    let mut binds = Vec::new();
    for value in spec
        .mounts
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let fields = value.split(':').collect::<Vec<_>>();
        anyhow::ensure!(
            fields.len() == 2 || (fields.len() == 3 && matches!(fields[2], "ro" | "rw")),
            "Mount syntax: source:/container/path[:ro]"
        );
        anyhow::ensure!(
            !fields[0].is_empty() && fields[1].starts_with('/'),
            "Mount source required; destination must be absolute"
        );
        binds.push(value.to_string());
    }
    let command = if spec.command.trim().is_empty() {
        None
    } else {
        Some(shell_words::split(&spec.command).context("Invalid command quotes")?)
    };
    Ok(Config {
        image: Some(spec.image.trim().into()),
        cmd: command,
        env: Some(env),
        tty: Some(spec.interactive),
        open_stdin: Some(spec.interactive),
        exposed_ports: Some(exposed),
        host_config: Some(HostConfig {
            port_bindings: Some(bindings.into_iter().map(|(k, v)| (k, Some(v))).collect()),
            binds: Some(binds),
            network_mode: (!spec.network.trim().is_empty()).then(|| spec.network.trim().into()),
            ..Default::default()
        }),
        ..Default::default()
    })
}
pub(super) async fn run(engine: &DockerEngine, spec: RunSpec) -> Result<String> {
    let config = config(&spec)?;
    let client = engine.client().await?;
    match client.inspect_image(spec.image.trim()).await {
        Ok(_) => {}
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => {
            let mut stream = client.create_image(
                Some(bollard::image::CreateImageOptions {
                    from_image: spec.image.trim().to_string(),
                    ..Default::default()
                }),
                None,
                None,
            );
            while let Some(item) = stream.next().await {
                let item = item?;
                if let Some(error) = item.error {
                    bail!("Pull failed: {error}");
                }
            }
        }
        Err(error) => return Err(error.into()),
    }
    let created = client
        .create_container(
            Some(CreateContainerOptions {
                name: spec.name.trim().to_string(),
                platform: None,
            }),
            config,
        )
        .await?;
    if let Err(error) = client
        .start_container(&created.id, None::<StartContainerOptions<String>>)
        .await
    {
        bail!("Created {}, but start failed: {error}. The container remains available for Inspect/Delete.",created.id);
    }
    Ok(created.id)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_ports_environment_and_interactive_shell() {
        let spec = RunSpec {
            image: "ubuntu".into(),
            ports: "8080:80,127.0.0.1:5353:53/udp".into(),
            env: "A=hello world;B=2".into(),
            mounts: "data:/data:ro".into(),
            interactive: true,
            ..Default::default()
        };
        let result = config(&spec).unwrap();
        assert_eq!(result.open_stdin, Some(true));
        assert_eq!(result.env.unwrap(), ["A=hello world", "B=2"]);
        assert_eq!(
            result.host_config.unwrap().port_bindings.unwrap()["80/tcp"]
                .as_ref()
                .unwrap()[0]
                .host_ip
                .as_deref(),
            Some("127.0.0.1")
        );
        assert!(config(&RunSpec {
            ports: "0:80".into(),
            ..spec
        })
        .is_err());
    }
}
