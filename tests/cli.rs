use std::{
    future::Future,
    net::SocketAddr,
    process::{Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::Value;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
    process::Command,
    sync::{Semaphore, mpsc},
    task::{JoinHandle, JoinSet},
};

// All HTTP tests use loopback sockets, ephemeral ports and explicit gates.
// No public websites, DNS, inherited proxies or external services are needed.
struct Server {
    address: SocketAddr,
    requests: mpsc::UnboundedReceiver<String>,
    gate: Arc<Semaphore>,
    #[cfg(unix)]
    status: Arc<AtomicU16>,
    task: JoinHandle<()>,
}

impl Server {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (tx, requests) = mpsc::unbounded_channel();
        let gate = Arc::new(Semaphore::new(0));
        let connection_gate = Arc::clone(&gate);
        let status = Arc::new(AtomicU16::new(200));
        let connection_status = Arc::clone(&status);
        let sequence = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn(async move {
            let mut connections = JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (socket, _) = accepted.unwrap();
                        let tx = tx.clone();
                        let gate = Arc::clone(&connection_gate);
                        let status = Arc::clone(&connection_status);
                        let sequence = Arc::clone(&sequence);
                        connections.spawn(async move {
                            let mut socket = BufReader::new(socket);
                            let mut request = String::new();
                            if socket.read_line(&mut request).await.is_err() { return; }
                            let mut headers = String::new();
                            loop {
                                let mut line = String::new();
                                match socket.read_line(&mut line).await {
                                    Ok(0) | Err(_) => return,
                                    Ok(_) if line == "\r\n" => break,
                                    Ok(_) => headers.push_str(&line),
                                }
                            }
                            assert!(request.starts_with("GET "));
                            assert!(headers.to_ascii_lowercase().contains("user-agent: websitehealth/"));
                            let path = request.split_whitespace().nth(1).unwrap().to_owned();
                            tx.send(path.clone()).unwrap();
                            if path.starts_with("/disconnect") { return; }
                            let (status, extra) = match path.split('?').next().unwrap() {
                                "/state" => (if status.load(Ordering::SeqCst) == 200 { "200 OK" } else { "503 Service Unavailable" }, String::new()),
                                path if path.starts_with("/flap/") => (if sequence.fetch_add(1, Ordering::SeqCst).is_multiple_of(2) { "503 Service Unavailable" } else { "200 OK" }, String::new()),
                                "/fail" => ("503 Service Unavailable", String::new()),
                                "/missing" => ("404 Not Found", String::new()),
                                "/redirect" => ("302 Found", "Location: /ok\r\n".into()),
                                "/redirect-slow" => ("302 Found", "Location: /slow\r\n".into()),
                                "/loop" => ("302 Found", "Location: /loop\r\n".into()),
                                "/credential-redirect" => ("302 Found", format!("Location: http://user:secret@{address}/ok\r\n")),
                                "/scheme-redirect" => ("302 Found", "Location: ftp://example.invalid/file\r\n".into()),
                                "/no-location" => ("302 Found", String::new()),
                                "/slow" => {
                                    gate.acquire().await.unwrap().forget();
                                    ("200 OK", String::new())
                                }
                                _ => ("200 OK", String::new()),
                            };
                            let body_length = if path == "/headers-only" { 100_000 } else { 0 };
                            let response = format!("HTTP/1.1 {status}\r\n{extra}Content-Length: {body_length}\r\nConnection: close\r\n\r\n");
                            let _ = socket.get_mut().write_all(response.as_bytes()).await;
                            if body_length != 0 {
                                let _permit = gate.acquire().await.unwrap();
                            }
                        });
                    }
                    result = connections.join_next(), if !connections.is_empty() => {
                        result.unwrap().unwrap();
                    }
                }
            }
        });
        Self {
            address,
            requests,
            gate,
            #[cfg(unix)]
            status,
            task,
        }
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    async fn request(&mut self) -> String {
        deadline(self.requests.recv())
            .await
            .expect("fixture stopped")
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn deadline<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), future)
        .await
        .expect("test operation exceeded 5 seconds")
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_websitehealth"));
    for variable in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
    ] {
        command.env_remove(variable);
    }
    command
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .kill_on_drop(true)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

async fn run(args: &[&str]) -> Output {
    deadline(cli().args(args).output()).await.unwrap()
}

fn records(output: &Output) -> Vec<Value> {
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| {
            let record: Value = serde_json::from_str(line).expect("invalid JSON line");
            assert_eq!(record.as_object().unwrap().len(), 7);
            let timestamp = record["timestamp"].as_str().unwrap();
            chrono::DateTime::parse_from_rfc3339(timestamp).unwrap();
            assert!(timestamp.ends_with('Z'));
            assert!(record["response_time_ms"].is_u64());
            record
        })
        .collect()
}

