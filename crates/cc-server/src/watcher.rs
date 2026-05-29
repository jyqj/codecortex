//! FileWatcher — filesystem monitoring via notify crate.
//!
//! Features:
//! - Separate changed/removed tracking
//! - Adaptive debounce: interval scales with repository file count
//! - Burst backoff: doubles debounce when event rate spikes (e.g. git checkout)
//! - Git dirty sanity poll: low-frequency `git status` fallback
//! - `should_track` filtering to skip non-indexable paths

use cc_model::CcResult;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

/// Directory segments that should never be indexed.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".tox",
    ".venv",
    "venv",
    ".eggs",
    ".idea",
    ".vscode",
    "dist",
    "build",
];

/// File names that should never be indexed.
const SKIP_FILES: &[&str] = &[".DS_Store", "Thumbs.db"];

// ---------------------------------------------------------------------------
// Adaptive debounce constants
// ---------------------------------------------------------------------------

const DEBOUNCE_BASE_MS: u64 = 500;
const DEBOUNCE_PER_500_FILES_MS: u64 = 100;
const DEBOUNCE_MAX_MS: u64 = 3000;

// ---------------------------------------------------------------------------
// Burst backoff constants
// ---------------------------------------------------------------------------

/// If more than this many events arrive within `BURST_WINDOW`, activate backoff.
const BURST_THRESHOLD: u64 = 20;
/// Window over which burst events are counted (1 second).
const BURST_WINDOW: Duration = Duration::from_secs(1);
/// Maximum debounce during burst backoff.
const BURST_DEBOUNCE_MAX_MS: u64 = 5000;

// ---------------------------------------------------------------------------
// Git sanity poll constants
// ---------------------------------------------------------------------------

/// How often the git sanity poll runs.
const GIT_POLL_INTERVAL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// WatcherConfig
// ---------------------------------------------------------------------------

/// Configuration for [`FileWatcher`].
///
/// Use [`WatcherConfig::default()`] for sensible defaults, or the builder
/// methods to customise behaviour.
#[derive(Debug, Clone)]
pub struct WatcherConfig {
    /// Number of tracked files in the repository. Used to compute adaptive
    /// debounce. If `None`, the watcher calculates it from the first scan.
    pub file_count: Option<usize>,
    /// Explicit debounce override (milliseconds). When set, adaptive
    /// calculation is skipped.
    pub debounce_ms: Option<u64>,
    /// Enable burst backoff. Default: `true`.
    pub burst_backoff: bool,
    /// Enable the git dirty sanity poll. Default: `true`.
    pub git_sanity_poll: bool,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            file_count: None,
            debounce_ms: None,
            burst_backoff: true,
            git_sanity_poll: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Adaptive debounce calculation
// ---------------------------------------------------------------------------

/// Compute the adaptive debounce interval based on repository file count.
///
/// Formula: `min(500 + (file_count / 500) * 100, 3000)` ms
fn compute_adaptive_debounce_ms(file_count: usize) -> u64 {
    let extra = (file_count as u64 / 500) * DEBOUNCE_PER_500_FILES_MS;
    (DEBOUNCE_BASE_MS + extra).min(DEBOUNCE_MAX_MS)
}

/// Count trackable files under `project_path` (non-recursive walk skipping
/// SKIP_DIRS). This is intentionally simple — accuracy is not critical because
/// it only determines the debounce interval.
fn count_project_files(project_path: &Path) -> usize {
    fn walk(dir: &Path, count: &mut usize) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = match entry.file_name().into_string() {
                Ok(n) => n,
                Err(_) => continue,
            };
            if path.is_dir() {
                // Skip well-known non-indexable directories.
                if SKIP_DIRS.contains(&name.as_str()) {
                    continue;
                }
                // Skip hidden directories.
                if name.starts_with('.') && name.len() > 1 {
                    continue;
                }
                walk(&path, count);
            } else if !SKIP_FILES.contains(&name.as_str()) {
                *count += 1;
            }
        }
    }

    let mut count = 0usize;
    walk(project_path, &mut count);
    count
}

// ---------------------------------------------------------------------------
// Pending events
// ---------------------------------------------------------------------------

