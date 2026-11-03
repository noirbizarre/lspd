//! Integration tests for watch rules (spec §21), running the daemon
//! in-process (see `tests/daemon.rs` for why).

use std::path::Path;
use std::time::Duration;

use lspd::daemon::protocol::{Request, Response};
use lspd::daemon::socket;
use lspd::project::ProjectRoot;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

async fn start_daemon(root: &ProjectRoot) {
    let socket_path = socket::control_socket_path(root.path());
    let spawned_root = root.clone();
    tokio::spawn(async move {
        let _ = lspd::daemon::serve(spawned_root).await;
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
            "daemon never started"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

async fn connect(root: &ProjectRoot) -> BufReader<UnixStream> {
    let socket_path = socket::control_socket_path(root.path());
    let stream = UnixStream::connect(&socket_path).await.unwrap();
    let mut connection = BufReader::new(stream);
    request(&mut connection, &Request::Handshake).await;
    connection
}

async fn request(connection: &mut BufReader<UnixStream>, request: &Request) -> Response {
    let mut line = serde_json::to_string(request).unwrap();
    line.push('\n');
    connection.write_all(line.as_bytes()).await.unwrap();

    let mut response_line = String::new();
    connection.read_line(&mut response_line).await.unwrap();
    serde_json::from_str(response_line.trim_end()).unwrap()
}

fn project_with_watch_rule(marker: &Path) -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("lspd.toml"),
        format!(
            r#"
            [[watch]]
            name = "touch-marker"
            paths = ["*.rs"]
            events = ["file.modified"]
            command = ["touch", "{}"]
            "#,
            marker.display()
        ),
    )
    .unwrap();
    tmp
}

#[tokio::test]
async fn a_matching_event_runs_the_rules_command() {
    let marker_dir = tempfile::TempDir::new().unwrap();
    let marker = marker_dir.path().join("marker");
    let tmp = project_with_watch_rule(&marker);
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    // Created before the daemon starts, specifically so the rule (which
    // only matches `file.modified`, per the spec's own example schema) has
    // an existing file to modify rather than create — creating a new file
    // is a distinct event kind, and a rule that only asked for
    // modifications must not fire on it.
    std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
    start_daemon(&root).await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    std::fs::write(tmp.path().join("main.rs"), "fn main() { /* changed */ }").unwrap();

    wait_for(&marker, Duration::from_secs(3)).await;
    assert!(marker.exists());
}

#[tokio::test]
async fn watch_list_reports_the_configured_rule_as_enabled_by_default() {
    let marker_dir = tempfile::TempDir::new().unwrap();
    let tmp = project_with_watch_rule(&marker_dir.path().join("marker"));
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect(&root).await;

    match request(&mut connection, &Request::WatchList).await {
        Response::WatchList { rules } => {
            assert_eq!(rules.len(), 1);
            assert_eq!(rules[0].name, "touch-marker");
            assert!(rules[0].enabled);
        }
        other => panic!("expected WatchList, got {other:?}"),
    }
}

#[tokio::test]
async fn disabling_a_rule_prevents_it_from_running_without_touching_its_configuration() {
    let marker_dir = tempfile::TempDir::new().unwrap();
    let marker = marker_dir.path().join("marker");
    let tmp = project_with_watch_rule(&marker);
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect(&root).await;

    let response = request(
        &mut connection,
        &Request::WatchSet {
            name: "touch-marker".to_string(),
            enabled: false,
        },
    )
    .await;
    assert!(matches!(response, Response::Ack));

    tokio::time::sleep(Duration::from_millis(150)).await;
    std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
    tokio::time::sleep(Duration::from_millis(700)).await;

    assert!(!marker.exists(), "a disabled rule must not run its command");

    match request(&mut connection, &Request::WatchList).await {
        Response::WatchList { rules } => assert!(!rules[0].enabled),
        other => panic!("expected WatchList, got {other:?}"),
    }
}

#[tokio::test]
async fn setting_an_unknown_rule_name_fails_with_a_structured_error() {
    let marker_dir = tempfile::TempDir::new().unwrap();
    let tmp = project_with_watch_rule(&marker_dir.path().join("marker"));
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = connect(&root).await;

    let response = request(
        &mut connection,
        &Request::WatchSet {
            name: "does-not-exist".to_string(),
            enabled: true,
        },
    )
    .await;
    assert!(matches!(response, Response::Error { .. }));
}

async fn wait_for(path: &Path, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    while !path.exists() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "{} was never created",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
