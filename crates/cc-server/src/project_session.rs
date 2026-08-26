//! Project session lifecycle: active project, per-project index cache,
//! auto-indexing, watcher, and idle reopen/eviction.
//!
//! This keeps MCP tool handlers focused on protocol dispatch while lifecycle
//! behavior stays behind one deep module seam.

use crate::engine::CodeIndex;
use crate::handlers::{self, SharedCodeIndex};
use crate::watcher::FileWatcher;
use cc_model::config::RepoSizeTier;
use cc_model::CcResult;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

const PROJECT_CACHE_CAPACITY: usize = 16;
const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;

#[derive(Clone)]
struct ProjectServices {
    index: SharedCodeIndex,
}

impl ProjectServices {
    fn new(project_path: Option<&Path>) -> CcResult<Self> {
        Ok(Self {
            index: Arc::new(RwLock::new(CodeIndex::new(project_path)?)),
        })
    }

    fn empty() -> Self {
        Self {
            index: Arc::new(RwLock::new(CodeIndex::empty())),
        }
    }

    fn index(&self) -> SharedCodeIndex {
        self.index.clone()
    }
}

/// Normalize a project path the same way all MCP project-path entry points do.
pub fn normalize_project_path(raw_path: &str) -> PathBuf {
    normalize_path(Path::new(raw_path))
}

fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Deep module for all per-project state used by the MCP server.
#[derive(Clone)]
pub struct ProjectSession {
    active: Arc<tokio::sync::RwLock<ProjectServices>>,
    project_cache: Arc<tokio::sync::Mutex<LruCache<PathBuf, ProjectServices>>>,
    last_activity: Arc<Mutex<Instant>>,
    auto_indexing: Arc<AtomicBool>,
    /// Handle for the active file-watcher background task. When the project
    /// changes this is replaced so the old watcher is stopped.
    watcher_handle: Arc<tokio::sync::Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl ProjectSession {
    pub fn new(project_path: Option<&Path>) -> Self {
        let services = ProjectServices::new(project_path).unwrap_or_else(|e| {
            tracing::warn!("failed to initialize project: {}", e);
            ProjectServices::new(None).unwrap_or_else(|e2| {
                tracing::error!("fatal: cannot create empty CodeIndex either: {}", e2);
                ProjectServices::empty()
            })
        });

        let mut initial_cache = LruCache::new(NonZeroUsize::new(PROJECT_CACHE_CAPACITY).unwrap());
        if let Some(path) = project_path {
            initial_cache.put(normalize_path(path), services.clone());
        }

        Self {
            active: Arc::new(tokio::sync::RwLock::new(services)),
            project_cache: Arc::new(tokio::sync::Mutex::new(initial_cache)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            auto_indexing: Arc::new(AtomicBool::new(false)),
            watcher_handle: Arc::new(tokio::sync::Mutex::new(None)),
        }
    }

    pub async fn active_index(&self) -> SharedCodeIndex {
        self.active.read().await.index()
    }

    pub async fn index_for_project_path(
        &self,
        project_path: Option<&str>,
    ) -> CcResult<SharedCodeIndex> {
        let Some(raw_path) = project_path.filter(|p| !p.trim().is_empty()) else {
            return Ok(self.active_index().await);
        };
        let path = normalize_project_path(raw_path);

        if let Some(index) = {
            let mut cache = self.project_cache.lock().await;
            cache.get(&path).map(ProjectServices::index)
        } {
            // A cached non-active project may have been idle-evicted (close()
            // releases the DB handle but keeps project_path and the build
            // gate). Reopen transparently — same recovery the active index
            // gets in `call_tool` — instead of failing with ProjectNotSet.
            Self::reopen_index_if_closed(&index)?;
            return Ok(index);
        }

        let services = ProjectServices::new(Some(path.as_path()))?;
        let index = services.index();
        self.project_cache.lock().await.put(path, services);
        Ok(index)
    }

    pub async fn set_active_project(&self, project_path: PathBuf) -> CcResult<SharedCodeIndex> {
        let path = normalize_path(&project_path);
        // Reuse the cached services for this path (same policy as
        // `index_for_project_path`): every entry point must share ONE
        // CodeIndex instance per project, otherwise the per-project build
        // gate cannot serialize manual builds against the auto-index/watcher
        // builds of a previously created instance on the same DB.
        let cached = {
            let mut cache = self.project_cache.lock().await;
            cache.get(&path).cloned()
        };
        let services = match cached {
            Some(existing) => existing,
            None => {
                let created = ProjectServices::new(Some(path.as_path()))?;
                self.project_cache
                    .lock()
                    .await
                    .put(path.clone(), created.clone());
                created
            }
        };
        let index = services.index();
        // Same idle-eviction recovery as `index_for_project_path`: a cached
        // entry may have been closed while non-active — reopen before it
        // becomes the active index, so the first tool call works directly.
        Self::reopen_index_if_closed(&index)?;
        *self.active.write().await = services;
        self.start_watcher(path);
        Ok(index)
    }

    pub fn touch_activity(&self) {
        if let Ok(mut ts) = self.last_activity.lock() {
            *ts = Instant::now();
        }
    }

    pub async fn reopen_active_index_if_closed(&self) -> CcResult<()> {
        let index = self.active_index().await;
        Self::reopen_index_if_closed(&index)
    }

    /// Transparently reopen an idle-evicted CodeIndex: read-lock probe of
    /// `is_closed`, then upgrade to the write lock and re-check before
    /// reopening (`reopen` re-runs `set_project` without touching the build
    /// gate, so build serialization survives the close/reopen cycle).
    fn reopen_index_if_closed(index: &SharedCodeIndex) -> CcResult<()> {
        let need_reopen = {
            let rt = handlers::lock_index(index)?;
            rt.is_closed()
        };
        if need_reopen {
            let mut rt = handlers::lock_index_write(index)?;
            if rt.is_closed() {
                if let Err(e) = rt.reopen() {
                    tracing::warn!("failed to reopen index after idle eviction: {}", e);
                }
            }
        }
        Ok(())
    }

    pub fn maybe_auto_index(&self) {
        let auto_indexing = self.auto_indexing.clone();
        let active = self.active.clone();

        if auto_indexing.load(Ordering::SeqCst) {
            return;
        }

        tokio::spawn(async move {
            let index = active.read().await.index();

            let should_index = tokio::task::spawn_blocking({
                let index = index.clone();
                move || {
                    let rt = match index.read() {
                        Ok(rt) => rt,
                        Err(_) => return false,
                    };
                    let project_path = match rt.project_path.as_deref() {
                        Some(p) => p,
                        None => return false,
                    };
                    let config = cc_model::config::load_project_config(project_path);
                    if !config.auto_index.enabled {
                        return false;
                    }
                    // Check whether the DB was freshly created (empty) rather
                    // than checking file existence — IndexDb::open already
                    // creates the file before we get here.
                    rt.needs_initial_index()
                }
            })
            .await
            .unwrap_or(false);

            if !should_index {
                return;
            }
            if auto_indexing
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                return;
            }

            let result = tokio::task::spawn_blocking(move || {
                // Per-project build gate: clone under a brief read lock,
                // release, then try_lock — never block on the gate from a
                // path that may later take the write lock while a gated
                // build is waiting for it. If a manual build is in flight,
                // skip: that build itself produces the initial index.
                let build_gate = match index.read() {
                    Ok(rt) => rt.build_gate(),
                    Err(_) => return,
                };
                let _build_permit = match build_gate.try_lock() {
                    Ok(permit) => permit,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        tracing::debug!("auto-index skipped — another build holds the gate");
                        return;
                    }
                    Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
                };
                // Shared split-build driver: brief read lock for inputs (plus
                // the auto-index file-limit gate, which fires inside the
                // lock-free prepare), heavy prepare with no CodeIndex lock
                // held, then the staged commit — write lock only around
                // phase_write and the delta apply, so readers never see the
                // non-transactional intermediate write state yet keep running
                // through the postprocess compute.
                if let Err(e) = handlers::core::run_split_build(&index, false, true, None) {
                    tracing::warn!("auto-index failed: {}", e);
                }
            })
            .await;

            if let Err(e) = result {
                tracing::warn!("auto-index task panicked: {}", e);
            }
            auto_indexing.store(false, Ordering::SeqCst);
        });
    }

