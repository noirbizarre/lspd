//! Integration tests for the daemon's control protocol and LSP proxy,
//! running `lspd::daemon::serve` in-process (no subprocess spawn) so
//! failures are attributable to the protocol itself rather than to process
//! management. `tests/cli.rs` covers the subprocess discover-or-start path.

use std::path::Path;
use std::time::Duration;

use lspd::daemon::protocol::{Request, Response};
use lspd::daemon::{handshake, socket};
use lspd::project::ProjectRoot;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

/// Starts a daemon for a temp project directory and returns once its control
/// socket accepts a connection — the same "connect as soon as the socket
/// exists" race a real client's `discover_or_start` retries into, exercised
/// here deterministically instead of via a spawned subprocess.
async fn start_daemon(root: &ProjectRoot) {
    let socket_path = socket::control_socket_path(root.path());
    let spawned_root = root.clone();
    tokio::spawn(async move {
        if let Err(error) = lspd::daemon::serve(spawned_root).await {
            eprintln!("test daemon exited with an error: {error}");
        }
    });

    wait_for_socket(&socket_path).await;
}

async fn wait_for_socket(path: &Path) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if UnixStream::connect(path).await.is_ok() {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "daemon never started listening on {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn connect_and_handshake(root: &ProjectRoot) -> BufReader<UnixStream> {
    let socket_path = socket::control_socket_path(root.path());
    let stream = UnixStream::connect(&socket_path)
        .await
        .expect("the daemon's socket should already be accepting connections");
    let mut connection = BufReader::new(stream);
    send(&mut connection, &Request::Handshake).await;
    let response = read(&mut connection).await;
    match response {
        Response::Handshake(h) => {
            h.verify()
                .expect("a freshly built daemon must speak this build's protocol");
        }
        other => panic!("expected a handshake response, got {other:?}"),
    }
    connection
}

async fn send(connection: &mut BufReader<UnixStream>, request: &Request) {
    let mut line = serde_json::to_string(request).unwrap();
    line.push('\n');
    connection.write_all(line.as_bytes()).await.unwrap();
}

async fn read(connection: &mut BufReader<UnixStream>) -> Response {
    let mut line = String::new();
    let bytes_read = timeout(Duration::from_secs(5), connection.read_line(&mut line))
        .await
        .expect("reading a response should not time out")
        .expect("reading a response should not error");
    assert_ne!(
        bytes_read, 0,
        "the daemon closed the connection without responding"
    );
    serde_json::from_str(line.trim_end())
        .unwrap_or_else(|err| panic!("malformed response {line:?}: {err}"))
}

fn python_project_with_stub_lsp(name: &str, command: &str) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("pyproject.toml"), "").unwrap();
    std::fs::write(
        tmp.path().join("lspd.toml"),
        format!(
            r#"
            [lsps]
            enabled = ["{name}"]

            [lsp.{name}]
            command = ["{command}"]
            "#
        ),
    )
    .unwrap();
    tmp
}

/// Regression test for the exact race observed manually: a client connecting
/// the instant the socket exists — potentially before the daemon's own
/// accept loop has started servicing it — must still get a well-formed
/// handshake response, not a torn or malformed one.
#[tokio::test]
async fn a_client_connecting_immediately_after_the_socket_exists_still_gets_a_valid_handshake() {
    let tmp = python_project_with_stub_lsp("echo", "cat");
    let root = ProjectRoot::discover(tmp.path()).unwrap();

    start_daemon(&root).await;
    let mut connection = connect_and_handshake(&root).await;

    // A second exchange on the same connection, to confirm the daemon is
    // fully functional afterwards too, not just able to answer once.
    send(
        &mut connection,
        &Request::LspStatus {
            name: "echo".to_string(),
        },
    )
    .await;
    let response = read(&mut connection).await;
    assert!(matches!(response, Response::LspStatus(_)));
}

