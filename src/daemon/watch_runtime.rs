//! Executes watch rules against the shared event stream (spec §21).
//!
//! Subscribes to the same [`super::events::FsEvent`] broadcast every other
//! consumer does — a watch rule is just one more subscriber that happens to
//! run a command, not a different kind of event source (spec §20).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use globset::{Glob, GlobSet, GlobSetBuilder};
use tokio::sync::{Mutex, broadcast};
use tracing::{info, warn};

use super::events::FsEvent;
use super::protocol::WatchRuleInfo;
use crate::config::watch::{EventKind, WatchRule};

struct CompiledRule {
    rule: WatchRule,
    globset: GlobSet,
}

/// Owns every watch rule for one daemon instance, plus the runtime-only
/// enable/disable overrides `lspd watch enable/disable` sets. Overrides
/// are intentionally not persisted to any configuration file — they apply
/// "for the running daemon's lifetime" (spec's local-override requirement,
/// interpreted here as session-scoped rather than disk-scoped; see
/// `docs/adr/0002-config-layering-and-merge-semantics.md`).
pub struct WatchRuntime {
    rules: Vec<CompiledRule>,
    overrides: Mutex<HashMap<String, bool>>,
    // `Arc` rather than a plain field: a triggered rule's completion task is
    // `tokio::spawn`ed (so `handle_event` returns immediately) and needs to
    // remove its own name from this set independently of `&self`'s lifetime.
    running: Arc<Mutex<HashSet<String>>>,
}