    /// Start a file watcher for the given project path. Stops any previously
    /// running watcher first. The watcher periodically drains pending events
    /// and triggers an incremental index rebuild.
    ///
    /// The watcher is only started when `auto_index.enabled` is `true` in the
    /// project configuration (`.codecortex.json`). This method is intentionally
    /// fire-and-forget — errors in the watcher never propagate to callers.
    pub fn start_watcher(&self, project_path: PathBuf) {
        let watcher_handle = self.watcher_handle.clone();
        let active = self.active.clone();
        let auto_indexing = self.auto_indexing.clone();

        tokio::spawn(async move {
            // Stop previous watcher if any.
            {
                let mut guard = watcher_handle.lock().await;
                if let Some(handle) = guard.take() {
                    handle.abort();
                }
            }

            // Check config: only start watcher when auto_index is enabled.
            let enabled = {
                let index = active.read().await.index();
                tokio::task::spawn_blocking(move || {
                    let rt = match index.read() {
                        Ok(rt) => rt,
                        Err(_) => return false,
                    };
                    let pp = match rt.project_path.as_deref() {
                        Some(p) => p,
                        None => return false,
                    };
                    cc_model::config::load_project_config(pp).auto_index.enabled
                })
                .await
                .unwrap_or(false)
            };

            if !enabled {
                tracing::info!("watcher: auto_index disabled, skipping file watcher");
                return;
            }

            // Create the FileWatcher on a blocking thread (it uses `notify` internally).
            let path_for_watcher = project_path.clone();
            let watcher_result =
                tokio::task::spawn_blocking(move || FileWatcher::start(&path_for_watcher)).await;

            let watcher = match watcher_result {
                Ok(Ok(w)) => w,
                Ok(Err(e)) => {
                    tracing::warn!("watcher: failed to start file watcher: {}", e);
                    return;
                }
                Err(e) => {
                    tracing::warn!("watcher: spawn_blocking failed: {}", e);
                    return;
                }
            };

            tracing::info!(path = %project_path.display(), "watcher: started file watcher");

            // Wrap watcher in Arc<Mutex> so it can be shared with the poll task
            // and cleaned up on Drop.
            let watcher = Arc::new(std::sync::Mutex::new(Some(watcher)));
            let watcher_for_task = watcher.clone();

            let poll_handle = tokio::spawn({
                let active = active.clone();
                let auto_indexing = auto_indexing.clone();
                async move {
                    let poll_interval = tokio::time::Duration::from_secs(2);
                    loop {
                        tokio::time::sleep(poll_interval).await;

                        // Cheap peek WITHOUT draining: events stay in the
                        // pending set until a build slot is secured, so a
                        // busy tick can never lose them (debounce keeps
                        // coalescing in the meantime).
                        let has_pending = {
                            let guard = match watcher_for_task.lock() {
                                Ok(g) => g,
                                Err(e) => e.into_inner(),
                            };
                            match guard.as_ref() {
                                Some(w) => w.has_pending(),
                                None => break,
                            }
                        };
                        if !has_pending {
                            continue;
                        }

                        // Acquire-before-drain, part 1: the auto_indexing
                        // flag. On failure leave events pending and retry on
                        // the next tick.
                        if auto_indexing
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_err()
                        {
                            tracing::debug!(
                                "watcher: deferring incremental index — already in progress"
                            );
                            continue;
                        }

                        let index = active.read().await.index();
                        let watcher_for_tick = watcher_for_task.clone();
                        let result = tokio::task::spawn_blocking(move || {
                            run_watcher_tick(&index, &watcher_for_tick)
                        })
                        .await;

                        auto_indexing.store(false, Ordering::SeqCst);

                        match result {
                            Ok(WatcherTickOutcome::WatcherGone) => break,
                            Ok(_) => {}
                            Err(e) => {
                                tracing::warn!("watcher: incremental index task panicked: {}", e);
                            }
                        }
                    }
                }
            });

            // Store the poll task handle so it can be aborted when the watcher
            // is replaced or the server shuts down.
            {
                let mut guard = watcher_handle.lock().await;
                *guard = Some(poll_handle);
            }
        });
    }