/// Pending file-system events, separated into changed and removed sets.
/// Uses `HashSet` for O(1) membership checks instead of linear `Vec::contains`.
#[derive(Debug, Default, Clone)]
struct PendingEvents {
    changed: HashSet<String>,
    removed: HashSet<String>,
}

/// Result returned by [`FileWatcher::drain_pending`].
#[derive(Debug, Default, Clone)]
pub struct WatcherDrain {
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

impl WatcherDrain {
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Burst tracker (shared between notify callback and debounce thread)
// ---------------------------------------------------------------------------

/// Tracks event counts within a rolling window to detect burst activity.
struct BurstTracker {
    /// Timestamps of recent events (ring-buffer style, pruned lazily).
    timestamps: Mutex<Vec<Instant>>,
    /// Whether the watcher is currently in burst-backoff mode.
    in_burst: AtomicBool,
}

impl BurstTracker {
    fn new() -> Self {
        Self {
            timestamps: Mutex::new(Vec::with_capacity(64)),
            in_burst: AtomicBool::new(false),
        }
    }

    /// Record one event. Returns `true` if burst threshold is now exceeded.
    fn record_event(&self) -> bool {
        let now = Instant::now();
        let mut ts = self.timestamps.lock().unwrap_or_else(|e| e.into_inner());
        ts.push(now);
        // Prune events older than BURST_WINDOW.
        ts.retain(|t| now.duration_since(*t) < BURST_WINDOW);
        let count = ts.len() as u64;
        let burst = count > BURST_THRESHOLD;
        if burst {
            self.in_burst.store(true, Ordering::Relaxed);
        }
        burst
    }

    /// Check if we are currently in burst mode and optionally clear the flag
    /// (called once per debounce cycle).
    fn take_burst(&self) -> bool {
        self.in_burst.swap(false, Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// FileWatcher
// ---------------------------------------------------------------------------

pub struct FileWatcher {
    _watcher: notify::RecommendedWatcher,
    pending: Arc<Mutex<PendingEvents>>,
    stopped: Arc<AtomicBool>,
    debounce_handle: Option<std::thread::JoinHandle<()>>,
    git_poll_handle: Option<std::thread::JoinHandle<()>>,
}

impl FileWatcher {
    /// Start watching `project_path` recursively with default settings
    /// (adaptive debounce, burst backoff, git sanity poll all enabled).
    pub fn start(project_path: &Path) -> CcResult<Self> {
        Self::start_with_config(project_path, WatcherConfig::default())
    }

    /// Start with full configuration control.
    pub fn start_with_config(project_path: &Path, config: WatcherConfig) -> CcResult<Self> {
        // --- Compute debounce interval ---
        let base_debounce_ms = if let Some(ms) = config.debounce_ms {
            ms
        } else {
            let file_count = config.file_count.unwrap_or_else(|| {
                let count = count_project_files(project_path);
                tracing::info!(
                    file_count = count,
                    "watcher: counted project files for adaptive debounce"
                );
                count
            });
            let ms = compute_adaptive_debounce_ms(file_count);
            tracing::info!(
                debounce_ms = ms,
                "watcher: adaptive debounce interval computed"
            );
            ms
        };

        let pending: Arc<Mutex<PendingEvents>> = Arc::new(Mutex::new(PendingEvents::default()));
        let staging: Arc<Mutex<PendingEvents>> = Arc::new(Mutex::new(PendingEvents::default()));
        let last_event_time: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));

        // Burst tracker (shared between notify callback and debounce thread).
        let burst_tracker = Arc::new(BurstTracker::new());

        // The effective debounce is stored as an atomic so the debounce thread
        // can temporarily inflate it during burst backoff.
        let effective_debounce_ms = Arc::new(AtomicU64::new(base_debounce_ms.max(50)));

        // Clone references for the notify callback.
        let staging_cb = staging.clone();
        let last_event_cb = last_event_time.clone();
        let burst_cb = burst_tracker.clone();
        let project = project_path.to_path_buf();

        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let mut staging = match staging_cb.lock() {
                    Ok(g) => g,
                    Err(e) => e.into_inner(),
                };

                let mut tracked_any = false;
                for path in &event.paths {
                    let rel = match path.strip_prefix(&project) {
                        Ok(r) => r,
                        Err(_) => continue,
                    };
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    if !should_track(&rel_str) {
                        continue;
                    }

                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) => {
                            staging.removed.remove(&rel_str);
                            staging.changed.insert(rel_str);
                        }
                        EventKind::Remove(_) => {
                            staging.changed.remove(&rel_str);
                            staging.removed.insert(rel_str);
                        }
                        _ => {}
                    }
                    tracked_any = true;
                }