impl WatchRuntime {
    /// Compiles every resolved rule's globs once, up front, so matching an
    /// incoming event never re-parses a pattern.
    #[must_use]
    pub fn new(rules: Vec<WatchRule>) -> Self {
        let compiled = rules
            .into_iter()
            .map(|rule| {
                let mut builder = GlobSetBuilder::new();
                for pattern in &rule.paths {
                    match Glob::new(pattern) {
                        Ok(glob) => {
                            builder.add(glob);
                        }
                        Err(error) => warn!(rule = %rule.name, pattern, %error, "invalid watch glob, ignoring"),
                    }
                }
                let globset = builder.build().unwrap_or_else(|_| GlobSet::empty());
                CompiledRule { rule, globset }
            })
            .collect();

        Self {
            rules: compiled,
            overrides: Mutex::new(HashMap::new()),
            running: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Sets a runtime override for `name`. Returns `false` if no rule by
    /// that name exists (the CLI reports this as `UnknownLsp`-style error
    /// upstream — reusing that shape for "unknown watch rule" as well would
    /// blur two different things, so callers map this to their own error).
    pub async fn set_enabled(&self, name: &str, enabled: bool) -> bool {
        if !self.rules.iter().any(|r| r.rule.name == name) {
            return false;
        }
        self.overrides
            .lock()
            .await
            .insert(name.to_string(), enabled);
        true
    }

    /// Lists every rule with its effective (override-folded) enabled state.
    pub async fn list(&self) -> Vec<WatchRuleInfo> {
        let overrides = self.overrides.lock().await;
        self.rules
            .iter()
            .map(|r| WatchRuleInfo {
                name: r.rule.name.clone(),
                paths: r.rule.paths.clone(),
                enabled: *overrides.get(&r.rule.name).unwrap_or(&r.rule.enabled),
            })
            .collect()
    }

    async fn is_enabled(&self, name: &str, config_default: bool) -> bool {
        *self
            .overrides
            .lock()
            .await
            .get(name)
            .unwrap_or(&config_default)
    }

    /// Matches `event` against every rule and spawns commands for the ones
    /// that fire. A rule already mid-execution is skipped for this event
    /// rather than queued (spec-adjacent "do not replace Mise's task
    /// system" caution: overlapping runs of the same task are the task
    /// runner's problem to serialize, not LSPD's).
    pub async fn handle_event(&self, event: &FsEvent, env: &HashMap<String, String>, root: &Path) {
        let event_kind = to_event_kind(event);
        for compiled in &self.rules {
            if !compiled.rule.events.contains(&event_kind) {
                continue;
            }
            if !event.paths().iter().any(|p| compiled.globset.is_match(p)) {
                continue;
            }
            if !self
                .is_enabled(&compiled.rule.name, compiled.rule.enabled)
                .await
            {
                continue;
            }
            self.trigger(compiled, env, root).await;
        }
    }

    async fn trigger(&self, compiled: &CompiledRule, env: &HashMap<String, String>, root: &Path) {
        let name = compiled.rule.name.clone();
        {
            let mut running = self.running.lock().await;
            if !running.insert(name.clone()) {
                info!(rule = %name, "skipped: a previous run is still in flight");
                return;
            }
        }

        let (executable, args) = compiled.rule.command.split();
        let executable = executable.to_string();
        let args = args.to_vec();
        let env = env.clone();
        let root = root.to_path_buf();
        let running = self.running.clone();
        let rule_name = name.clone();

        tokio::spawn(async move {
            let result = tokio::process::Command::new(&executable)
                .args(&args)
                .envs(&env)
                .current_dir(&root)
                .stdin(Stdio::null())
                .status()
                .await;

            match result {
                Ok(status) if status.success() => {
                    info!(rule = %rule_name, "watch command succeeded")
                }
                Ok(status) => warn!(rule = %rule_name, %status, "watch command failed"),
                Err(error) => {
                    warn!(rule = %rule_name, %error, "watch command could not be spawned")
                }
            }

            running.lock().await.remove(&rule_name);
        });
    }
}

fn to_event_kind(event: &FsEvent) -> EventKind {
    match event {
        FsEvent::Created { .. } => EventKind::Created,
        FsEvent::Modified { .. } => EventKind::Modified,
        FsEvent::Deleted { .. } => EventKind::Deleted,
        FsEvent::Renamed { .. } => EventKind::Renamed,
    }
}

/// Drives a [`WatchRuntime`] from a broadcast receiver until the sender
/// side (the `Watcher` in `daemon::events`) is dropped.
pub async fn run(
    runtime: Arc<WatchRuntime>,
    mut events: broadcast::Receiver<FsEvent>,
    root: PathBuf,
    env: HashMap<String, String>,
) {
    loop {
        match events.recv().await {
            Ok(event) => runtime.handle_event(&event, &env, &root).await,
            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                warn!(skipped, "watch runtime lagged behind the event stream");
            }
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::lsp::CommandSpec;
    use tempfile::TempDir;

    fn rule(name: &str, pattern: &str, events: Vec<EventKind>) -> WatchRule {
        WatchRule {
            name: name.to_string(),
            paths: vec![pattern.to_string()],
            events,
            command: CommandSpec::Executable("true".to_string()),
            enabled: true,
        }
    }

    #[tokio::test]
    async fn a_new_rule_is_enabled_by_its_configured_default() {
        let runtime = WatchRuntime::new(vec![rule("tests", "**/*.rs", vec![EventKind::Modified])]);
        let list = runtime.list().await;
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
    }

    #[tokio::test]
    async fn disabling_an_unknown_rule_reports_failure() {
        let runtime = WatchRuntime::new(vec![]);
        assert!(!runtime.set_enabled("does-not-exist", false).await);
    }

    #[tokio::test]
    async fn a_runtime_override_is_reflected_in_the_listing_without_touching_config() {
        let runtime = WatchRuntime::new(vec![rule("tests", "**/*.rs", vec![EventKind::Modified])]);
        assert!(runtime.set_enabled("tests", false).await);
        let list = runtime.list().await;
        assert!(!list[0].enabled);
    }

    #[tokio::test]
    async fn a_disabled_rule_does_not_trigger_on_a_matching_event() {
        let tmp = TempDir::new().unwrap();
        let marker = tmp.path().join("marker");
        let command = format!("touch {}", marker.display());
        let runtime = WatchRuntime::new(vec![WatchRule {
            name: "touch-marker".to_string(),
            paths: vec!["*.rs".to_string()],
            events: vec![EventKind::Modified],
            command: CommandSpec::ExecutableWithArgs(
                command.split_whitespace().map(String::from).collect(),
            ),
            enabled: false,
        }]);

        runtime
            .handle_event(
                &FsEvent::Modified {
                    path: "src/main.rs".into(),
                },
                &HashMap::new(),
                tmp.path(),
            )
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(!marker.exists());
    }

    #[tokio::test]
    async fn an_enabled_rule_runs_its_command_on_a_matching_event() {
        let tmp = TempDir::new().unwrap();
        let marker = tmp.path().join("marker");
        let runtime = WatchRuntime::new(vec![WatchRule {
            name: "touch-marker".to_string(),
            paths: vec!["*.rs".to_string()],
            events: vec![EventKind::Modified],
            command: CommandSpec::ExecutableWithArgs(vec![
                "touch".to_string(),
                marker.display().to_string(),
            ]),
            enabled: true,
        }]);

        runtime
            .handle_event(
                &FsEvent::Modified {
                    path: "src/main.rs".into(),
                },
                &HashMap::new(),
                tmp.path(),
            )
            .await;
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        assert!(marker.exists());
    }
}
