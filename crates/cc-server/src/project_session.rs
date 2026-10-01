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
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
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
    /// Weak identity registry also covers live views evicted from the bounded LRU.
    /// Its lock serializes cache misses; blocking initialization/reopen does
    /// not run on the async scheduler or retain a query lock across network.
    live_projects: Arc<tokio::sync::Mutex<HashMap<PathBuf, Weak<RwLock<CodeIndex>>>>>,
    last_activity: Arc<Mutex<Instant>>,
    auto_indexing: Arc<AtomicBool>,
    tasks: Arc<crate::session_tasks::SessionTasks>,
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

        let mut live_projects = HashMap::new();
        if let Some(path) = project_path {
            live_projects.insert(normalize_path(path), Arc::downgrade(&services.index));
        }
        Self {
            active: Arc::new(tokio::sync::RwLock::new(services)),
            project_cache: Arc::new(tokio::sync::Mutex::new(initial_cache)),
            live_projects: Arc::new(tokio::sync::Mutex::new(live_projects)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            auto_indexing: Arc::new(AtomicBool::new(false)),
            tasks: Arc::default(),
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

        Ok(self.services_for_path(path).await?.index())
    }

    pub async fn set_active_project(&self, project_path: PathBuf) -> CcResult<SharedCodeIndex> {
        let path = normalize_path(&project_path);
        let services = self.services_for_path(path.clone()).await?;
        let index = services.index();
        let mut active = self.active.write().await;
        *active = services;
        self.start_watcher(path);
        drop(active);
        Ok(index)
    }

    async fn services_for_path(&self, path: PathBuf) -> CcResult<ProjectServices> {
        // Open cache hits need no cold registry, blocking task or SQL read.
        let cached = self
            .project_cache
            .try_lock()
            .ok()
            .and_then(|mut cache| cache.get(&path).cloned());
        if let Some(services) = cached {
            let open = services.index.try_read().is_ok_and(|rt| !rt.is_closed());
            if open {
                return Ok(services);
            }
        }
        // Move the owned cold guard INTO the worker. Cancelling the awaiting
        // request cannot release it while initialization still runs, and the
        // worker publishes one cached runtime before releasing the guard.
        let mut live = self.live_projects.clone().lock_owned().await;
        let cache = self.project_cache.clone();
        tokio::task::spawn_blocking(move || {
            live.retain(|_, weak| weak.strong_count() != 0);
            let cached = cache.blocking_lock().get(&path).cloned();
            let services = match cached.or_else(|| {
                live.get(&path)
                    .and_then(Weak::upgrade)
                    .map(|index| ProjectServices { index })
            }) {
                Some(existing) => existing,
                None => ProjectServices::new(Some(&path))?,
            };
            Self::reopen_index_if_closed(&services.index())?;
            live.insert(path.clone(), Arc::downgrade(&services.index));
            let displaced = cache.blocking_lock().push(path, services.clone());
            drop(live);
            drop(displaced);
            Ok(services)
        })
        .await
        .map_err(|e| {
            cc_model::CcError::Other(format!("project initialization/reopen failed: {e}"))
        })?
    }

    /// Explicit bounded sweep used by the idle loop and deterministic lifecycle tests.
    pub async fn evict_idle_now(&self) -> usize {
        close_idle_instances(&self.active, &self.project_cache).await
    }

    pub fn touch_activity(&self) {
        if let Ok(mut ts) = self.last_activity.lock() {
            *ts = Instant::now();
        }
    }

    pub async fn reopen_active_index_if_closed(&self) -> CcResult<()> {
        let index = self.active_index().await;
        if index.try_read().is_ok_and(|rt| !rt.is_closed()) {
            return Ok(());
        }
        tokio::task::spawn_blocking(move || Self::reopen_index_if_closed(&index))
            .await
            .map_err(|e| cc_model::CcError::Other(format!("active project reopen failed: {e}")))?
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
                rt.reopen()?;
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
        let active = self.active.clone();
        let auto_indexing = self.auto_indexing.clone();
        // Initialization and polling share ONE tracked task; shutdown cannot
        // race a detached initializer that installs a fresh watcher afterward.
        self.tasks.replace_watcher(move |ready| async move {
            let index = active.read().await.index();
            let checked = index.clone();
            let expected = project_path.clone();
            let enabled = tokio::task::spawn_blocking(move || {
                let Ok(rt) = checked.read() else { return false };
                rt.project_path.as_ref() == Some(&expected)
                    && cc_model::config::load_project_config(&expected)
                        .auto_index
                        .enabled
            })
            .await
            .unwrap_or(false);
            if !enabled {
                return;
            }
            let native_started = Instant::now();
            let watcher = match tokio::task::spawn_blocking(move || {
                FileWatcher::start(&project_path)
            })
            .await
            {
                Ok(Ok(w)) => Arc::new(Mutex::new(Some(w))),
                Ok(Err(e)) => {
                    tracing::warn!("watcher initialization failed: {e}");
                    return;
                }
                Err(e) => {
                    tracing::warn!("watcher worker failed: {e}");
                    return;
                }
            };
            tracing::info!(
                native_start_ms = native_started.elapsed().as_millis(),
                "watcher native subscription ready"
            );
            ready.store(true, Ordering::Release);
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                // Always poll the captured project's runtime, never whichever
                // project happens to become active after an await.
                let pending = watcher
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                    .is_some_and(|w| w.has_pending());
                if !pending {
                    let checked = index.clone();
                    if !tokio::task::spawn_blocking(move || resolution_work_pending(&checked))
                        .await
                        .unwrap_or(false)
                    {
                        continue;
                    }
                }
                if auto_indexing
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    continue;
                }
                let permit = crate::session_tasks::AutoIndexPermit(auto_indexing.clone());
                let checked = index.clone();
                let pending = watcher.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let _permit = permit;
                    run_watcher_tick(&checked, &pending)
                })
                .await;
                match result {
                    Ok(WatcherTickOutcome::WatcherGone) => break,
                    Ok(_) => {}
                    Err(e) => tracing::warn!("watcher tick failed: {e}"),
                }
            }
        });
    }

    pub fn watcher_ready(&self) -> bool {
        self.tasks.watcher_ready()
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
        self.tasks.shutdown().await;
    }

    pub async fn start_idle_eviction(&self) {
        let last_activity = self.last_activity.clone();
        let active = self.active.clone();
        let project_cache = self.project_cache.clone();
        let idle_timeout_secs = active_idle_timeout_secs(&active).await;
        self.tasks.start_idle(async move {
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

/// Inspect durable work without consuming it. Disabled/zero-budget maintenance
/// waits for a user/config change rather than spinning useless builds.
fn resolution_work_pending(index: &SharedCodeIndex) -> bool {
    let Ok(rt) = index.read() else { return false };
    let Some(db) = rt.index_db() else {
        return false;
    };
    let Some(path) = rt.project_path.as_deref() else {
        return false;
    };
    let config = cc_model::config::load_project_config(path);
    if !config.indexing.dirty_propagation || config.indexing.dirty_propagation_max_files == 0 {
        return false;
    }
    match db.reads().resolution_freshness() {
        Ok(state) => !state.complete,
        Err(error) => {
            tracing::warn!(%error, "cannot inspect durable resolution work");
            false
        }
    }
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
    if drain.is_empty() && !resolution_work_pending(index) {
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
    let scope = drain.build_scope();
    if let Err(e) = handlers::core::run_split_build(index, false, false, scope.as_ref()) {
        // Draining must not acknowledge events when publication failed.
        let guard = watcher.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(w) = guard.as_ref() {
            w.request_rescan();
        }
        tracing::warn!(
            "watcher: incremental index failed; full reconciliation queued: {}",
            e
        );
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
    tokio::task::spawn_blocking(move || {
        let mut closed = 0usize;
        for index in indexes {
            // Idle maintenance never queues behind a query or build lock.
            let Ok(mut guard) = index.try_write() else {
                continue;
            };
            let gate = guard.build_gate();
            let Ok(_build) = gate.try_lock() else {
                continue;
            };
            if guard.query_pins() == 0 && guard.project_path.is_some() && !guard.is_closed() {
                guard.close();
                closed += 1;
            }
        }
        closed
    })
    .await
    .unwrap_or(0)
}

async fn active_idle_timeout_secs(active: &tokio::sync::RwLock<ProjectServices>) -> u64 {
    let index = active.read().await.index();
    index
        .try_read()
        .ok()
        .map(|rt| rt.idle_timeout_secs())
        .unwrap_or(DEFAULT_IDLE_TIMEOUT_SECS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    #[tokio::test]
    async fn p5d_hot_cache_does_not_wait_behind_cold_registry() {
        let dir = TempDir::new().unwrap();
        let session = ProjectSession::new(Some(dir.path()));
        let original = session.active_index().await;
        let _cold = session.live_projects.lock().await;
        let routed = tokio::time::timeout(
            Duration::from_millis(200),
            session.index_for_project_path(Some(dir.path().to_str().unwrap())),
        )
        .await
        .expect("open cache hit must bypass the cold registry")
        .unwrap();
        assert!(Arc::ptr_eq(&routed, &original));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn p5d_cancelled_cold_client_keeps_initializer_fenced_until_publication() {
        let dir = TempDir::new().unwrap();
        let path = normalize_path(dir.path());
        let session = ProjectSession::new(None);
        // Block the initializer's cache lookup without stalling the scheduler.
        let cache = session.project_cache.clone().lock_owned().await;
        let caller = session.clone();
        let requested = path.clone();
        let job = tokio::spawn(async move { caller.services_for_path(requested).await });
        tokio::time::timeout(Duration::from_secs(2), async {
            while session.live_projects.try_lock().is_ok() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        job.abort();
        assert!(matches!(job.await, Err(e) if e.is_cancelled()));
        assert!(
            session.live_projects.try_lock().is_err(),
            "blocking initializer, not its cancelled client, must own the cold guard"
        );
        drop(cache);
        let routed = tokio::time::timeout(
            Duration::from_secs(5),
            session.services_for_path(path.clone()),
        )
        .await
        .unwrap()
        .unwrap();
        let cached = session
            .project_cache
            .lock()
            .await
            .get(&path)
            .unwrap()
            .index();
        assert!(Arc::ptr_eq(&routed.index(), &cached));
        assert!(Arc::ptr_eq(
            &routed.index().read().unwrap().build_gate(),
            &cached.read().unwrap().build_gate()
        ));
        session.shutdown().await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn p5d_active_reopen_lock_wait_does_not_block_async_scheduler() {
        let dir = TempDir::new().unwrap();
        let session = ProjectSession::new(Some(dir.path()));
        let index = session.active_index().await;
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let _guard = index.write().unwrap();
            start_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(3));
        });
        start_rx.await.unwrap();
        let outcome = tokio::time::timeout(
            Duration::from_millis(100),
            session.reopen_active_index_if_closed(),
        )
        .await;
        let _ = release_tx.send(());
        worker.join().unwrap();
        assert!(
            outcome.is_err(),
            "async timeout must run while a lifecycle lock is held"
        );
        session.reopen_active_index_if_closed().await.unwrap();
        session.shutdown().await;
    }

    /// Poll until the auto-index build commits (clears `needs_initial_index`).
    async fn wait_for_auto_index(session: &ProjectSession, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let index = session.active_index().await;
            let built = index
                .read()
                .map(|rt| !rt.needs_initial_index())
                .unwrap_or(false);
            if built && !session.auto_indexing.load(Ordering::SeqCst) {
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

        // set_active_project starts a watcher whose configuration probe can
        // briefly hold B's read lock. Each idle sweep is deliberately
        // nonblocking, so require eventual full reclamation, not that the
        // first sweep wins that race. Sum every sweep (no best-of sample).
        let closed = tokio::time::timeout(Duration::from_secs(5), async {
            let mut closed = 0;
            loop {
                closed += close_idle_instances(&session.active, &session.project_cache).await;
                if closed == 2 {
                    break closed;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("both project instances must become reclaimable after transient contention");
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
        session.shutdown().await;
    }

    /// A watcher's short read probe is enough to make an idle sweep skip an
    /// instance. Reproduce that interleaving explicitly, then prove the next
    /// sweep reclaims the same instance when the probe releases its guard.
    #[tokio::test(flavor = "current_thread")]
    async fn idle_sweep_skips_reader_then_reclaims_every_cached_instance() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        let session = ProjectSession::new(Some(a.path()));
        let index_a = session.active_index().await;
        // Route B without starting an unrelated native watcher: the reader
        // below deterministically supplies the actual contention boundary.
        let services_b = session
            .services_for_path(normalize_path(b.path()))
            .await
            .unwrap();
        let index_b = services_b.index();
        *session.active.write().await = services_b;
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let reader = index_b.clone();
        let worker = std::thread::spawn(move || {
            let _probe = reader.read().unwrap();
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        ready_rx.await.unwrap();
        let first =
            tokio::time::timeout(Duration::from_millis(500), session.evict_idle_now()).await;
        release_tx.send(()).unwrap();
        worker.join().unwrap();
        assert_eq!(first.expect("idle sweep must not wait behind a reader"), 1);
        assert!(index_a.read().unwrap().is_closed());
        assert!(!index_b.read().unwrap().is_closed());
        assert_eq!(session.evict_idle_now().await, 1);
        assert!(index_b.read().unwrap().is_closed());
        assert_eq!(
            session.evict_idle_now().await,
            0,
            "closed aliases must not be double counted"
        );
        session.shutdown().await;
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

    #[test]
    fn p1d_watcher_tick_keeps_known_pending_batch_until_manual_gate_released() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "pub fn answer() -> i32 { 42 }\n").unwrap();
        let index = Arc::new(std::sync::RwLock::new(
            crate::engine::CodeIndex::new(Some(dir.path())).unwrap(),
        ));
        index.write().unwrap().build_index(true).unwrap();
        let before = active_generation(&index);
        let watcher = FileWatcher::start_with_config(
            &normalize_path(dir.path()),
            crate::watcher::WatcherConfig {
                git_sanity_poll: false,
                ..Default::default()
            },
        )
        .unwrap();
        std::fs::write(
            dir.path().join("extra.rs"),
            "pub fn extra_marker() -> i32 { 7 }\n",
        )
        .unwrap();
        // Controlled queue insertion verifies the gate/drain ordering, separately
        // from the native delivery integration test below, which remains enabled.
        watcher.inject_pending_for_test("extra.rs");
        let watcher = Arc::new(std::sync::Mutex::new(Some(watcher)));
        let gate = index.read().unwrap().build_gate();
        let permit = gate.lock().unwrap();
        for _ in 0..3 {
            assert!(matches!(
                run_watcher_tick(&index, &watcher),
                WatcherTickOutcome::Skipped
            ));
            assert!(watcher.lock().unwrap().as_ref().unwrap().has_pending());
            assert_eq!(active_generation(&index), before);
        }
        drop(permit);
        assert!(matches!(
            run_watcher_tick(&index, &watcher),
            WatcherTickOutcome::Completed
        ));
        assert!(index.read().unwrap().index_status().unwrap().indexed_files >= 2);
        watcher.lock().unwrap().take();
    }

    #[test]
    fn p3c_watcher_rescan_reconciles_unlisted_config_and_retries_failed_build() {
        let dir = TempDir::new().unwrap();
        std::fs::write(
            dir.path().join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false}}"#,
        )
        .unwrap();
        for name in ["a.ts", "b.ts"] {
            std::fs::write(
                dir.path().join(name),
                "export function ping(x:number){return x;}\n",
            )
            .unwrap();
        }
        std::fs::write(
            dir.path().join("use.ts"),
            "import {ping} from '@api'; export function run(){return ping(1);}\n",
        )
        .unwrap();
        let cfg =
            |name: &str| format!(r#"{{"compilerOptions":{{"paths":{{"@api":["./{name}"]}}}}}}"#);
        std::fs::write(dir.path().join("tsconfig.json"), cfg("a.ts")).unwrap();
        let index = Arc::new(std::sync::RwLock::new(
            crate::engine::CodeIndex::new(Some(dir.path())).unwrap(),
        ));
        index.write().unwrap().build_index(true).unwrap();
        // Change before starting the watcher: only the explicit rescan flag can discover it.
        std::fs::write(dir.path().join("tsconfig.json"), cfg("b.ts")).unwrap();
        let watcher = FileWatcher::start_with_config(
            &normalize_path(dir.path()),
            crate::watcher::WatcherConfig {
                git_sanity_poll: false,
                ..Default::default()
            },
        )
        .unwrap();
        watcher.request_rescan();
        let watcher = Arc::new(std::sync::Mutex::new(Some(watcher)));
        assert!(matches!(
            run_watcher_tick(&index, &watcher),
            WatcherTickOutcome::Completed
        ));
        let db = index.read().unwrap().index_db().unwrap().clone();
        let rows = db
            .reads()
            .query_json(
                "SELECT resolved_path FROM imports WHERE file_path='use.ts'",
                &[],
            )
            .unwrap();
        assert_eq!(rows[0]["resolved_path"], "b.ts");
        let key = cc_model::project_model::PROJECT_INPUT_KEY;
        let valid = db.reads().get_metadata(key).unwrap().unwrap();
        db.writes().set_metadata(key, "broken").unwrap();
        watcher.lock().unwrap().as_ref().unwrap().request_rescan();
        assert!(matches!(
            run_watcher_tick(&index, &watcher),
            WatcherTickOutcome::Completed
        ));
        assert!(
            watcher.lock().unwrap().as_ref().unwrap().has_pending(),
            "failed build lost the rescan request"
        );
        db.writes().set_metadata(key, &valid).unwrap();
        assert!(matches!(
            run_watcher_tick(&index, &watcher),
            WatcherTickOutcome::Completed
        ));
        assert!(
            !watcher
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .drain_pending()
                .rescan_needed
        );
        watcher.lock().unwrap().take();
    }

    #[test]
    fn p2c_watcher_drains_durable_debt_without_new_events() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join(".codecortex.json"), r#"{"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":1,"db_read_pool_size":1}}"#).unwrap();
        for name in ["a.py", "b.py", "c.py"] {
            std::fs::write(
                dir.path().join(name),
                "def entry(x):\n    return later(x)\n",
            )
            .unwrap();
        }
        let index = Arc::new(std::sync::RwLock::new(
            crate::engine::CodeIndex::new(Some(dir.path())).unwrap(),
        ));
        index.write().unwrap().build_index(true).unwrap();
        std::fs::write(
            dir.path().join("provider.py"),
            "def later(x):\n    return x\n",
        )
        .unwrap();
        index.write().unwrap().build_index(false).unwrap();
        let watcher = FileWatcher::start_with_config(
            &normalize_path(dir.path()),
            crate::watcher::WatcherConfig {
                git_sanity_poll: false,
                ..Default::default()
            },
        )
        .unwrap();
        let watcher = Arc::new(std::sync::Mutex::new(Some(watcher)));
        assert!(resolution_work_pending(&index));
        for _ in 0..2 {
            assert!(matches!(
                run_watcher_tick(&index, &watcher),
                WatcherTickOutcome::Completed
            ));
        }
        assert!(!resolution_work_pending(&index));
        assert!(matches!(
            run_watcher_tick(&index, &watcher),
            WatcherTickOutcome::Skipped
        ));
        watcher.lock().unwrap().take();
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
        // Observe successful subscription/poll-task installation rather than
        // assuming a fixed sleep is enough under parallel workspace load.
        let ready_deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if session.tasks.watcher_ready() {
                break;
            }
            assert!(
                Instant::now() < ready_deadline,
                "watcher did not become ready; event retention not tested"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }

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