#[tokio::test]
async fn ensuring_an_lsp_starts_it_and_reusing_ensure_does_not_start_a_second_process() {
    let tmp = python_project_with_stub_lsp("echo", "cat");
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect_and_handshake(&root).await;

    send(
        &mut connection,
        &Request::LspEnsure {
            name: "echo".to_string(),
        },
    )
    .await;
    let first = match read(&mut connection).await {
        Response::LspEnsured { status, .. } => status,
        other => panic!("expected LspEnsured, got {other:?}"),
    };
    assert_eq!(first.status, lspd::daemon::lsp_manager::LspStatus::Running);
    let pid = first.pid.expect("a running instance has a pid");

    send(
        &mut connection,
        &Request::LspEnsure {
            name: "echo".to_string(),
        },
    )
    .await;
    let second = match read(&mut connection).await {
        Response::LspEnsured { status, .. } => status,
        other => panic!("expected LspEnsured, got {other:?}"),
    };
    assert_eq!(
        second.pid,
        Some(pid),
        "ensure must reuse the running instance, not start a duplicate"
    );
}

#[tokio::test]
async fn the_proxy_socket_round_trips_bytes_to_and_from_the_stub_process() {
    let tmp = python_project_with_stub_lsp("echo", "cat");
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect_and_handshake(&root).await;

    send(
        &mut connection,
        &Request::LspEndpoint {
            name: "echo".to_string(),
        },
    )
    .await;
    let proxy_socket = match read(&mut connection).await {
        Response::LspEndpoint { socket } => socket,
        other => panic!("expected LspEndpoint, got {other:?}"),
    };

    let mut proxy = UnixStream::connect(&proxy_socket)
        .await
        .expect("proxy socket should accept a connection");
    proxy.write_all(b"hello lsp\n").await.unwrap();
    let mut buf = [0u8; 32];
    let n = timeout(
        Duration::from_secs(2),
        tokio::io::AsyncReadExt::read(&mut proxy, &mut buf),
    )
    .await
    .expect("the stub should echo back promptly")
    .unwrap();
    assert_eq!(&buf[..n], b"hello lsp\n");
}

#[tokio::test]
async fn a_second_concurrent_proxy_client_is_rejected_while_one_is_attached() {
    let tmp = python_project_with_stub_lsp("echo", "cat");
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect_and_handshake(&root).await;

    send(
        &mut connection,
        &Request::LspEndpoint {
            name: "echo".to_string(),
        },
    )
    .await;
    let proxy_socket = match read(&mut connection).await {
        Response::LspEndpoint { socket } => socket,
        other => panic!("expected LspEndpoint, got {other:?}"),
    };

    // Hold the first connection open so it stays attached.
    let _first = UnixStream::connect(&proxy_socket).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let mut second = UnixStream::connect(&proxy_socket).await.unwrap();
    // A rejected client is closed rather than sent an error frame (it shares
    // the raw LSP byte stream, which cannot safely carry a JSON error) — so
    // the observable behaviour is that writes eventually fail or reads hit
    // EOF, never that it silently proxies to the same process.
    let mut buf = [0u8; 16];
    let read_result = timeout(
        Duration::from_secs(1),
        tokio::io::AsyncReadExt::read(&mut second, &mut buf),
    )
    .await;
    match read_result {
        Ok(Ok(0)) => {} // EOF: closed immediately, as expected.
        Ok(Ok(n)) => panic!("a busy proxy must not send data to a rejected client, got {n} bytes"),
        Ok(Err(_)) => {} // Connection reset is also an acceptable rejection signal.
        Err(_) => panic!("a rejected client should be closed promptly, not left hanging"),
    }
}

/// Confirms `daemon::handshake::PROTOCOL_VERSION` and `PROTOCOL_NAME` are
/// what a freshly started daemon actually reports — a drift here would mean
/// the constants and the running code disagree.
#[tokio::test]
async fn the_handshake_reports_this_builds_protocol_identity() {
    let tmp = python_project_with_stub_lsp("echo", "cat");
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;

    let socket_path = socket::control_socket_path(root.path());
    let stream = UnixStream::connect(&socket_path).await.unwrap();
    let mut connection = BufReader::new(stream);
    send(&mut connection, &Request::Handshake).await;
    match read(&mut connection).await {
        Response::Handshake(h) => {
            assert_eq!(h.protocol, handshake::PROTOCOL_NAME);
            assert_eq!(h.protocol_version, handshake::PROTOCOL_VERSION);
            assert_eq!(h.project_root, root.path());
        }
        other => panic!("expected a handshake response, got {other:?}"),
    }
}
