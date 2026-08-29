//! LSP service lifecycle and stdio proxy (spec §14, §15, §17).
//!
//! Distinguishes a *definition* (`config::lsp::ResolvedLspDefinition`, just
//! configuration) from a *running instance* (this module): the same
//! definition name can be `ensure`d repeatedly and reuses one process, never
//! starting a duplicate (spec §15). Proxy access is one exclusive client at
//! a time per instance (`docs/adr/0005-single-exclusive-lsp-proxy-client.md`)
//! — safe multiplexing of a stateful LSP session across clients is out of
//! scope for this MVP.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tokio::net::UnixListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tracing::{info, warn};

use crate::config::lsp::ResolvedLspDefinition;
use crate::error::Error;

/// A running (or previously running) instance's lifecycle state, computed on
/// demand from the child process rather than tracked by a background
/// supervisor — simpler, and always consistent with reality since it can
/// never drift from what `try_wait` actually reports.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum LspStatus {
    /// The process is alive.
    Running,
    /// `ensure` has never been called for this name in this daemon's
    /// lifetime.
    Stopped,
    /// The process exited (cleanly or not) and has not been re-`ensure`d.
    Crashed,
}

/// What `lsp status`/`lsp ensure` reports.
#[derive(Debug, Clone, Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct LspStatusInfo {
    /// The definition's name.
    pub name: String,
    /// Current lifecycle state.
    pub status: LspStatus,
    /// The OS process id, if running.
    pub pid: Option<u32>,
}

struct Pipes {
    stdin: ChildStdin,
    stdout: ChildStdout,
}

struct LspInstance {
    child: Child,
    // Not read again after `spawn()` stores them — both were already handed
    // to `run_proxy`'s task via `.clone()` before being kept here. Retained
    // on the struct anyway (rather than dropped) so a future explicit `lsp
    // stop` can reach the same pipes/flag without restructuring this type.
    #[allow(dead_code)]
    pipes: Arc<Mutex<Pipes>>,
    proxy_socket: PathBuf,
    #[allow(dead_code)]
    attached: Arc<AtomicBool>,
    // Aborting this stops the proxy accept loop when the instance is
    // replaced (e.g. respawned after a crash) — otherwise two listeners
    // would race to bind the same socket path.
    proxy_task: tokio::task::AbortHandle,
}

impl Drop for LspInstance {
    fn drop(&mut self) {
        self.proxy_task.abort();
        let _ = std::fs::remove_file(&self.proxy_socket);
    }
}

/// Owns every LSP instance for one project, keyed by definition name.
pub struct LspManager {
    /// Where per-instance proxy sockets are created, e.g.
    /// `$XDG_RUNTIME_DIR/projectd/<hash>/`.
    sockets_dir: PathBuf,
    instances: Mutex<HashMap<String, LspInstance>>,
}

impl LspManager {
    /// Creates a manager that places proxy sockets under `sockets_dir`,
    /// which must already exist.
    #[must_use]
    pub fn new(sockets_dir: PathBuf) -> Self {
        Self {
            sockets_dir,
            instances: Mutex::new(HashMap::new()),
        }
    }

    fn proxy_socket_path(&self, name: &str) -> PathBuf {
        self.sockets_dir.join(format!("{name}.lsp.sock"))
    }

    /// Reports the current status of `name`, without starting anything.
    pub async fn status(&self, name: &str) -> LspStatusInfo {
        let mut instances = self.instances.lock().await;
        Self::status_of(&mut instances, name)
    }

    fn status_of(instances: &mut HashMap<String, LspInstance>, name: &str) -> LspStatusInfo {
        match instances.get_mut(name) {
            None => LspStatusInfo {
                name: name.to_string(),
                status: LspStatus::Stopped,
                pid: None,
            },
            Some(instance) => match instance.child.try_wait() {
                Ok(None) => LspStatusInfo {
                    name: name.to_string(),
                    status: LspStatus::Running,
                    pid: instance.child.id(),
                },
                Ok(Some(_)) | Err(_) => LspStatusInfo {
                    name: name.to_string(),
                    status: LspStatus::Crashed,
                    pid: None,
                },
            },
        }
    }

