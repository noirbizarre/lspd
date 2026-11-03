//! The shared filesystem event stream (spec §19, §20).
//!
//! One watcher per daemon instance, rooted at the project root, broadcasting
//! normalized [`FsEvent`]s to every subscribed connection. Deliberately
//! independent of watch rules (`daemon::watch_runtime`): this module knows
//! nothing about commands, only about producing events — "LSPD emits
//! events. Consumers decide what to do with them." (spec §20).

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tracing::{debug, warn};

/// Directories never worth watching in any project: version control
/// metadata, build output, and dependency caches that routinely see
/// thousands of writes a watcher would otherwise have to debounce.
/// Overridable per project via `[events].ignore` in configuration (not yet
/// wired into `config::mod` — tracked as follow-up; the default list alone
/// already covers the common case).
const DEFAULT_IGNORE: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    "dist",
    "build",
    ".lspd",
];

/// A normalized filesystem event, independent of which OS backend produced
/// it. Renames are atomic (`Renamed`) whenever the backend can correlate the
/// pair; otherwise they surface as a `Deleted` + `Created` pair, same as any
/// other unrelated pair of events (spec §19).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum FsEvent {
    /// `file.created`.
    #[serde(rename = "file.created")]
    Created {
        /// Path relative to the project root.
        path: PathBuf,
    },
    /// `file.modified`.
    #[serde(rename = "file.modified")]
    Modified {
        /// Path relative to the project root.
        path: PathBuf,
    },
    /// `file.deleted`.
    #[serde(rename = "file.deleted")]
    Deleted {
        /// Path relative to the project root.
        path: PathBuf,
    },
    /// `file.renamed`, atomic when the backend supports pairing rename
    /// halves.
    #[serde(rename = "file.renamed")]
    Renamed {
        /// The previous path, relative to the project root.
        old_path: PathBuf,
        /// The new path, relative to the project root.
        new_path: PathBuf,
    },
}

impl FsEvent {
    /// The path(s) this event concerns, used for glob filtering. A rename
    /// matches on either side, so a rule watching `*.py` still fires when a
    /// `.py` file is renamed to `.pyc` (a "this stopped being a match"
    /// signal a watcher would otherwise miss).
    #[must_use]
    pub fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Created { path } | Self::Modified { path } | Self::Deleted { path } => {
                vec![path.as_path()]
            }
            Self::Renamed { old_path, new_path } => vec![old_path.as_path(), new_path.as_path()],
        }
    }
}

/// Owns the background watcher for one daemon instance. Dropping this stops
/// watching (the debouncer's destructor tears down the OS watch).
pub struct Watcher {
    // Kept alive only for its `Drop` impl; never read again after
    // construction. The broadcast sender is what matters going forward.
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    sender: broadcast::Sender<FsEvent>,
}

impl Watcher {
    /// Starts watching `root` recursively, applying the default ignore list
    /// (`.git`, `target`, `node_modules`, and similar).
    /// Events are normalized and pushed onto an internal
    /// [`broadcast`] channel; [`Self::subscribe`] hands out receivers.
    pub fn start(root: &Path) -> crate::Result<Self> {
        let (sender, _) = broadcast::channel(1024);
        let forward = sender.clone();
        let root_for_events = root.to_path_buf();

        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    for event in events {
                        for normalized in normalize(&root_for_events, &event.event) {
                            // No receivers is the common case (no client has
                            // called `events` recently) — that is not an
                            // error, broadcast just drops the value.
                            let _ = forward.send(normalized);
                        }
                    }
                }
                Err(errors) => {
                    for error in errors {
                        warn!(%error, "filesystem watch error");
                    }
                }
            },
        )
        .map_err(|source| crate::Error::io(root, std::io::Error::other(source)))?;

        debouncer
            .watch(root, RecursiveMode::Recursive)
            .map_err(|source| crate::Error::io(root, std::io::Error::other(source)))?;

        debug!(root = %root.display(), "filesystem watcher started");

        Ok(Self {
            _debouncer: debouncer,
            sender,
        })
    }

    /// Hands out a new receiver. Each subscriber gets every event emitted
    /// from this point forward — `broadcast` has no history/replay.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<FsEvent> {
        self.sender.subscribe()
    }
}

/// Converts one `notify` event into zero or more [`FsEvent`]s (usually one;
/// a `notify` "both" rename event becomes exactly one `Renamed`), applying
/// [`DEFAULT_IGNORE`] and relativizing paths to `root`.
fn normalize(root: &Path, event: &notify::Event) -> Vec<FsEvent> {
    use notify::EventKind;
    use notify::event::{ModifyKind, RenameMode};

    let relativize = |p: &Path| -> Option<PathBuf> {
        let rel = p.strip_prefix(root).unwrap_or(p);
        if is_ignored(rel) {
            None
        } else {
            Some(rel.to_path_buf())
        }
    };

    match &event.kind {
        EventKind::Create(_) => event
            .paths
            .iter()
            .filter_map(|p| relativize(p))
            .map(|path| FsEvent::Created { path })
            .collect(),
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if event.paths.len() == 2 => {
            match (relativize(&event.paths[0]), relativize(&event.paths[1])) {
                (Some(old_path), Some(new_path)) => vec![FsEvent::Renamed { old_path, new_path }],
                // One side is ignored (e.g. renamed into `target/`): report
                // it as whichever half remains observable, rather than
                // silently dropping the whole event.
                (Some(old_path), None) => vec![FsEvent::Deleted { path: old_path }],
                (None, Some(new_path)) => vec![FsEvent::Created { path: new_path }],
                (None, None) => vec![],
            }
        }
        EventKind::Modify(_) => event
            .paths
            .iter()
            .filter_map(|p| relativize(p))
            .map(|path| FsEvent::Modified { path })
            .collect(),
        EventKind::Remove(_) => event
            .paths
            .iter()
            .filter_map(|p| relativize(p))
            .map(|path| FsEvent::Deleted { path })
            .collect(),
        _ => vec![],
    }
}

fn is_ignored(relative_path: &Path) -> bool {
    relative_path
        .components()
        .any(|c| DEFAULT_IGNORE.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_a_default_ignored_directory_is_ignored() {
        assert!(is_ignored(Path::new("target/debug/lspd")));
        assert!(is_ignored(Path::new("frontend/node_modules/lib/index.js")));
    }

    #[test]
    fn a_path_outside_any_ignored_directory_is_not_ignored() {
        assert!(!is_ignored(Path::new("src/main.rs")));
    }

    #[test]
    fn a_renamed_event_serializes_to_the_documented_file_dot_renamed_shape() {
        // Pinned literally, not just round-tripped: the spec (§19) requires
        // the wire tag to read `file.renamed`, not `renamed` — a plain
        // `rename_all = "snake_case"` would silently drop the `file.` prefix
        // and still round-trip cleanly against itself.
        let event = FsEvent::Renamed {
            old_path: PathBuf::from("src/foo.py"),
            new_path: PathBuf::from("src/bar.py"),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"event":"file.renamed","old_path":"src/foo.py","new_path":"src/bar.py"}"#
        );
    }

    #[test]
    fn fs_event_paths_reports_both_sides_of_a_rename() {
        let event = FsEvent::Renamed {
            old_path: PathBuf::from("src/foo.py"),
            new_path: PathBuf::from("src/bar.py"),
        };
        assert_eq!(
            event.paths(),
            vec![Path::new("src/foo.py"), Path::new("src/bar.py")]
        );
    }
}
