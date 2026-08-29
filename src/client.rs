//! The daemon client: discover-or-start (spec §14) plus the request/response
//! exchange over the control socket. Shared by `commands::*` (the CLI) and
//! `mcp::tools` (the MCP server) — both need the exact same "is a daemon
//! running for this project, and if not, start one" logic, and the spec is
//! explicit that lifecycle/ownership decisions belong to ProjectD, not to
//! each client independently.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use crate::daemon::handshake::Handshake;
use crate::daemon::protocol::{Request, Response};
use crate::daemon::socket;
use crate::error::Error;
use crate::project::ProjectRoot;

/// An established, handshaken connection to a project's daemon.
pub type Connection = BufReader<UnixStream>;

/// How long to wait for a freshly spawned daemon to start accepting
/// connections before giving up. Generous: the first connection after a
/// cold `cargo`/binary start also pays process-spawn and config-resolution
/// cost, not just socket bind time.
const START_TIMEOUT: Duration = Duration::from_secs(5);

/// Connects to `root`'s daemon, starting one if none is reachable, then
/// performs the handshake. Returns a connection ready for further requests.
pub async fn discover_or_start(root: &ProjectRoot) -> crate::Result<Connection> {
    let socket_path = socket::control_socket_path(root.path());

    let stream = match UnixStream::connect(&socket_path).await {
        Ok(stream) => stream,
        Err(_) => {
            spawn_daemon(root)?;
            wait_for_socket(&socket_path).await?
        }
    };

    let mut connection = BufReader::new(stream);
    let response = request(&mut connection, Request::Handshake).await?;
    match response {
        Response::Handshake(handshake) => {
            handshake.verify()?;
            Ok(connection)
        }
        other => Err(unexpected_response(other)),
    }
}

/// Sends one request and reads the matching response frame.
pub async fn request(connection: &mut Connection, request: Request) -> crate::Result<Response> {
    let mut line =
        serde_json::to_string(&request).map_err(|source| Error::MalformedJson { source })?;
    line.push('\n');
    connection
        .write_all(line.as_bytes())
        .await
        .map_err(|source| Error::io("<daemon socket>", source))?;

    read_response(connection).await
}

/// Reads one response frame without sending anything first — used after
/// `Request::Subscribe`, which puts the daemon side into push-only mode: the
/// daemon keeps sending `Response::Event` frames on its own, and a client
/// reading them must not interleave further requests on the same
/// connection.
pub async fn read_response(connection: &mut Connection) -> crate::Result<Response> {
    let mut response_line = String::new();
    let bytes_read = connection
        .read_line(&mut response_line)
        .await
        .map_err(|source| Error::io("<daemon socket>", source))?;
    if bytes_read == 0 {
        return Err(Error::DaemonUnreachable {
            source: std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "daemon closed the connection",
            ),
        });
    }

    let response: Response = serde_json::from_str(response_line.trim_end())
        .map_err(|source| Error::MalformedJson { source })?;

    if let Response::Error { code, message } = response {
        return Err(Error::RequestFailed { code, message });
    }

    Ok(response)
}

/// Builds an error for a response that did not match the variant a caller
/// expected for the request it sent — this should only happen if the
/// daemon and client disagree about the protocol despite a passing
/// handshake, which is itself worth surfacing clearly rather than panicking.
#[must_use]
pub fn unexpected_response(response: Response) -> Error {
    Error::RequestFailed {
        code: "projectd::daemon::unexpected_response".to_string(),
        message: format!("daemon returned an unexpected response: {response:?}"),
    }
}

fn spawn_daemon(root: &ProjectRoot) -> crate::Result<()> {
    let exe =
        std::env::current_exe().map_err(|source| Error::io("<current executable>", source))?;

    let log_dir = directories::ProjectDirs::from("", "", "projectd")
        .map(|dirs| {
            dirs.state_dir()
                .unwrap_or_else(|| dirs.cache_dir())
                .to_path_buf()
        })
        .unwrap_or_else(std::env::temp_dir);
    std::fs::create_dir_all(&log_dir).map_err(|source| Error::io(&log_dir, source))?;
    let log_path = log_dir.join(format!("{}.log", socket::project_hash(root.path())));
    let log_file =
        std::fs::File::create(&log_path).map_err(|source| Error::io(&log_path, source))?;
    let log_file_err = log_file
        .try_clone()
        .map_err(|source| Error::io(&log_path, source))?;

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(exe)
            .arg("--project")
            .arg(root.path())
            .arg("daemon")
            .stdin(std::process::Stdio::null())
            .stdout(log_file)
            .stderr(log_file_err)
            // A fresh process group, so signals sent to the CLI invocation's
            // group (e.g. Ctrl-C in a terminal) do not also reach the
            // detached daemon it just started.
            .process_group(0)
            .spawn()
            .map_err(|source| Error::io("<projectd daemon>", source))?;
    }

    Ok(())
}

async fn wait_for_socket(socket_path: &std::path::Path) -> crate::Result<UnixStream> {
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        match UnixStream::connect(socket_path).await {
            Ok(stream) => return Ok(stream),
            Err(source) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(Error::DaemonUnreachable { source });
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

/// Convenience for `projectd status`: connects only if a daemon is already
/// reachable, without starting one — `status` should never have the side
/// effect of spawning a daemon just to report that none was running.
pub async fn try_handshake(root: &ProjectRoot) -> Option<Handshake> {
    let socket_path = socket::control_socket_path(root.path());
    let stream = UnixStream::connect(&socket_path).await.ok()?;
    let mut connection = BufReader::new(stream);
    match request(&mut connection, Request::Handshake).await.ok()? {
        Response::Handshake(handshake) => Some(handshake),
        _ => None,
    }
}