    pub fn start_initial_project_tasks(&self, project_path: Option<&Path>) {
        if let Some(path) = project_path {
            self.maybe_auto_index();
            self.start_watcher(normalize_path(path));
        }
    }

    /// Stop the watcher poll task and wait for it to terminate.
    ///
    /// Dropping the `JoinHandle` only detaches the task, leaving up to one
    /// poll interval of background work racing process exit. Abort + await
    /// guarantees the poll loop is gone before shutdown proceeds. (A commit
    /// already running inside `spawn_blocking` finishes on its blocking
    /// thread; SQLite transactional writes keep the DB consistent either way.)
    pub async fn shutdown(&self) {
        let handle = { self.watcher_handle.lock().await.take() };
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
            tracing::debug!("watcher poll task stopped");
        }
    }

    pub async fn start_idle_eviction(&self) {
        let last_activity = self.last_activity.clone();
        let active = self.active.clone();
        let project_cache = self.project_cache.clone();
        let idle_timeout_secs = active_idle_timeout_secs(&active).await;
        tokio::spawn(async move {
            let idle_timeout = std::time::Duration::from_secs(idle_timeout_secs);
            let check_interval = std::time::Duration::from_secs(30);
            loop {
                tokio::time::sleep(check_interval).await;
                let elapsed = last_activity
                    .lock()
                    .map(|ts| ts.elapsed())
                    .unwrap_or_default();
                if elapsed >= idle_timeout {
                    let closed = close_idle_instances(&active, &project_cache).await;
                    if closed > 0 {
                        tracing::info!(
                            closed,
                            "idle eviction: closed after {}s",
                            elapsed.as_secs()
                        );
                    }
                }
            }
        });
    }

    /// Get a `RepoSizeTier` from a CodeIndex handle, falling back to Tiny on error.
    pub fn current_tier(index: &SharedCodeIndex) -> RepoSizeTier {
        index
            .read()
            .ok()
            .map(|rt| rt.repo_size_tier())
            .unwrap_or(RepoSizeTier::Tiny)
    }
}

/// How one watcher poll tick ended.
enum WatcherTickOutcome {
    /// The watcher slot is gone — the poll loop should exit.
    WatcherGone,
    /// No build ran this tick (gate busy / nothing pending / lock failure);
    /// any pending events were left untouched for the next tick.
    Skipped,
    /// A drain was followed by an incremental build attempt.
    Completed,
}