                if tracked_any {
                    // Record burst event.
                    burst_cb.record_event();

                    // Update last event timestamp.
                    if let Ok(mut ts) = last_event_cb.lock() {
                        *ts = Some(Instant::now());
                    }
                }
            }
        })
        .map_err(|e| cc_model::CcError::Other(format!("watcher error: {e}")))?;

        let mut watcher = watcher;
        watcher
            .watch(project_path, RecursiveMode::Recursive)
            .map_err(|e| cc_model::CcError::Other(format!("watch error: {e}")))?;

        // --- Spawn debounce flusher thread ---
        let pending_flush = pending.clone();
        let staging_flush = staging.clone();
        let last_event_flush = last_event_time.clone();
        let stopped_flush = stopped.clone();
        let burst_flush = burst_tracker.clone();
        let eff_debounce = effective_debounce_ms.clone();
        let burst_backoff_enabled = config.burst_backoff;

        let debounce_handle = std::thread::Builder::new()
            .name("watcher-debounce".into())
            .spawn(move || {
                let poll_interval = Duration::from_millis(50);
                // Track whether the previous cycle was a burst so we can
                // restore the original debounce after one normal cycle.
                let mut was_burst = false;

                while !stopped_flush.load(Ordering::Relaxed) {
                    std::thread::sleep(poll_interval);

                    // --- Burst backoff logic ---
                    if burst_backoff_enabled {
                        let is_burst = burst_flush.take_burst();
                        if is_burst {
                            let current = eff_debounce.load(Ordering::Relaxed);
                            let doubled = (current * 2).min(BURST_DEBOUNCE_MAX_MS);
                            eff_debounce.store(doubled, Ordering::Relaxed);
                            tracing::debug!(
                                new_debounce_ms = doubled,
                                "watcher: burst detected, debounce doubled"
                            );
                            was_burst = true;
                        } else if was_burst {
                            // Burst is over — restore base debounce.
                            eff_debounce.store(base_debounce_ms.max(50), Ordering::Relaxed);
                            tracing::debug!(
                                debounce_ms = base_debounce_ms,
                                "watcher: burst ended, debounce restored"
                            );
                            was_burst = false;
                        }
                    }

                    let debounce_dur = Duration::from_millis(eff_debounce.load(Ordering::Relaxed));

                    let should_flush = {
                        let ts = last_event_flush.lock().unwrap_or_else(|e| e.into_inner());
                        match *ts {
                            Some(t) => t.elapsed() >= debounce_dur,
                            None => false,
                        }
                    };

                    if should_flush {
                        let mut staging = staging_flush.lock().unwrap_or_else(|e| e.into_inner());
                        if staging.changed.is_empty() && staging.removed.is_empty() {
                            continue;
                        }
                        let flushed = std::mem::take(&mut *staging);
                        // Clear last_event so we don't re-flush immediately.
                        if let Ok(mut ts) = last_event_flush.lock() {
                            *ts = None;
                        }
                        drop(staging);

                        let mut pending = pending_flush.lock().unwrap_or_else(|e| e.into_inner());
                        for path in flushed.changed {
                            pending.removed.remove(&path);
                            pending.changed.insert(path);
                        }
                        for path in flushed.removed {
                            pending.changed.remove(&path);
                            pending.removed.insert(path);
                        }
                    }
                }
            })
            .map_err(|e| cc_model::CcError::Other(format!("spawn debounce thread: {e}")))?;

        // --- Spawn git sanity poll thread (optional) ---
        let git_poll_handle = if config.git_sanity_poll {
            let pending_git = pending.clone();
            let stopped_git = stopped.clone();
            let project_git = project_path.to_path_buf();

            let handle = std::thread::Builder::new()
                .name("watcher-git-poll".into())
                .spawn(move || {
                    git_sanity_poll_loop(&project_git, &pending_git, &stopped_git);
                })
                .map_err(|e| cc_model::CcError::Other(format!("spawn git poll thread: {e}")))?;
            Some(handle)
        } else {
            None
        };

        Ok(Self {
            _watcher: watcher,
            pending,
            stopped,
            debounce_handle: Some(debounce_handle),
            git_poll_handle,
        })
    }

    /// Drain all pending events accumulated since the last call.
    ///
    /// Returns a [`WatcherDrain`] with separate `changed` and `removed` lists
    /// of relative paths (forward-slash separated).
    pub fn drain_pending(&self) -> WatcherDrain {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let events = std::mem::take(&mut *pending);
        WatcherDrain {
            changed: events.changed.into_iter().collect(),
            removed: events.removed.into_iter().collect(),
        }
    }

    /// Stop the watcher and its background threads.
    pub fn stop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
        if let Some(handle) = self.debounce_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.git_poll_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

