//! The project-scoped daemon: the long-lived process behind one project's
//! control socket, owning its LSP instances, filesystem watcher and watch
//! rule execution (spec §14, §15).

pub mod events;
pub mod handshake;
pub mod lsp_manager;
pub mod protocol;
pub mod socket;
pub mod watch_runtime;

use std::path::Path;
use std::sync::Arc;

use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Notify;
use tracing::{info, warn};

use crate::config::EffectiveConfig;
use crate::error::Error;
use crate::mise::{self, MiseProvider};
use crate::project::ProjectRoot;

use events::Watcher;
use handshake::Handshake;
use lsp_manager::LspManager;
use protocol::{Request, Response};
use watch_runtime::WatchRuntime;

/// Everything one running daemon instance owns, shared across every accepted
/// connection via `Arc`.
struct DaemonState {
    root: ProjectRoot,
    handshake: Handshake,
    lsp_manager: LspManager,
    watch_runtime: Arc<WatchRuntime>,
    /// Kept alive for the daemon's lifetime; also re-subscribed from by
    /// every `Request::Subscribe` connection.
    watcher: Watcher,
    shutdown: Notify,
}

/// Runs the daemon for `root` in the foreground until it is asked to shut
/// down (SIGINT, SIGTERM, or `Request::Shutdown`). Binds the control socket
/// deterministically derived from `root` (`daemon::socket`); fails if
/// another daemon is already listening there.
pub async fn serve(root: ProjectRoot) -> crate::Result<()> {
    let socket_path = socket::control_socket_path(root.path());
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent).map_err(|source| Error::io(parent, source))?;
    }

    // A live daemon already answering here is a real conflict, not
    // staleness — only remove the socket file after confirming nothing is
    // listening on it.
    if UnixStream::connect(&socket_path).await.is_ok() {
        return Err(Error::io(
            &socket_path,
            std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "an lspd daemon is already running for this project",
            ),
        ));
    }
    socket::remove_stale(&socket_path).map_err(|source| Error::io(&socket_path, source))?;

    let listener =
        UnixListener::bind(&socket_path).map_err(|source| Error::io(&socket_path, source))?;
    info!(project = %root, socket = %socket_path.display(), "lspd daemon listening");

    let sockets_dir = socket::proxy_sockets_dir(root.path());
    lsp_manager::ensure_sockets_dir(&sockets_dir)?;

    let config = EffectiveConfig::resolve(&root)?;
    let watch_rules = config.watch_rules()?;
    let watcher = Watcher::start(root.path())?;
    let watch_runtime = Arc::new(WatchRuntime::new(watch_rules));

    let environment = mise::effective_environment(&MiseProvider, root.path());
    tokio::spawn(watch_runtime::run(
        watch_runtime.clone(),
        watcher.subscribe(),
        root.path().to_path_buf(),
        environment,
    ));

    let state = Arc::new(DaemonState {
        handshake: Handshake::new(root.path().to_path_buf()),
        root,
        lsp_manager: LspManager::new(sockets_dir),
        watch_runtime,
        watcher,
        shutdown: Notify::new(),
    });

    run_accept_loop(&listener, &state).await;

    let _ = std::fs::remove_file(&socket_path);
    info!("lspd daemon shut down");
    Ok(())
}

async fn run_accept_loop(listener: &UnixListener, state: &Arc<DaemonState>) {
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("installing a SIGTERM handler should not fail");

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                match accepted {
                    Ok((stream, _addr)) => {
                        let state = state.clone();
                        tokio::spawn(async move {
                            if let Err(error) = handle_connection(state, stream).await {
                                warn!(?error, "control connection ended with an error");
                            }
                        });
                    }
                    Err(error) => warn!(%error, "failed to accept a control connection"),
                }
            }
            _ = tokio::signal::ctrl_c() => {
                info!("received SIGINT, shutting down");
                break;
            }
            _ = sigterm.recv() => {
                info!("received SIGTERM, shutting down");
                break;
            }
            () = state.shutdown.notified() => {
                info!("shutdown requested over the control socket");
                break;
            }
        }
    }
}

