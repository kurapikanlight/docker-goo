use super::*;
use serde_json::json;
use std::sync::Arc;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixListener,
    sync::Mutex,
};

#[tokio::test]
async fn engine_api_roundtrip_and_safe_delete() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("engine.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let recorded = requests.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let recorded = recorded.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).await.is_err() {
                        return;
                    }
                    request.push(byte[0]);
                }
                let header = String::from_utf8_lossy(&request);
                let first = header.lines().next().unwrap().to_string();
                recorded.lock().await.push(first.clone());
                let path = first.split_whitespace().nth(1).unwrap();
                let (status, body) = if first.starts_with("DELETE")
                    || path.contains("/start")
                    || path.contains("/stop")
                    || path.contains("/restart")
                    || path.contains("/pause")
                    || path.contains("/unpause")
                {
                    (204, Vec::new())
                } else if path.ends_with("/version") {
                    (
                        200,
                        json!({"ApiVersion":"1.47","Version":"test"})
                            .to_string()
                            .into_bytes(),
                    )
                } else if path.ends_with("/info") {
                    (200,json!({"Name":"fixture","ServerVersion":"test","NCPU":4,"MemTotal":1073741824}).to_string().into_bytes())
                } else if path.contains("/containers/json") {
                    (200,json!([{"Id":"abc","Names":["/demo"],"Image":"alpine:test","State":"running","Labels":{"com.docker.compose.project":"my-stack"},"Ports":[{"PrivatePort":80,"PublicPort":8080,"IP":"127.0.0.1","Type":"tcp"}]}]).to_string().into_bytes())
                } else if path.contains("/images/json") {
                    let image = bollard::models::ImageSummary {
                        id: "sha256:test".into(),
                        repo_tags: vec!["alpine:test".into()],
                        size: 1048576,
                        ..Default::default()
                    };
                    (200, serde_json::to_vec(&vec![image]).unwrap())
                } else if path.contains("/volumes") {
                    (
                        200,
                        json!({
                            "Volumes": [{
                                "Name": "data",
                                "Driver": "local",
                                "Mountpoint": "/data",
                                "Scope": "local",
                                "Labels": {},
                                "Options": {}
                            }],
                            "Warnings": []
                        })
                        .to_string()
                        .into_bytes(),
                    )
                } else if path.contains("/networks") {
                    (
                        200,
                        json!([{"Id":"net1","Name":"bridge","Driver":"bridge","Scope":"local"}])
                            .to_string()
                            .into_bytes(),
                    )
                } else if path.contains("/logs") {
                    let log = b"hello from Docker\n";
                    let mut body = vec![1, 0, 0, 0];
                    body.extend_from_slice(&(log.len() as u32).to_be_bytes());
                    body.extend_from_slice(log);
                    (200, body)
                } else if path.ends_with("/containers/abc/json") {
                    (
                        200,
                        json!({"Id":"abc","Name":"/demo"}).to_string().into_bytes(),
                    )
                } else {
                    (404, b"{\"message\":\"unknown endpoint\"}".to_vec())
                };
                let response=format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len());
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.write_all(&body).await.unwrap();
            });
        }
    });
    let engine = DockerEngine::connect(Endpoint {
        host: format!("unix://{}", socket.display()),
        certificates: None,
    })
    .await
    .unwrap();
    let snapshot = engine.snapshot().await.unwrap();
    assert_eq!(snapshot.containers[0].name, "demo");
    assert_eq!(snapshot.containers[0].extra, "127.0.0.1:8080->80");
    assert_eq!(snapshot.images[0].detail, "1.0 MiB");
    assert_eq!(snapshot.volumes[0].name, "data");
    assert_eq!(snapshot.networks[0].name, "bridge");
    assert_eq!(snapshot.compose[0].name, "my-stack");
    assert_eq!(snapshot.compose[0].state, "1 running");
    let logs = engine
        .logs("abc")
        .await
        .unwrap()
        .next()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(logs, "hello from Docker\n");
    assert!(engine
        .inspect(Kind::Containers, "abc")
        .await
        .unwrap()
        .contains("demo"));
    for action in [
        Action::Start,
        Action::Stop,
        Action::Restart,
        Action::Pause,
        Action::Resume,
        Action::Delete,
    ] {
        engine.action("abc", action).await.unwrap();
    }
    let requests = requests.lock().await;
    assert!(requests
        .iter()
        .any(|r| r.contains("containers/json?all=true")));
    let deletion = requests.iter().find(|r| r.starts_with("DELETE")).unwrap();
    assert!(deletion.contains("force=false"));
    assert!(deletion.contains("v=false"));
    server.abort();
}

