//! Integration tests for the shared filesystem event stream (spec §19, §20),
//! running the daemon in-process (see `tests/daemon.rs` for why).

use std::path::Path;
use std::time::Duration;

use projectd::daemon::events::FsEvent;
use projectd::daemon::protocol::{Request, Response};
use projectd::daemon::socket;
use projectd::project::ProjectRoot;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

async fn start_daemon(root: &ProjectRoot) {
    let socket_path = socket::control_socket_path(root.path());
    let spawned_root = root.clone();
    tokio::spawn(async move {
        let _ = projectd::daemon::serve(spawned_root).await;
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

async fn subscribe(root: &ProjectRoot, paths: Vec<String>) -> BufReader<UnixStream> {
    let socket_path = socket::control_socket_path(root.path());
    let stream = UnixStream::connect(&socket_path).await.unwrap();
    let mut connection = BufReader::new(stream);

    // The handshake must be the connection's first exchange (spec §16)
    // before `Subscribe` — or anything else — is accepted.
    send_line(&mut connection, &Request::Handshake).await;
    let mut handshake_line = String::new();
    connection.read_line(&mut handshake_line).await.unwrap();

    send_line(&mut connection, &Request::Subscribe { paths }).await;

    connection
}

async fn send_line(connection: &mut BufReader<UnixStream>, request: &Request) {
    let mut line = serde_json::to_string(request).unwrap();
    line.push('\n');
    connection.write_all(line.as_bytes()).await.unwrap();
}

async fn next_event(connection: &mut BufReader<UnixStream>) -> FsEvent {
    let mut line = String::new();
    timeout(Duration::from_secs(5), connection.read_line(&mut line))
        .await
        .expect("an event should arrive within the timeout")
        .unwrap();
    match serde_json::from_str(line.trim_end()).unwrap() {
        Response::Event(event) => event,
        other => panic!("expected an event, got {other:?}"),
    }
}

#[tokio::test]
async fn creating_a_file_emits_a_created_event() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = subscribe(&root, vec![]).await;

    // Give the watcher a moment to finish installing its OS-level watch
    // before the write, so this isn't racing the watcher's own startup.
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(tmp.path().join("hello.txt"), "hi").unwrap();

    let event = next_event(&mut connection).await;
    assert_eq!(
        event,
        FsEvent::Created {
            path: "hello.txt".into()
        }
    );
}

#[tokio::test]
async fn renaming_a_file_emits_one_atomic_renamed_event() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    std::fs::write(tmp.path().join("old.txt"), "hi").unwrap();
    // Let the debouncer (300ms window) flush and drop `old.txt`'s own
    // `Created` event before subscribing — otherwise a subscriber connecting
    // mid-window would still observe it, since debouncing delays delivery
    // relative to the write, not relative to when a subscriber joined.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let mut connection = subscribe(&root, vec![]).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::rename(tmp.path().join("old.txt"), tmp.path().join("new.txt")).unwrap();

    let event = next_event(&mut connection).await;
    assert_eq!(
        event,
        FsEvent::Renamed {
            old_path: "old.txt".into(),
            new_path: "new.txt".into(),
        }
    );
}

#[tokio::test]
async fn a_path_filter_only_forwards_matching_events() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = subscribe(&root, vec!["*.rs".to_string()]).await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(tmp.path().join("ignored.txt"), "hi").unwrap();
    std::fs::write(tmp.path().join("matched.rs"), "hi").unwrap();

    // Only the `.rs` creation should ever reach this subscriber — the
    // `.txt` one is filtered server-side, not left for the client to skip.
    let event = next_event(&mut connection).await;
    assert_eq!(
        event,
        FsEvent::Created {
            path: "matched.rs".into()
        }
    );
}

#[tokio::test]
async fn events_under_a_default_ignored_directory_are_never_forwarded() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join("target")).unwrap();
    let root = ProjectRoot::discover(tmp.path()).unwrap();
    start_daemon(&root).await;
    let mut connection = subscribe(&root, vec![]).await;

    tokio::time::sleep(Duration::from_millis(100)).await;
    std::fs::write(tmp.path().join("target/build-output"), "hi").unwrap();
    // A real, non-ignored event afterwards proves the subscription itself
    // works and this test isn't just timing out for an unrelated reason.
    std::fs::write(tmp.path().join("visible.txt"), "hi").unwrap();

    let event = next_event(&mut connection).await;
    assert_eq!(
        event,
        FsEvent::Created {
            path: "visible.txt".into()
        }
    );
}