/// One watcher poll tick, run on a blocking thread. Ordering invariant:
/// the build gate is acquired BEFORE `drain_pending`, so a drained batch is
/// always followed by a build attempt — events are never droppable after
/// drain. (The caller already holds the `auto_indexing` flag.)
fn run_watcher_tick(
    index: &SharedCodeIndex,
    watcher: &Arc<std::sync::Mutex<Option<FileWatcher>>>,
) -> WatcherTickOutcome {
    // Acquire-before-drain, part 2: the per-project build gate. Clone it
    // under a brief read lock, release, then try_lock — a busy manual build
    // just defers this tick and the pending set stays intact.
    let build_gate = match index.read() {
        Ok(rt) => rt.build_gate(),
        Err(_) => return WatcherTickOutcome::Skipped,
    };
    let _build_permit = match build_gate.try_lock() {
        Ok(permit) => permit,
        Err(std::sync::TryLockError::WouldBlock) => {
            tracing::debug!("watcher: deferring incremental index — another build holds the gate");
            return WatcherTickOutcome::Skipped;
        }
        Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };

    // Only now is it safe to drain: both the auto_indexing flag and the
    // build gate are held, so the drained batch WILL be indexed below.
    let drain = {
        let guard = match watcher.lock() {
            Ok(g) => g,
            Err(e) => e.into_inner(),
        };
        match guard.as_ref() {
            Some(w) => w.drain_pending(),
            None => return WatcherTickOutcome::WatcherGone,
        }
    };
    if drain.is_empty() {
        return WatcherTickOutcome::Skipped;
    }

    tracing::info!(
        changed = drain.changed.len(),
        removed = drain.removed.len(),
        rescan = drain.rescan_needed,
        "watcher: file changes detected, triggering incremental index"
    );

    // Shared split-build driver: brief read lock for inputs, heavy prepare
    // and postprocess compute with no CodeIndex lock held, write lock only
    // around phase_write and the delta apply. The drained event set rides
    // along as the build scope, so the prepare stats/hashes only the touched
    // paths instead of walking the whole tree (safety fallbacks to the full
    // walk are decided inside the scan/diff phase).
    let scope = cc_index::BuildScope {
        changed: drain.changed,
        removed: drain.removed,
    };
    if let Err(e) = handlers::core::run_split_build(index, false, false, Some(&scope)) {
        tracing::warn!("watcher: incremental index failed: {}", e);
    }
    WatcherTickOutcome::Completed
}

/// Close every open `CodeIndex` the session holds: the active instance AND
/// every LRU-cached one. Idle applies to the whole session (`last_activity`
/// is touched by every tool call), and a non-active LRU entry would otherwise
/// keep its DB pool, write connection, and seed/catalog caches alive until 16
/// other projects pushed it out of the cache. Entries stay in the LRU
/// (`close()` keeps `project_path` and the build gate) and reopen
/// transparently on the next use. Returns how many instances were closed.
async fn close_idle_instances(
    active: &Arc<tokio::sync::RwLock<ProjectServices>>,
    project_cache: &Arc<tokio::sync::Mutex<LruCache<PathBuf, ProjectServices>>>,
) -> usize {
    let mut indexes: Vec<SharedCodeIndex> = vec![active.read().await.index()];
    {
        let cache = project_cache.lock().await;
        indexes.extend(cache.iter().map(|(_, services)| services.index()));
    }
    let mut closed = 0usize;
    for index in indexes {
        let mut guard = match index.write() {
            Ok(g) => g,
            Err(_) => continue,
        };
        if guard.project_path.is_some() && !guard.is_closed() {
            guard.close();
            closed += 1;
        }
    }
    closed
}

