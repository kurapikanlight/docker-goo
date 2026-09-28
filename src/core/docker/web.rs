use super::*;
pub(super) fn urls(
    ports: &std::collections::HashMap<String, Option<Vec<bollard::models::PortBinding>>>,
    endpoint: &str,
) -> Result<Vec<String>> {
    let remote = if endpoint.starts_with("unix://") {
        None
    } else {
        url::Url::parse(endpoint)
            .ok()
            .and_then(|u| u.host_str().map(str::to_owned))
            .filter(|host| !matches!(host.as_str(), "localhost" | "127.0.0.1" | "[::1]" | "::1"))
    };
    let mut urls = Vec::new();
    for (key, bindings) in ports {
        if !key.ends_with("/tcp") {
            continue;
        }
        for binding in bindings.iter().flatten() {
            let Some(port) = binding
                .host_port
                .as_deref()
                .filter(|p| p.parse::<u16>().is_ok_and(|v| v > 0))
            else {
                continue;
            };
            let ip = binding.host_ip.as_deref().unwrap_or("0.0.0.0");
            let host = match ip {
                "0.0.0.0" | "::" => remote.as_deref().unwrap_or("127.0.0.1"),
                "127.0.0.1" | "::1" if remote.is_some() => continue,
                other => other,
            };
            let host = if host.contains(':') && !host.starts_with('[') {
                format!("[{host}]")
            } else {
                host.into()
            };
            let scheme = if key.starts_with("443/") || key.starts_with("8443/") {
                "https"
            } else {
                "http"
            };
            urls.push(format!("{scheme}://{host}:{port}"));
        }
    }
    urls.sort();
    urls.dedup();
    Ok(urls)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn does_not_open_remote_loopback_on_local_machine() {
        let ports = std::collections::HashMap::from([(
            "80/tcp".into(),
            Some(vec![bollard::models::PortBinding {
                host_ip: Some("127.0.0.1".into()),
                host_port: Some("8080".into()),
            }]),
        )]);
        assert_eq!(
            urls(&ports, "unix:///var/run/docker.sock").unwrap(),
            ["http://127.0.0.1:8080"]
        );
        assert!(urls(&ports, "ssh://remote").unwrap().is_empty());
        assert_eq!(
            urls(&ports, "tcp://127.0.0.1:2375").unwrap(),
            ["http://127.0.0.1:8080"]
        );
    }
}
