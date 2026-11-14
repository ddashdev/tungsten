//! `tungsten watch` — debounced filesystem watcher that re-runs sync on change.

use anyhow::{Context, Result};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::time::Instant;

use crate::commands::sync::Target;
use crate::commands::sync::paths::walk_root;
use crate::log;
use crate::utils::config::{self, Config};

/// Debounce window: changes within this period after the first event are
/// collapsed into a single sync pass.
const DEBOUNCE_MS: u64 = 300;

pub async fn run(
    config: Config,
    api_key: Option<String>,
    target: Target,
    is_syncing: Arc<AtomicBool>,
) -> Result<()> {
    log!(section, "WATCH MODE");
    log!(info, "Watching for changes (target: {:?})...", target);

    let mut watch_dirs: Vec<String> = config
        .inputs
        .values()
        .map(|input| walk_root(&input.path).to_string())
        .filter(|root| !root.is_empty())
        .collect();
    watch_dirs.sort();
    watch_dirs.dedup();

    if watch_dirs.is_empty() {
        anyhow::bail!(
            "No watchable directories found in tungsten.toml inputs\n  \
             Hint: Make sure your input paths point to real directories"
        );
    }

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(
        move |event| {
            // Non-blocking send; errors are ignored (channel closed = watcher shutting down).
            let _ = tx.send(event);
        },
        notify::Config::default(),
    )
    .context("Failed to create file watcher")?;

    for dir in &watch_dirs {
        let path = std::path::Path::new(dir);
        if path.exists() {
            watcher
                .watch(path, RecursiveMode::Recursive)
                .with_context(|| format!("Failed to watch directory \"{}\"", dir))?;
            log!(info, "Watching \"{}\"", dir);
        } else {
            log!(warn, "Watch directory \"{}\" does not exist. Skipping", dir);
        }
    }

    // A directory's own sidecar sits next to it, outside the recursive watch,
    // so each root's parent is watched too, filtered to that sidecar.
    let scope = WatchScope::new(&watch_dirs);
    for parent in &scope.sidecar_parents {
        if parent.exists() && !watch_dirs.iter().any(|dir| Path::new(dir) == parent) {
            watcher
                .watch(parent, RecursiveMode::NonRecursive)
                .with_context(|| format!("Failed to watch directory \"{}\"", parent.display()))?;
            log!(
                debug,
                "Watching \"{}\" for sidecar changes",
                parent.display()
            );
        }
    }

    log!(
        success,
        "Watching {} director(y/ies). Press Ctrl+C to stop.",
        watch_dirs.len()
    );

    // Ignore our own codegen output files so our writes don't trigger re-syncs.
    let output_files: HashSet<String> = config
        .inputs
        .values()
        .flat_map(|input| {
            let out = input.output_path.clone();
            let dts = crate::core::postsync::codegen::dts_path_for(&out);
            [out, dts]
        })
        .map(|p| p.replace('\\', "/"))
        .collect();

    // Initial sync on startup.
    do_sync(&api_key, target, &config, &is_syncing).await;

    // Event loop with debouncing. When idle we simply await the next file
    // event (no busy-polling); once a change is pending we race the debounce
    // deadline against any further events so bursts collapse into one sync.
    let mut last_event: Option<Instant> = None;
    let mut pending_change = false;

    loop {
        let debounce_deadline = last_event
            .map(|t| t + Duration::from_millis(DEBOUNCE_MS))
            .unwrap_or_else(|| Instant::now() + Duration::from_millis(DEBOUNCE_MS));

        tokio::select! {
            event = rx.recv() => {
                match event {
                    Some(Ok(event)) => {
                        if is_relevant_event(&event, &output_files, &scope) {
                            log!(debug, "Change detected: {:?}", event.kind);
                            last_event = Some(Instant::now());
                            pending_change = true;
                        }
                    }
                    Some(Err(e)) => log!(warn, "Watch error: {}", e),
                    None => {
                        log!(error, "File watcher disconnected");
                        return Ok(());
                    }
                }
            }
            _ = tokio::time::sleep_until(debounce_deadline), if pending_change => {
                log!(debug, "Debounce window elapsed, triggering re-sync");
                pending_change = false;
                last_event = None;
                log!(section, "RE-SYNCING");
                let fresh_config = match tokio::task::spawn_blocking(|| config::load("tungsten.toml"))
                    .await
                {
                    Ok(Ok(c)) => c,
                    Ok(Err(e)) => {
                        log!(warn, "Failed to reload tungsten.toml: {}", e);
                        continue;
                    }
                    Err(e) => {
                        log!(warn, "Config reload panicked: {}", e);
                        continue;
                    }
                };
                do_sync(&api_key, target, &fresh_config, &is_syncing).await;
            }
        }
    }
}

async fn do_sync(
    api_key: &Option<String>,
    target: Target,
    config: &Config,
    is_syncing: &Arc<AtomicBool>,
) {
    is_syncing.store(true, Ordering::Relaxed);

    if let Err(e) = crate::commands::sync::run(config, api_key.as_deref(), target, false).await {
        log!(warn, "Sync error: {:#}", e);
    }

    is_syncing.store(false, Ordering::Relaxed);
}

/// What the watcher covers: the recursive input roots, plus the parent
/// directories watched only for each root's own sidecar.
struct WatchScope {
    roots: Vec<PathBuf>,
    sidecar_parents: Vec<PathBuf>,
    sidecar_names: HashSet<String>,
}

impl WatchScope {
    fn new(watch_dirs: &[String]) -> Self {
        let mut sidecar_parents = Vec::new();
        let mut sidecar_names = HashSet::new();
        for dir in watch_dirs {
            let dir = Path::new(dir);
            let Some(name) = dir.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            sidecar_names.insert(format!("{name}.tmeta"));
            sidecar_names.insert(format!("{name}.tmeta.toml"));
            let parent = match dir.parent() {
                Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
                _ => PathBuf::from("."),
            };
            if !sidecar_parents.contains(&parent) {
                sidecar_parents.push(parent);
            }
        }
        Self {
            roots: watch_dirs
                .iter()
                .filter_map(|dir| Path::new(dir).canonicalize().ok())
                .collect(),
            sidecar_parents,
            sidecar_names,
        }
    }

    /// Events from a sidecar-only parent watch count only for the sidecar itself.
    fn covers(&self, path: &Path) -> bool {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if self.sidecar_names.contains(name) {
            return true;
        }
        let probe = path
            .canonicalize()
            .or_else(|_| path.parent().unwrap_or(path).canonicalize());
        match probe {
            Ok(probe) => self.roots.iter().any(|root| probe.starts_with(root)),
            Err(_) => true,
        }
    }
}

fn is_relevant_event(event: &Event, output_files: &HashSet<String>, scope: &WatchScope) -> bool {
    match event.kind {
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
            // Ignore our own output files so watch mode doesn't re-sync forever.
            // Tungsten writes `tungsten.lock` and codegen output files after
            // every sync; without this filter those writes trigger new events -> loop.
            // Only the configured output paths are filtered, user `.luau` files
            // in watched dirs still trigger a sync.
            if let Some(path) = event.paths.first() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name == "tungsten.lock" || name == "tungsten.lock.toml" {
                    return false;
                }
                if let Some(rel) = path.to_str().map(|p| p.replace('\\', "/"))
                    && output_files.contains(&rel)
                {
                    return false;
                }
                // Ignore the debug output folder entirely.
                if path.starts_with(".tungsten_debug") {
                    return false;
                }
                if !scope.covers(path) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}