#[tokio::test]
async fn help_and_version_are_successful_without_urls() {
    let help = run(&["--help"]).await;
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    for flag in [
        "--watch",
        "--interval",
        "--timeout",
        "--concurrency",
        "--json",
        "--setup",
        "--file",
        "--alerts",
        "--beep",
        "130 Ctrl-C",
    ] {
        assert!(help.contains(flag));
    }
    let version = run(&["--version"]).await;
    assert!(version.status.success());
    assert_eq!(
        String::from_utf8(version.stdout).unwrap(),
        "websitehealth 0.1.0\n"
    );
}

#[tokio::test]
async fn single_success_text_and_exit_zero() {
    let server = Server::start().await;
    let url = server.url("/ok");
    let output = run(&[&url]).await;
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains(&format!("UP   #1 {url} | HTTP 200 |")));
    assert!(text.ends_with(" ms\n"));
}

#[tokio::test]
async fn multiple_urls_report_final_status_and_exit_one_for_any_failure() {
    let server = Server::start().await;
    let urls: Vec<_> = ["/ok", "/missing", "/fail", "/redirect", "/no-location"]
        .map(|path| server.url(path))
        .into();
    let mut args = vec!["--json"];
    args.extend(urls.iter().map(String::as_str));
    let output = run(&args).await;
    assert_eq!(output.status.code(), Some(1));
    let mut results = records(&output);
    results.sort_by_key(|record| record["target"].as_u64().unwrap());
    assert_eq!(results.len(), 5);
    for (i, (record, status)) in results.iter().zip([200, 404, 503, 200, 302]).enumerate() {
        assert_eq!(record["target"], i + 1);
        assert_eq!(record["url"], urls[i]);
        assert_eq!(record["status"], status);
        assert_eq!(record["healthy"], status == 200);
        assert!(record["error"].is_null());
    }
}

#[tokio::test]
async fn query_is_sent_but_not_printed_in_text_or_json() {
    let mut server = Server::start().await;
    for json in [false, true] {
        let url = server.url("/ok?token=secret#also-secret");
        let args: Vec<&str> = if json {
            vec!["--json", &url]
        } else {
            vec![&url]
        };
        let output = run(&args).await;
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(server.request().await, "/ok?token=secret");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        if json {
            assert_eq!(records(&output)[0]["url"], server.url("/ok"));
        }
    }
}

#[tokio::test]
async fn timeout_has_no_http_status_and_exits_one() {
    let server = Server::start().await;
    for path in ["/slow?token=secret", "/redirect-slow"] {
        let output = run(&["--json", "--timeout", "0.1", &server.url(path)]).await;
        assert_eq!(output.status.code(), Some(1));
        let results = records(&output);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["healthy"], false);
        assert_eq!(results[0]["error"], "request timed out");
        assert!(results[0]["status"].is_null());
        assert!(results[0]["response_time_ms"].as_u64().unwrap() >= 80);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
    }
}

#[tokio::test]
async fn closed_stdout_exits_two_without_panicking() {
    let server = Server::start().await;
    // Close the read end before launching the process, so no async pipe
    // registration or child teardown can race with the first result write.
    let (reader, writer) = std::io::pipe().unwrap();
    drop(reader);
    // Command::output would replace the explicit stdout with a capture pipe.
    let child = cli().stdout(writer).arg(server.url("/ok")).spawn().unwrap();
    let output = deadline(child.wait_with_output()).await.unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn response_body_does_not_delay_header_check() {
    let server = Server::start().await;
    let output = run(&["--json", "--timeout", "0.5", &server.url("/headers-only")]).await;
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(records(&output)[0]["status"], 200);
}

#[tokio::test]
async fn ready_results_and_queued_targets_progress_while_one_request_stalls() {
    let mut server = Server::start().await;
    let mut child = cli()
        .args([
            "--json",
            "--concurrency",
            "2",
            "--timeout",
            "30",
            &server.url("/slow"),
            &server.url("/ok?second"),
            &server.url("/ok?third"),
        ])
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    // The stalled request cannot finish until we open its gate below.
    for expected_target in [2, 3] {
        let line = deadline(lines.next_line()).await.unwrap().unwrap();
        let result: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(result["target"], expected_target);
        assert_eq!(result["healthy"], true);
    }
    let mut paths = vec![
        server.request().await,
        server.request().await,
        server.request().await,
    ];
    paths.sort();
    assert_eq!(paths, ["/ok?second", "/ok?third", "/slow"]);
    server.gate.add_permits(1);
    let result: Value =
        serde_json::from_str(&deadline(lines.next_line()).await.unwrap().unwrap()).unwrap();
    assert_eq!(result["target"], 1);
    assert_eq!(result["healthy"], true);
    assert!(deadline(child.wait()).await.unwrap().success());
}

#[tokio::test]
async fn concurrent_requests_never_exceed_configured_limit() {
    let mut server = Server::start().await;
    let urls: Vec<_> = (0..6).map(|i| server.url(&format!("/slow?{i}"))).collect();
    let child = cli()
        .args(["--json", "--timeout", "30", "--concurrency", "2"])
        .args(urls)
        .spawn()
        .unwrap();
    server.request().await;
    server.request().await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), server.requests.recv())
            .await
            .is_err()
    );
    for _ in 0..4 {
        server.gate.add_permits(1);
        server.request().await;
        assert!(server.requests.try_recv().is_err());
    }
    server.gate.add_permits(2);
    let output = deadline(child.wait_with_output()).await.unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(records(&output).len(), 6);
}