// ---------------------------------------------------------------------------
// Git sanity poll
// ---------------------------------------------------------------------------

/// Low-frequency polling loop that runs `git status --porcelain` and injects
/// any dirty files that the notify watcher may have missed.
fn git_sanity_poll_loop(
    project_path: &Path,
    pending: &Arc<Mutex<PendingEvents>>,
    stopped: &Arc<AtomicBool>,
) {
    // Use a shorter sleep granularity so we can react to `stopped` faster
    // than every 30 seconds.
    let sleep_granularity = Duration::from_secs(1);
    let mut elapsed = Duration::ZERO;

    while !stopped.load(Ordering::Relaxed) {
        std::thread::sleep(sleep_granularity);
        elapsed += sleep_granularity;

        if elapsed < GIT_POLL_INTERVAL {
            continue;
        }
        elapsed = Duration::ZERO;

        let dirty_files = match git_status_porcelain(project_path) {
            Some(files) => files,
            None => continue,
        };

        if dirty_files.is_empty() {
            continue;
        }

        let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
        let mut backfilled = 0usize;
        for file in dirty_files {
            if !should_track(&file) {
                continue;
            }
            if !pending.changed.contains(&file) && !pending.removed.contains(&file) {
                pending.changed.insert(file);
                backfilled += 1;
            }
        }
        if backfilled > 0 {
            tracing::info!(
                count = backfilled,
                "watcher: git sanity poll backfilled missed dirty files"
            );
        }
    }
}

/// Run `git status --porcelain` and return the list of dirty relative paths.
fn git_status_porcelain(project_path: &Path) -> Option<Vec<String>> {
    let output = match std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project_path)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::debug!(error = %e, "git status --porcelain failed to spawn; skipping dirty poll");
            return None;
        }
    };

    if !output.status.success() {
        tracing::debug!(
            code = output.status.code(),
            stderr = %String::from_utf8_lossy(&output.stderr).trim(),
            "git status --porcelain returned non-zero; skipping dirty poll (not a git repo?)"
        );
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let files: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            // porcelain format: "XY filename" or "XY orig -> renamed"
            let trimmed = line.get(3..)?;
            // Handle renames: "old -> new" — we care about the new name.
            let path = if let Some(pos) = trimmed.find(" -> ") {
                &trimmed[pos + 4..]
            } else {
                trimmed
            };
            let path = path.trim();
            if path.is_empty() {
                None
            } else {
                Some(path.replace('\\', "/"))
            }
        })
        .collect();

    Some(files)
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// Returns `true` if the relative path should be tracked for indexing.
///
/// Excludes:
/// - Paths containing skip-listed directory segments (`.git/`, `node_modules/`, etc.)
/// - Hidden directories (segments starting with `.`) beyond the root level
/// - Known non-indexable file names (`.DS_Store`, `Thumbs.db`)
pub fn should_track(rel_path: &str) -> bool {
    if rel_path.is_empty() {
        return false;
    }

    let parts: Vec<&str> = rel_path.split('/').collect();

    for (idx, part) in parts.iter().enumerate() {
        let is_last = idx == parts.len() - 1;

        // Skip-listed directory names apply to non-leaf segments.
        if !is_last && SKIP_DIRS.contains(part) {
            return false;
        }

        // Hidden directory segments (start with `.` and length > 1), except leaf.
        if !is_last && part.starts_with('.') && part.len() > 1 {
            return false;
        }
    }

    // Check the filename component.
    if let Some(filename) = parts.last() {
        if SKIP_FILES.contains(filename) {
            return false;
        }
    }

    true
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