async fn handle_connection(state: Arc<DaemonState>, stream: UnixStream) -> crate::Result<()> {
    use tokio::io::{AsyncBufReadExt, BufReader};

    let mut connection = BufReader::new(stream);
    let mut line = String::new();

    // The handshake must be the first exchange on any connection (spec
    // §16) — anything else is a protocol violation, reported and the
    // connection closed rather than silently accepted.
    line.clear();
    if connection
        .read_line(&mut line)
        .await
        .map_err(|source| Error::io("<client>", source))?
        == 0
    {
        return Ok(());
    }
    match serde_json::from_str::<Request>(line.trim_end()) {
        Ok(Request::Handshake) => {
            send(
                &mut connection,
                &Response::Handshake(state.handshake.clone()),
            )
            .await?;
        }
        _ => {
            send(
                &mut connection,
                &Response::Error {
                    code: "lspd::daemon::handshake_required".to_string(),
                    message: "the first request on a connection must be a handshake".to_string(),
                },
            )
            .await?;
            return Ok(());
        }
    }

    loop {
        line.clear();
        let bytes_read = connection
            .read_line(&mut line)
            .await
            .map_err(|source| Error::io("<client>", source))?;
        if bytes_read == 0 {
            return Ok(());
        }

        let request: Request = match serde_json::from_str(line.trim_end()) {
            Ok(request) => request,
            Err(error) => {
                send(
                    &mut connection,
                    &Response::Error {
                        code: "lspd::daemon::malformed_request".to_string(),
                        message: error.to_string(),
                    },
                )
                .await?;
                continue;
            }
        };

        if matches!(request, Request::Subscribe { .. }) {
            let Request::Subscribe { paths } = request else {
                unreachable!()
            };
            return stream_events(state, connection, paths).await;
        }

        let response = dispatch(&state, request).await;
        send(&mut connection, &response).await?;
    }
}

async fn dispatch(state: &Arc<DaemonState>, request: Request) -> Response {
    match request {
        Request::Handshake => Response::Handshake(state.handshake.clone()),
        Request::LspStatus { name } => Response::LspStatus(state.lsp_manager.status(&name).await),
        Request::LspEnsure { name } => match resolve_definition(&state.root, &name) {
            Ok(definition) => match state.lsp_manager.ensure(&definition).await {
                Ok(status) => Response::LspEnsured { name, status },
                Err(error) => error_response(&error),
            },
            Err(error) => error_response(&error),
        },
        Request::LspEndpoint { name } => match resolve_definition(&state.root, &name) {
            Ok(definition) => match state.lsp_manager.endpoint(&definition).await {
                Ok(socket) => Response::LspEndpoint { socket },
                Err(error) => error_response(&error),
            },
            Err(error) => error_response(&error),
        },
        Request::WatchList => Response::WatchList {
            rules: state.watch_runtime.list().await,
        },
        Request::WatchSet { name, enabled } => {
            if state.watch_runtime.set_enabled(&name, enabled).await {
                Response::Ack
            } else {
                error_response(&Error::UnknownLsp { name })
            }
        }
        Request::Shutdown => {
            state.shutdown.notify_one();
            Response::Ack
        }
        Request::Subscribe { .. } => unreachable!("handled in handle_connection before dispatch"),
    }
}

fn resolve_definition(
    root: &ProjectRoot,
    name: &str,
) -> crate::Result<crate::config::lsp::ResolvedLspDefinition> {
    // Re-resolved on every call rather than cached: configuration is cheap
    // to load (a handful of small TOML files) and this guarantees `ensure`
    // always launches with the current configuration, including any edit
    // made to `lspd.local.toml` since the daemon started.
    EffectiveConfig::resolve(root)?.resolve_lsp(name)
}

fn error_response(error: &crate::Error) -> Response {
    let code = miette::Diagnostic::code(error)
        .map(|c| c.to_string())
        .unwrap_or_else(|| "lspd::error::unknown".to_string());
    Response::Error {
        code,
        message: error.to_string(),
    }
}

async fn send(
    connection: &mut (impl tokio::io::AsyncWrite + Unpin),
    response: &Response,
) -> crate::Result<()> {
    use tokio::io::AsyncWriteExt;
    let mut line =
        serde_json::to_string(response).map_err(|source| Error::MalformedJson { source })?;
    line.push('\n');
    connection
        .write_all(line.as_bytes())
        .await
        .map_err(|source| Error::io("<client>", source))
}

async fn stream_events(
    state: Arc<DaemonState>,
    mut connection: tokio::io::BufReader<UnixStream>,
    paths: Vec<String>,
) -> crate::Result<()> {
    let globset = build_globset(&paths);
    let mut receiver = state.watcher.subscribe();

    loop {
        match receiver.recv().await {
            Ok(event) => {
                let matches = globset
                    .as_ref()
                    .is_none_or(|set| event.paths().iter().any(|p| set.is_match(p)));
                if matches
                    && send(&mut connection, &Response::Event(event))
                        .await
                        .is_err()
                {
                    // The client disconnected — this is the normal way a
                    // `lspd events` subscription ends.
                    return Ok(());
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
        }
    }
}

fn build_globset(paths: &[String]) -> Option<globset::GlobSet> {
    if paths.is_empty() {
        return None;
    }
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in paths {
        if let Ok(glob) = globset::Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder.build().ok()
}

/// Reused by `commands::daemon` to locate the log file a detached daemon was
/// started with, for `lspd status`-style diagnostics.
#[must_use]
pub fn log_path_for(root: &Path) -> std::path::PathBuf {
    let dir = directories::ProjectDirs::from("", "", "lspd")
        .map(|dirs| {
            dirs.state_dir()
                .unwrap_or_else(|| dirs.cache_dir())
                .to_path_buf()
        })
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("{}.log", socket::project_hash(root)))
}