async fn active_idle_timeout_secs(active: &tokio::sync::RwLock<ProjectServices>) -> u64 {
    let index = active.read().await.index();
    index
        .read()
        .ok()
        .and_then(|rt| {
            rt.project_path.as_deref().map(|p| {
                cc_model::config::load_project_config(p)
                    .auto_index
                    .idle_timeout_secs
            })
        })
        .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    /// Poll until the auto-index build commits (clears `needs_initial_index`).
    async fn wait_for_auto_index(session: &ProjectSession, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let index = session.active_index().await;
            let built = index
                .read()
                .map(|rt| !rt.needs_initial_index())
                .unwrap_or(false);
            if built {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    fn active_generation(index: &SharedCodeIndex) -> cc_db::index_db::IndexGeneration {
        let rt = index.read().unwrap();
        rt.index_db().unwrap().reads().generation().unwrap()
    }

    // The split-lock claim itself (prepare runs without the write lock) is
    // guaranteed by structure: `maybe_auto_index` calls the associated
    // `CodeIndex::prepare_build` between a read-lock `build_inputs` clone and
    // the staged write-lock commit, identical to the watcher poll path. This
    // test pins the observable behavior around that path: a fresh DB gets
    // built, and a fresh index skips the rebuild.
    #[tokio::test(flavor = "multi_thread")]
    async fn maybe_auto_index_builds_then_skips_when_fresh() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

        let session = ProjectSession::new(Some(dir.path()));
        session.maybe_auto_index();
        assert!(
            wait_for_auto_index(&session, Duration::from_secs(30)).await,
            "auto-index did not complete"
        );

        let index = session.active_index().await;
        let stats = {
            let rt = index.read().unwrap();
            rt.index_status().unwrap()
        };
        assert!(stats.indexed_files >= 1, "expected lib.rs to be indexed");
        let generation = active_generation(&index);
        assert!(generation.index_epoch > 0, "build must bump index_epoch");

        // Skip-when-fresh, asserted deterministically on the gating predicate:
        // maybe_auto_index returns before the CAS whenever needs_initial_index
        // is false, so this is the load-bearing check.
        {
            let rt = index.read().unwrap();
            assert!(
                !rt.needs_initial_index(),
                "freshly built index must not need an initial index"
            );
        }
        // Best-effort end-to-end confirmation of the same skip (the sleep only
        // gives the spawned gate task a window in which a buggy rebuild would
        // bump the generation; it cannot false-fail on slow machines).
        session.maybe_auto_index();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(
            active_generation(&index),
            generation,
            "fresh index must not be rebuilt by a second maybe_auto_index"
        );
    }

    /// Idle eviction must close EVERY cached instance, not just the active
    /// one: before the fix, a non-active project parked in the 16-slot LRU
    /// kept its DB pool, write connection, and seed/catalog caches alive
    /// indefinitely (idle eviction only ever touched the active index).
    #[tokio::test(flavor = "multi_thread")]
    async fn close_idle_instances_closes_cached_non_active_projects() {
        let dir_a = TempDir::new().unwrap();
        std::fs::write(dir_a.path().join("lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let dir_b = TempDir::new().unwrap();
        std::fs::write(dir_b.path().join("lib.rs"), "pub fn b() -> i32 { 2 }\n").unwrap();

        // A is created first, then B becomes active; A stays cached non-active.
        let session = ProjectSession::new(Some(dir_a.path()));
        let index_a = session.active_index().await;
        session
            .set_active_project(dir_b.path().to_path_buf())
            .await
            .unwrap();
        let index_b = session.active_index().await;
        assert!(!Arc::ptr_eq(&index_a, &index_b), "B must be a new instance");

        let closed = close_idle_instances(&session.active, &session.project_cache).await;
        assert_eq!(
            closed, 2,
            "both active B and cached non-active A must close"
        );
        assert!(
            index_a.read().unwrap().is_closed(),
            "cached A must be closed"
        );
        assert!(
            index_b.read().unwrap().is_closed(),
            "active B must be closed"
        );

        // The cached entry must still reopen transparently on the next use.
        let reopened = session
            .index_for_project_path(Some(dir_a.path().to_str().unwrap()))
            .await
            .unwrap();
        assert!(
            Arc::ptr_eq(&index_a, &reopened),
            "LRU must reuse instance A"
        );
        assert!(!reopened.read().unwrap().is_closed(), "A must be reopened");
    }

    /// An idle-evicted NON-active project must reopen transparently when the
    /// 16-slot LRU serves its cached CodeIndex: before the fix the closed
    /// instance was returned as-is and every query failed with ProjectNotSet
    /// until the user manually re-ran `index` on that project.
    #[tokio::test(flavor = "multi_thread")]
    async fn cached_project_reopens_transparently_after_idle_eviction() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
        let raw_path = dir.path().to_str().unwrap().to_string();

        let session = ProjectSession::new(Some(dir.path()));
        let index = session
            .index_for_project_path(Some(&raw_path))
            .await
            .unwrap();

        // Simulate idle eviction on the cached instance (close() releases the
        // DB handle but keeps project_path and the build gate).
        {
            let mut rt = index.write().unwrap();
            rt.close();
            assert!(rt.is_closed());
        }

        // The LRU hit must return the SAME instance, reopened and queryable.
        let reopened = session
            .index_for_project_path(Some(&raw_path))
            .await
            .unwrap();
        assert!(
            Arc::ptr_eq(&index, &reopened),
            "cache hit must reuse the per-project CodeIndex instance"
        );
        let rt = reopened.read().unwrap();
        assert!(!rt.is_closed(), "cached closed index must be reopened");
        rt.index_status()
            .expect("reopened index must serve queries directly");
    }

    /// Switching the active project back onto an idle-evicted cache entry
    /// must reopen it (same recovery as `index_for_project_path`): A active
    /// → eviction closes A → switch to B → switch back to A; the returned
    /// index must serve queries directly instead of failing until a manual
    /// re-index.
    #[tokio::test(flavor = "multi_thread")]
    async fn set_active_project_reopens_evicted_cache_entry() {
        let dir_a = TempDir::new().unwrap();
        std::fs::write(dir_a.path().join("lib.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
        let dir_b = TempDir::new().unwrap();
        std::fs::write(dir_b.path().join("lib.rs"), "pub fn b() -> i32 { 2 }\n").unwrap();

        let session = ProjectSession::new(Some(dir_a.path()));
        let index_a = session.active_index().await;

        // Simulate idle eviction on A, then switch the active project away.
        {
            let mut rt = index_a.write().unwrap();
            rt.close();
            assert!(rt.is_closed());
        }
        session
            .set_active_project(dir_b.path().to_path_buf())
            .await
            .unwrap();

        // Switching back must reuse the cached instance, reopened.
        let reactivated = session
            .set_active_project(dir_a.path().to_path_buf())
            .await
            .unwrap();
        assert!(
            Arc::ptr_eq(&index_a, &reactivated),
            "cache hit must reuse the per-project CodeIndex instance"
        );
        let rt = reactivated.read().unwrap();
        assert!(!rt.is_closed(), "reactivated index must be reopened");
        rt.index_status()
            .expect("reactivated index must serve queries directly");
    }

    /// Watcher poll ticks that find the build slot busy must NOT drain (and
    /// thereby drop) pending events: they stay queued and get indexed once
    /// the slot frees up. Before the acquire-before-drain fix, a busy tick
    /// drained first and permanently lost the batch — silent stale index.
    #[tokio::test(flavor = "multi_thread")]
    async fn watcher_busy_tick_preserves_pending_events() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();

        let session = ProjectSession::new(Some(dir.path()));
        session.maybe_auto_index();
        assert!(
            wait_for_auto_index(&session, Duration::from_secs(30)).await,
            "initial auto-index did not complete"
        );
        let index = session.active_index().await;
        let generation_before = active_generation(&index);

        // Simulate a long-running build: while this flag is held, watcher
        // ticks must defer WITHOUT draining the pending events.
        session.auto_indexing.store(true, Ordering::SeqCst);

        session.start_watcher(normalize_path(dir.path()));
        // Give the watcher task time to subscribe before generating events.
        tokio::time::sleep(Duration::from_millis(2500)).await;

        // Several writes so a late-starting watcher still observes at least
        // one; all of them land inside the busy window.
        for _ in 0..3 {
            std::fs::write(
                dir.path().join("extra.rs"),
                "pub fn extra_marker() -> i32 { 7 }\n",
            )
            .unwrap();
            tokio::time::sleep(Duration::from_millis(700)).await;
        }

        // Cover at least two busy poll ticks after the debounce flush; before
        // the fix each of those ticks drained and dropped the pending batch.
        tokio::time::sleep(Duration::from_secs(4)).await;
        assert_eq!(
            active_generation(&index),
            generation_before,
            "no build may run while the auto_indexing slot is busy"
        );

        // Free the slot: the still-pending events must now trigger an
        // incremental index that picks up extra.rs.
        session.auto_indexing.store(false, Ordering::SeqCst);

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut indexed = false;
        while Instant::now() < deadline {
            let count = {
                let rt = index.read().unwrap();
                rt.index_status().map(|s| s.indexed_files).unwrap_or(0)
            };
            if count >= 2 {
                indexed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        assert!(
            indexed,
            "pending watcher events were lost while the build slot was busy"
        );
        session.shutdown().await;
    }
}