#[tokio::test]
async fn redirect_loops_and_unsupported_targets_fail_without_leaking_credentials() {
    let mut server = Server::start().await;
    for path in ["/loop", "/credential-redirect", "/scheme-redirect"] {
        let output = run(&["--json", &server.url(path)]).await;
        assert_eq!(output.status.code(), Some(1));
        let result = &records(&output)[0];
        assert_eq!(result["healthy"], false);
        assert!(result["status"].is_null());
        assert_eq!(result["error"], "redirect failed");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("secret"));
        let mut count = 0;
        while let Ok(request) = server.requests.try_recv() {
            assert_eq!(request, path);
            count += 1;
        }
        assert_eq!(count, if path == "/loop" { 11 } else { 1 });
    }
}

#[tokio::test]
async fn invalid_arguments_exit_two_without_network_requests_or_secret_echoes() {
    let mut server = Server::start().await;
    let valid_url = server.url("/ok");
    for args in [
        vec![],
        vec!["ftp://user:secret@example.invalid"],
        vec![&valid_url, "https://user:secret@example.invalid"],
        vec!["https://[secret"],
        vec!["example.invalid"],
        vec!["--interval", "1", &valid_url],
        vec!["--timeout", "0", &valid_url],
        vec!["--timeout", "NaN", &valid_url],
        vec!["--timeout", "inf", &valid_url],
        vec!["--timeout=https://user:secret@example.invalid", &valid_url],
        vec!["-chttps://user:secret@example.invalid/"],
        vec!["-wac=https://example.invalid/?token=secret"],
        vec!["-thttps://example.invalid/?token=secret"],
        vec!["-cexample.invalid/?token=secret"],
        vec!["--setup"],
        vec!["--alerts", &valid_url],
        vec!["--beep", &valid_url],
        vec!["--concurrency", "0", &valid_url],
        vec!["--concurrency", "257", &valid_url],
        vec!["--unknown", &valid_url],
    ] {
        let output = run(&args).await;
        assert_eq!(output.status.code(), Some(2), "args: {args:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert!(server.requests.try_recv().is_err());
    }
}

#[tokio::test]
async fn network_failure_has_concise_error_and_exit_one() {
    let server = Server::start().await;
    let output = run(&["--json", &server.url("/disconnect?token=secret")]).await;
    assert_eq!(output.status.code(), Some(1));
    let result = &records(&output)[0];
    assert!(result["status"].is_null());
    assert_eq!(result["error"], "request failed");
}

#[tokio::test]
async fn untrusted_tls_certificate_is_rejected() {
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
    };
    let cert = rcgen::generate_simple_self_signed(vec!["127.0.0.1".into()]).unwrap();
    let key = PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key.into())
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("https://{}/", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        if let Ok(mut stream) = acceptor.accept(stream).await {
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .await;
        }
    });
    let output = run(&["--json", &url]).await;
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        records(&output)[0]["error"],
        "connection failed (DNS, TCP, or TLS)"
    );
    deadline(server).await.unwrap();
}

#[cfg(unix)]
async fn interrupt(child: &mut tokio::process::Child) {
    let status = deadline(
        Command::new("kill")
            .args(["-INT", &child.id().unwrap().to_string()])
            .status(),
    )
    .await
    .unwrap();
    assert!(status.success());
    assert_eq!(deadline(child.wait()).await.unwrap().code(), Some(130));
}

#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_cancels_inflight_requests_and_leaves_valid_json() {
    let mut server = Server::start().await;
    let mut child = cli()
        .args([
            "--json",
            "--watch",
            "--timeout",
            "30",
            &server.url("/slow"),
            &server.url("/ok"),
        ])
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let result: Value =
        serde_json::from_str(&deadline(lines.next_line()).await.unwrap().unwrap()).unwrap();
    assert_eq!(result["target"], 2);
    server.request().await;
    server.request().await;
    interrupt(&mut child).await;
    assert!(deadline(lines.next_line()).await.unwrap().is_none());
}