    /// Starts `definition`'s process if it is not already running, reusing
    /// the existing instance otherwise (spec §15). Also (re)binds the
    /// instance's proxy socket.
    pub async fn ensure(&self, definition: &ResolvedLspDefinition) -> crate::Result<LspStatusInfo> {
        let mut instances = self.instances.lock().await;

        let needs_spawn = match instances.get_mut(&definition.name) {
            Some(instance) => !matches!(instance.child.try_wait(), Ok(None)),
            None => true,
        };

        if needs_spawn {
            let instance = self.spawn(definition).await?;
            instances.insert(definition.name.clone(), instance);
        }

        Ok(Self::status_of(&mut instances, &definition.name))
    }

    async fn spawn(&self, definition: &ResolvedLspDefinition) -> crate::Result<LspInstance> {
        let mut command = Command::new(&definition.executable);
        command
            .args(&definition.args)
            .envs(&definition.environment)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            // A dedicated process group so killing this child on daemon
            // shutdown does not also signal the daemon's own group — see
            // `daemon::mod`'s shutdown handling.
            .kill_on_drop(true);

        let mut child = command
            .spawn()
            .map_err(|source| Error::io(&definition.executable, source))?;

        let stdin = child
            .stdin
            .take()
            .expect("spawned with Stdio::piped(), stdin must be present");
        let stdout = child
            .stdout
            .take()
            .expect("spawned with Stdio::piped(), stdout must be present");

        info!(name = %definition.name, pid = ?child.id(), "LSP instance started");

        let proxy_socket = self.proxy_socket_path(&definition.name);
        let _ = std::fs::remove_file(&proxy_socket);
        let listener =
            UnixListener::bind(&proxy_socket).map_err(|source| Error::io(&proxy_socket, source))?;

        let pipes = Arc::new(Mutex::new(Pipes { stdin, stdout }));
        let attached = Arc::new(AtomicBool::new(false));
        let proxy_task = tokio::spawn(run_proxy(
            listener,
            pipes.clone(),
            attached.clone(),
            definition.name.clone(),
        ))
        .abort_handle();

        Ok(LspInstance {
            child,
            pipes,
            proxy_socket,
            attached,
            proxy_task,
        })
    }

    /// Ensures `definition` is running, then returns its proxy socket path.
    pub async fn endpoint(&self, definition: &ResolvedLspDefinition) -> crate::Result<PathBuf> {
        self.ensure(definition).await?;
        let instances = self.instances.lock().await;
        let instance = instances
            .get(&definition.name)
            .expect("ensure() above just inserted or confirmed this entry");
        Ok(instance.proxy_socket.clone())
    }
}

/// Accepts proxy connections for one instance. Only one client may be
/// attached at a time; a second concurrent connection is closed immediately
/// (see the module-level ADR reference) rather than queued or multiplexed.
async fn run_proxy(
    listener: UnixListener,
    pipes: Arc<Mutex<Pipes>>,
    attached: Arc<AtomicBool>,
    name: String,
) {
    loop {
        let (stream, _addr) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => {
                warn!(%name, %error, "LSP proxy accept failed");
                break;
            }
        };

        if attached.swap(true, Ordering::SeqCst) {
            warn!(%name, "rejected a second concurrent LSP proxy client");
            drop(stream);
            continue;
        }

        let pipes = pipes.clone();
        let attached = attached.clone();
        let name = name.clone();
        tokio::spawn(async move {
            let mut pipes = pipes.lock().await;
            // Borrowed apart rather than through one `&mut pipes.{stdin,stdout}`
            // pair, so the two directions can be copied concurrently: `Pipes`
            // holds two independent handles, not one resource `join!` would
            // otherwise need to serialize access to.
            let Pipes { stdin, stdout } = &mut *pipes;
            let (mut client_read, mut client_write) = stream.into_split();
            let to_child = tokio::io::copy(&mut client_read, stdin);
            let to_client = tokio::io::copy(stdout, &mut client_write);
            let _ = tokio::join!(to_child, to_client);
            attached.store(false, Ordering::SeqCst);
            info!(%name, "LSP proxy client detached");
        });
    }
}

/// Ensures the directory a project's per-instance proxy sockets live in
/// exists.
pub fn ensure_sockets_dir(path: &Path) -> crate::Result<()> {
    std::fs::create_dir_all(path).map_err(|source| Error::io(path, source))
}