#[test]
fn stats_decode_and_linux_memory_accounting() {
    let cpu = |total, system| json!({"cpu_usage":{"total_usage":total,"usage_in_usermode":0,"usage_in_kernelmode":0},"system_cpu_usage":system,"online_cpus":4,"throttling_data":{"periods":0,"throttled_periods":0,"throttled_time":0}});
    let value = json!({"read":"2026-01-01T00:00:02Z","preread":"2026-01-01T00:00:01Z","num_procs":0,"pids_stats":{},"memory_stats":{"usage":1048576,"limit":2097152},"blkio_stats":{},"storage_stats":{},"cpu_stats":cpu(300,1000),"precpu_stats":cpu(100,600),"networks":{"eth0":{"rx_bytes":1000,"tx_bytes":500,"rx_packets":1,"tx_packets":1,"rx_errors":0,"tx_errors":0,"rx_dropped":0,"tx_dropped":0}}});
    let stats: Stats = serde_json::from_value(value).unwrap();
    let stats = metrics(&stats);
    assert_eq!(stats.cpu, 200.0);
    assert_eq!(stats.memory, 1048576);
    assert_eq!(stats.limit, 2097152);
    assert_eq!((stats.rx, stats.tx), (1000, 500));
}

// Exercise the actual Engine method against a Docker HTTP boundary.
#[tokio::test]
async fn terminal_attachment_and_exec_requests() {
    for (running, command, tty, stdin, paused) in [
        (true, "auto", true, true, false),
        (false, "auto", true, true, false),
        (false, "/bin/bash", true, true, false),
        (true, "/bin/bash", false, false, false),
        (true, "/bin/sh", false, false, false),
        (true, "python -i", false, false, false),
        (true, "auto", false, true, false),
        (true, "auto", true, false, false),
        (false, "auto", false, false, false),
        (true, "auto", true, true, true),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("terminal.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        let requests = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let recorded = requests.clone();
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let recorded = recorded.clone();
                tokio::spawn(async move {
                    let mut header = Vec::new();
                    let mut byte = [0];
                    while !header.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).await.is_err() {
                            return;
                        }
                        header.push(byte[0]);
                    }
                    let header = String::from_utf8(header).unwrap();
                    let first = header.lines().next().unwrap().to_string();
                    let length = header
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    let mut body = vec![0; length];
                    stream.read_exact(&mut body).await.unwrap();
                    recorded
                        .lock()
                        .await
                        .push((first.clone(), String::from_utf8(body).unwrap()));
                    if first.contains("/attach?") || first.contains("/exec/session/start") {
                        stream.write_all(b"HTTP/1.1 101 UPGRADED\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n").await.unwrap();
                        let mut input = [0; 32];
                        let _ = stream.read(&mut input).await;
                        return;
                    }
                    let body = if first.contains("/json") {
                        json!({"State":{"Running":running,"Paused":paused},"Config":{"Tty":tty,"OpenStdin":stdin}}).to_string()
                    } else if first.contains("/exec") {
                        json!({"Id":"session"}).to_string()
                    } else {
                        String::new()
                    };
                    let status = if body.is_empty() { 204 } else { 200 };
                    let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    stream.write_all(response.as_bytes()).await.unwrap();
                });
            }
        });
        let engine = DockerEngine::connect(Endpoint {
            host: format!("unix://{}", socket.display()),
            certificates: None,
        })
        .await
        .unwrap();
        let result = engine.shell("abc", command, 101, 37).await;
        let attach = !running || command == "auto";
        if paused || (attach && !(tty && stdin)) {
            let error = result
                .err()
                .expect("must reject unsupported attachment")
                .to_string();
            assert!(
                error.contains(if paused { "paused" } else { "-it" }),
                "{error}"
            );
            assert_eq!(requests.lock().await.len(), 1);
        } else {
            let session = result.unwrap();
            assert_eq!(session.id, if attach { "container:abc" } else { "session" });
            let requests = requests.lock().await;
            if attach {
                let attach_index = requests
                    .iter()
                    .position(|(r, _)| r.contains("/attach?"))
                    .unwrap();
                let request = &requests[attach_index].0;
                for option in [
                    "stdin=true",
                    "stdout=true",
                    "stderr=true",
                    "stream=true",
                    "logs=false",
                ] {
                    assert!(request.contains(option), "{request}");
                }
                assert!(!requests.iter().any(|(r, _)| r.contains("/exec")));
                let start_index = requests.iter().position(|(r, _)| r.contains("/start"));
                if running {
                    assert!(start_index.is_none());
                } else {
                    assert!(attach_index < start_index.unwrap());
                }
                assert!(requests
                    .iter()
                    .any(|(r, _)| r.contains("/containers/abc/resize?")
                        && r.contains("w=101")
                        && r.contains("h=37")));
            } else {
                assert!(!requests
                    .iter()
                    .any(|(r, _)| r.contains("/attach") || r.contains("/containers/abc/start")));
                let body = &requests
                    .iter()
                    .find(|(r, _)| r.contains("/containers/abc/exec"))
                    .unwrap()
                    .1;
                let value: serde_json::Value = serde_json::from_str(body).unwrap();
                let expected = match command {
                    "/bin/bash" => json!(["/bin/bash"]),
                    "/bin/sh" => json!(["/bin/sh"]),
                    _ => json!(["python", "-i"]),
                };
                assert_eq!(value["Cmd"], expected);
                assert!(requests
                    .iter()
                    .any(|(r, _)| r.contains("/exec/session/resize?")));
            }
        }
        server.abort();
    }
}
