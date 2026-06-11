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
            return Ok(index);
        }

        let services = ProjectServices::new(Some(path.as_path()))?;
        let index = services.index();
        self.project_cache.lock().await.put(path, services);
        Ok(index)
    }

    pub async fn set_active_project(&self, project_path: PathBuf) -> CcResult<SharedCodeIndex> {
        let path = normalize_path(&project_path);
        let services = ProjectServices::new(Some(path.as_path()))?;
        let index = services.index();
        self.project_cache
            .lock()
            .await
            .put(path.clone(), services.clone());
        *self.active.write().await = services;
        self.start_watcher(path);
        Ok(index)
    }

    pub fn touch_activity(&self) {
        if let Ok(mut ts) = self.last_activity.lock() {
            *ts = Instant::now();
        }
    }

    pub async fn reopen_active_index_if_closed(&self) -> Result<(), String> {
        let index = self.active_index().await;
        let need_reopen = {
            let rt = handlers::lock_index(&index)?;
            rt.is_closed()
        };
        if need_reopen {
            let mut rt = handlers::lock_index_write(&index)?;
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
                // Brief read lock: clone the owned build inputs plus the
                // auto-index file-count gate.
                let (inputs, file_limit) = match index.read() {
                    Ok(rt) => {
                        let cloned = rt
                            .build_inputs()
                            .and_then(|inputs| Ok((inputs, rt.auto_index_file_limit()?)));
                        match cloned {
                            Ok(pair) => pair,
                            Err(e) => {
                                tracing::warn!("auto-index build_inputs failed: {}", e);
                                return;
                            }
                        }
                    }
                    Err(_) => return,
                };
                // Heavy prepare: no CodeIndex lock held, so read queries are
                // not blocked during scan/parse/resolve. The file-limit gate
                // (skip oversized repos) fires inside prepare.
                let prepared = match CodeIndex::prepare_build(&inputs, false, Some(file_limit)) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("auto-index failed: {}", e);
                        return;
                    }
                };
                // Brief write lock: commit (phase_write + postprocess +
                // bookkeeping) under the lock so readers never see the
                // non-transactional intermediate write state.
                if let Ok(mut rt) = index.write() {
                    if let Err(e) = rt.commit_build(&inputs, false, Some(file_limit), prepared) {
                        tracing::warn!("auto-index failed: {}", e);
                    }
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

                        // Drain pending events from the watcher.
                        let drain = {
                            let guard = match watcher_for_task.lock() {
                                Ok(g) => g,
                                Err(e) => e.into_inner(),
                            };
                            match guard.as_ref() {
                                Some(w) => w.drain_pending(),
                                None => break,
                            }
                        };

                        if drain.is_empty() {
                            continue;
                        }

                        tracing::info!(
                            changed = drain.changed.len(),
                            removed = drain.removed.len(),
                            "watcher: file changes detected, triggering incremental index"
                        );

                        // Skip if another auto-index is already running.
                        if auto_indexing
                            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                            .is_err()
                        {
                            tracing::debug!(
                                "watcher: skipping incremental index — already in progress"
                            );
                            continue;
                        }

                        let index = active.read().await.index();
                        let result = tokio::task::spawn_blocking(move || {
                            // Brief read lock: clone the owned build inputs.
                            let inputs = match index.read() {
                                Ok(rt) => match rt.build_inputs() {
                                    Ok(i) => i,
                                    Err(e) => {
                                        tracing::warn!(
                                            "watcher: incremental build_inputs failed: {}",
                                            e
                                        );
                                        return;
                                    }
                                },
                                Err(_) => return,
                            };
                            // Heavy prepare: no CodeIndex lock held, so read
                            // queries are not blocked during scan/parse/resolve.
                            let prepared = match CodeIndex::prepare_build(&inputs, false, None) {
                                Ok(p) => p,
                                Err(e) => {
                                    tracing::warn!("watcher: incremental prepare failed: {}", e);
                                    return;
                                }
                            };
                            // Brief write lock: commit (phase_write + postprocess
                            // + bookkeeping) under the lock so readers never see
                            // the non-transactional intermediate write state.
                            if let Ok(mut rt) = index.write() {
                                if let Err(e) = rt.commit_build(&inputs, false, None, prepared) {
                                    tracing::warn!("watcher: incremental commit failed: {}", e);
                                }
                            }
                        })
                        .await;

                        if let Err(e) = result {
                            tracing::warn!("watcher: incremental index task panicked: {}", e);
                        }
                        auto_indexing.store(false, Ordering::SeqCst);
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
                    let index = active.read().await.index();
                    let mut guard = match index.write() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };
                    if guard.project_path.is_some() && !guard.is_closed() {
                        tracing::info!("idle eviction: closing after {}s", elapsed.as_secs());
                        guard.close();
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
    // a write-lock `commit_build`, identical to the watcher poll path. This
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
}