#[cfg(unix)]
#[tokio::test]
async fn watch_repeats_failures_and_ctrl_c_interrupts_interval_sleep() {
    let server = Server::start().await;
    for (interval, rounds) in [("0.01", 2), ("60", 1)] {
        let mut child = cli()
            .args([
                "--json",
                "--watch",
                "--interval",
                interval,
                &server.url("/fail"),
            ])
            .spawn()
            .unwrap();
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        for _ in 0..rounds {
            let result: Value =
                serde_json::from_str(&deadline(lines.next_line()).await.unwrap().unwrap()).unwrap();
            assert_eq!(result["status"], 503);
        }
        interrupt(&mut child).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn ctrl_c_exits_with_an_open_but_unread_stdout_or_alert_pipe() {
    for blocked_stderr in [false, true] {
        let mut server = Server::start().await;
        let path = format!("/flap/{}", "x".repeat(16_384));
        let mut command = cli();
        command.args(["-w", "-a", "-j", "-i", "0.001", &server.url(&path)]);
        if blocked_stderr {
            command.stdout(Stdio::null());
        } else {
            command.stderr(Stdio::null());
        }
        let mut child = command.spawn().unwrap();
        server.request().await;
        // Large records fill the pipe in a few rounds. Keep the read end open
        // and never drain it, including while waiting for SIGINT to take effect.
        let mut saturated = false;
        for _ in 0..100 {
            if tokio::time::timeout(Duration::from_millis(200), server.requests.recv())
                .await
                .is_err()
            {
                saturated = true;
                break;
            }
        }
        assert!(saturated, "the fixture did not saturate the output pipe");
        interrupt(&mut child).await;
    }
}

#[cfg(unix)]
#[tokio::test]
async fn local_alerts_report_initial_failure_and_recovery_without_repeating_or_leaking() {
    let server = Server::start().await;
    server.status.store(503, Ordering::SeqCst);
    let mut child = cli()
        .args(["-wajb", "-i", "0.01", &server.url("/state?token=secret")])
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut stderr = BufReader::new(child.stderr.take().unwrap()).lines();
    let first = deadline(stderr.next_line()).await.unwrap().unwrap();
    assert!(first.contains("ALERT DOWN #1"));
    assert!(!first.contains("secret"));
    assert!(
        !first.contains('\u{7}'),
        "beeps must not contaminate redirected stderr"
    );
    for _ in 0..3 {
        let result: Value =
            serde_json::from_str(&deadline(stdout.next_line()).await.unwrap().unwrap()).unwrap();
        assert_eq!(result["status"], 503);
        assert_eq!(result.as_object().unwrap().len(), 7);
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(50), stderr.next_line())
            .await
            .is_err()
    );
    server.status.store(200, Ordering::SeqCst);
    assert!(
        deadline(stderr.next_line())
            .await
            .unwrap()
            .unwrap()
            .contains("ALERT RECOVERED #1")
    );
    server.status.store(503, Ordering::SeqCst);
    assert!(
        deadline(stderr.next_line())
            .await
            .unwrap()
            .unwrap()
            .contains("ALERT DOWN #1")
    );
    interrupt(&mut child).await;
}

struct ScratchDir(std::path::PathBuf);

impl ScratchDir {
    fn new() -> Self {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("websitehealth-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn url_files_combine_with_positional_urls_and_validate_before_requests() {
    let mut server = Server::start().await;
    let scratch = ScratchDir::new();
    let path = scratch.0.join("websites.txt");
    std::fs::write(
        &path,
        format!(
            "# My sites\r\n\r\n {}\r\n{}\n",
            server.url("/ok"),
            server.url("/redirect")
        ),
    )
    .unwrap();
    let output = run(&["-j", "-f", path.to_str().unwrap(), &server.url("/fail")]).await;
    assert_eq!(output.status.code(), Some(1));
    let mut results = records(&output);
    results.sort_by_key(|r| r["target"].as_u64().unwrap());
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["url"], server.url("/ok"));
    assert_eq!(results[1]["url"], server.url("/redirect"));
    assert_eq!(results[2]["url"], server.url("/fail"));
    while server.requests.try_recv().is_ok() {}
    for text in [
        "# empty\n".to_owned(),
        format!("{}\nhttps://user:secret@example.invalid", server.url("/ok")),
    ] {
        std::fs::write(&path, text).unwrap();
        let output = run(&["-f", path.to_str().unwrap()]).await;
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
        assert!(server.requests.try_recv().is_err());
    }
    let output = run(&["-f", scratch.0.join("missing").to_str().unwrap()]).await;
    assert_eq!(output.status.code(), Some(2));
}
