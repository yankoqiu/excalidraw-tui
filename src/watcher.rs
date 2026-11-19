//! Parent-directory file watching with debounce.
//!
//! Watching the containing directory (instead of the file inode) keeps working
//! when editors save via `write temp file -> rename over target`.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use anyhow::{Context, Result};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};

/// Debounced watcher for a single file.
pub struct Watcher {
    rx: Receiver<PathBuf>,
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

impl Watcher {
    pub fn new(file: &Path, debounce: Duration) -> Result<Self> {
        let parent = file
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let parent = parent.canonicalize().with_context(|| {
            format!(
                "failed to resolve watch directory {} (does it exist?)",
                parent.display()
            )
        })?;
        let file_name = file
            .file_name()
            .with_context(|| format!("{} has no file name", file.display()))?
            .to_os_string();

        let (tx, rx) = mpsc::channel();
        let mut debouncer = new_debouncer(debounce, move |result: DebounceEventResult| {
            let Ok(events) = result else {
                return;
            };
            if events
                .into_iter()
                .any(|event| event.path.file_name() == Some(file_name.as_os_str()))
            {
                let _ = tx.send(PathBuf::from(&file_name));
            }
        })
        .context("failed to start file watcher")?;

        debouncer
            .watcher()
            .watch(&parent, RecursiveMode::NonRecursive)
            .with_context(|| format!("failed to watch directory {}", parent.display()))?;

        Ok(Self {
            rx,
            _debouncer: debouncer,
        })
    }

    /// Non-blocking check for a debounced change notification.
    pub fn try_recv(&self) -> Option<PathBuf> {
        self.rx.try_recv().ok()
    }
}
