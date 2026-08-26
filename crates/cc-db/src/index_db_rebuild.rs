//! Full-rebuild protocol of [`IndexDb`]: bulk pragmas, temp-db build,
//! staging swap, and the shared swap/reopen/verify sequence used by the
//! temp-db, staging, and direct-writer strategies. Split from `index_db.rs`;
//! every method is an `impl IndexDb` continuation with unchanged bodies.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use cc_model::{CcError, CcResult};

use crate::index_db::{IndexDb, IndexGeneration};
use crate::index_migrate::{CURRENT_SCHEMA_VERSION, FULL_SCHEMA_SQL};
use crate::sql_util::db_err;

impl IndexDb {
    // ── Bulk rebuild pragmas ────────────────────────────────────

    /// Apply aggressive pragmas for full rebuild (not safe for incremental).
    /// Only call on the write connection, never on read pool connections.
    fn set_bulk_rebuild_pragmas(conn: &Connection) -> CcResult<()> {
        conn.execute_batch(
            "PRAGMA synchronous = OFF;
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -64000;
             PRAGMA mmap_size = 268435456;",
        )
        .map_err(|e| CcError::Database(format!("set_bulk_rebuild_pragmas: {}", e)))?;
        Ok(())
    }

    /// Restore normal pragmas after bulk operation.
    fn restore_normal_pragmas(conn: &Connection) -> CcResult<()> {
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA temp_store = DEFAULT;
             PRAGMA cache_size = -2000;",
        )
        .map_err(|e| CcError::Database(format!("restore_normal_pragmas: {}", e)))?;
        Ok(())
    }

    pub(crate) fn execute_cached<P: rusqlite::Params>(
        conn: &Connection,
        sql: &str,
        params: P,
    ) -> CcResult<usize> {
        let mut stmt = conn.prepare_cached(sql).map_err(db_err)?;
        stmt.execute(params).map_err(db_err)
    }

    // ── Full rebuild protocol (shared by both rebuild paths) ────

    /// Derive a SQLite sidecar path (`-wal` / `-shm`) by appending to the
    /// full file name. SQLite appends to the complete name, so this must not
    /// go through `Path::with_extension` (which replaces the last extension).
    fn sidecar_path(db_path: &Path, suffix: &str) -> PathBuf {
        let mut os = db_path.as_os_str().to_owned();
        os.push(suffix);
        PathBuf::from(os)
    }

    /// Path used by the standard temp-db full rebuild staging file.
    pub(crate) fn rebuild_staging_path(&self) -> PathBuf {
        self.db_path.with_extension("sqlite3.tmp")
    }

    /// Remove stale staging sidecars before writing a fresh temp database.
    fn cleanup_rebuild_staging_artifacts(tmp_path: &Path) {
        let _ = std::fs::remove_file(tmp_path);
        let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-wal"));
        let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-shm"));
    }

    /// Produce a fully written replacement database at `tmp_path` without
    /// swapping it into place. Returns the epoch floor snapshot taken before
    /// the build so a later [`Self::swap_rebuild_staging`] can finalize under
    /// the write lock.
    pub(crate) fn build_rebuild_staging(
        &self,
        tmp_path: &Path,
        build_temp: impl FnOnce(&Path) -> CcResult<()>,
    ) -> CcResult<IndexGeneration> {
        let generation_floor = self.generation().unwrap_or_default();
        Self::cleanup_rebuild_staging_artifacts(tmp_path);
        build_temp(tmp_path)?;
        Ok(generation_floor)
    }

    /// Finalize and atomically swap a staged rebuild database into place.
    pub(crate) fn swap_rebuild_staging(
        &self,
        tmp_path: &Path,
        generation_floor: IndexGeneration,
        label: &str,
    ) -> CcResult<()> {
        if !tmp_path.exists() {
            return Err(CcError::Database(format!(
                "rebuild staging file missing: {}",
                tmp_path.display()
            )));
        }

        // Acquire write lock, do atomic swap while lock is held
        {
            // Lock the write connection to prevent concurrent writes.
            // The rename MUST happen inside this scope so no writer can
            // slip in between lock-release and file replacement.
            let _write_guard = self.write_conn.lock().map_err(db_err)?;

            // Write the final epoch vector now that no further writes can
            // land: max(floor, live) + 1 covers writes committed while the
            // rebuild ran (including from other processes).
            self.finalize_rebuild_generation(tmp_path, generation_floor)?;

            // Remove the old WAL/SHM files — the new file will create its own
            let wal = self.db_path.with_extension("sqlite3-wal");
            let shm = self.db_path.with_extension("sqlite3-shm");
            let _ = std::fs::remove_file(&wal);
            let _ = std::fs::remove_file(&shm);

            // Atomic rename: temp → main (inside write lock)
            std::fs::rename(tmp_path, &self.db_path).map_err(|e| {
                CcError::Database(format!(
                    "atomic rename {} → {}: {}",
                    tmp_path.display(),
                    self.db_path.display(),
                    e
                ))
            })?;

            // Clean up temp WAL/SHM if any
            let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-wal"));
            let _ = std::fs::remove_file(Self::sidecar_path(tmp_path, "-shm"));
        }

        // Reopen write connection
        let (new_write_conn, _status) = Self::open_and_ensure_schema(&self.db_path)?;
        {
            let mut guard = self.write_conn.lock().map_err(db_err)?;
            *guard = new_write_conn;
        }

        // Rebuild the read pool using the configured/adaptive pool size.
        let new_pool = Self::build_read_pool(&self.db_path, self.read_pool_size)?;
        {
            let mut pool_guard = self
                .pool
                .write()
                .map_err(|e| CcError::Database(format!("write pool lock: {}", e)))?;
            *pool_guard = new_pool;
        }

        // Checkpoint WAL to reclaim space after full rebuild
        if let Err(e) = self.checkpoint_wal() {
            tracing::warn!(err = %e, "{}: WAL checkpoint failed (non-fatal)", label);
        }

        Ok(())
    }

    /// Shared temp-db build strategy: open a fresh schema, bulk-insert via
    /// `write_fn`, recreate indexes, restore pragmas. Used by both the bundled
    /// rebuild path and the prepare-time staging path.
    pub(crate) fn execute_temp_db_staging_build<F>(tmp_path: &Path, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        tracing::info!(
            tmp = %tmp_path.display(),
            "full rebuild: creating temp database"
        );

        let tmp_conn = Connection::open(tmp_path)
            .map_err(|e| CcError::Database(format!("open temp db: {}", e)))?;
        tmp_conn
            .execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| CcError::Database(format!("temp db pragmas: {}", e)))?;
        tmp_conn.set_prepared_statement_cache_capacity(64);

        tmp_conn
            .execute_batch(FULL_SCHEMA_SQL)
            .map_err(|e| CcError::Database(format!("temp db schema init failed: {}", e)))?;
        tmp_conn
            .pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION)
            .map_err(|e| CcError::Database(format!("temp db set version: {}", e)))?;

        Self::set_bulk_rebuild_pragmas(&tmp_conn)?;

        tmp_conn
            .execute_batch(&crate::direct_writer::drop_index_statements(
                FULL_SCHEMA_SQL,
            ))
            .map_err(|e| CcError::Database(format!("temp db drop indexes: {}", e)))?;

        tracing::info!("full rebuild: writing data to temp database");

        {
            let tx_result = (|| -> CcResult<()> {
                let tx = tmp_conn
                    .unchecked_transaction()
                    .map_err(|e| CcError::Database(format!("temp db transaction: {}", e)))?;
                write_fn(&tx)?;
                tx.commit()
                    .map_err(|e| CcError::Database(format!("temp db commit: {}", e)))?;
                Ok(())
            })();

            if let Err(e) = tx_result {
                tracing::warn!(err = %e, "full rebuild failed, cleaning up temp db");
                drop(tmp_conn);
                let _ = std::fs::remove_file(tmp_path);
                return Err(e);
            }
        }

        tracing::info!("full rebuild: recreating indexes");
        tmp_conn
            .execute_batch(&crate::direct_writer::extract_index_statements(
                FULL_SCHEMA_SQL,
            ))
            .map_err(|e| CcError::Database(format!("temp db recreate indexes: {}", e)))?;

        Self::restore_normal_pragmas(&tmp_conn)?;
        drop(tmp_conn);

        tracing::info!("full rebuild: temp database ready for swap");
        Ok(())
    }

    /// Shared full-rebuild protocol behind [`Self::rebuild_with_temp_db`] and
    /// [`Self::rebuild_with_direct_writer`], composed from
    /// [`Self::build_rebuild_staging`] + [`Self::swap_rebuild_staging`] so the
    /// prepare-time staging path (staging during build prepare, swap at
    /// commit) shares exactly this implementation.
    ///
    /// The strategy-specific part is `build_temp`: it must leave a fully
    /// written, closed SQLite database file at `tmp_path`, or return an error
    /// (cleaning up after itself if it wants to — the protocol only sweeps
    /// stale temp artifacts at the *start* of the next rebuild). The protocol
    /// runs the invariant-bearing steps in order:
    ///
    /// 1. Snapshot the epoch floor.
    /// 2. Remove stale temp artifacts left by a previous crashed run.
    /// 3. `build_temp(tmp_path)` — produce the replacement database.
    /// 4. Under the write lock: finalize the generation into the temp file,
    ///    remove the live WAL/SHM sidecars, atomically rename temp → main,
    ///    remove the temp sidecars.
    /// 5. Reopen the write connection, rebuild the read pool, checkpoint the
    ///    fresh WAL (non-fatal on failure).
    ///
    /// # Invariants
    ///
    /// - **Epoch floor.** The generation snapshotted in step 1 is only a
    ///   *floor*: writers (including other processes) may advance the live
    ///   epochs while `build_temp` runs. The final epoch vector is therefore
    ///   written at swap time, under the write lock, as `max(floor, live) + 1`
    ///   per epoch (see [`Self::finalize_rebuild_generation`]), so a rebuild
    ///   can never produce a "same generation, different content" collision
    ///   with values observed before or during the rebuild.
    /// - **Lock scope / no reentrancy.** The `write_conn` mutex is held for
    ///   the entire finalize + rename window so no writer can commit between
    ///   epoch finalization and the file swap. The mutex is not reentrant:
    ///   `build_temp` (and any `write_fn` it wraps) writes only to the temp
    ///   file and must never take the write lock or call back into either
    ///   rebuild method on the same thread. Concurrent writes from *other*
    ///   threads during `build_temp` are allowed — the epoch floor exists
    ///   precisely to absorb them.
    /// - **FTS dual maintenance.** `symbols_fts` and `file_paths_fts` are
    ///   maintained by schema triggers on `symbols`/`files`, while
    ///   `chunks_fts`, `files_fts` and `literal_fts` have no triggers and are
    ///   maintained at the application layer ([`Self::delete_file_data`] and
    ///   the insert helpers). Rebuild strategies write into a fresh schema, so
    ///   both models hold automatically — but `build_temp` must populate data
    ///   through the shared insert helpers so the application-maintained FTS
    ///   tables stay in sync with their base tables, including the
    ///   FTS-rowid-equals-base-rowid alignment the indexed deletes rely on.
    fn run_rebuild_protocol(
        &self,
        tmp_path: &Path,
        label: &str,
        build_temp: impl FnOnce(&Path) -> CcResult<()>,
    ) -> CcResult<()> {
        let generation_floor = self.build_rebuild_staging(tmp_path, build_temp)?;
        self.swap_rebuild_staging(tmp_path, generation_floor, label)
    }

    /// Perform a full rebuild using a temporary database file, then atomically
    /// swap it into place. This avoids WAL contention and allows aggressive
    /// pragmas without risk to the live database.
    ///
    /// The `write_fn` closure receives a mutable reference to the temp
    /// `Connection` with bulk pragmas already applied and indexes dropped.
    /// It should insert all data. Indexes are recreated automatically after
    /// the closure returns.
    ///
    /// After successful write, the temp file is renamed over the main db file
    /// and all connections (pool + write) are re-opened.
    ///
    /// Thin adapter over [`Self::run_rebuild_protocol`]; only the temp-file
    /// build strategy lives here.
    pub(crate) fn rebuild_with_temp_db<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("sqlite3.tmp");
        self.run_rebuild_protocol(&tmp_path, "full rebuild", |tmp_path| {
            Self::execute_temp_db_staging_build(tmp_path, write_fn)
        })?;

        tracing::info!("full rebuild: temp-db swap complete");
        Ok(())
    }

    /// High-speed full rebuild using DirectWriter.
    ///
    /// Creates a fresh database with aggressive PRAGMAs (journal OFF, synchronous OFF,
    /// 64KB pages, exclusive locking), writes all data via the caller-supplied closure,
    /// creates indexes after data, validates with integrity_check, then atomically
    /// swaps the new file into place.
    ///
    /// The `write_fn` signature matches `rebuild_with_temp_db` so callers can switch
    /// between them trivially.
    ///
    /// Thin adapter over [`Self::run_rebuild_protocol`]; only the
    /// DirectWriter build strategy lives here.
    ///
    /// Enable via `IndexingConfig::use_direct_writer == true`.
    pub(crate) fn rebuild_with_direct_writer<F>(&self, write_fn: F) -> CcResult<()>
    where
        F: FnOnce(&Connection) -> CcResult<()>,
    {
        let tmp_path = self.db_path.with_extension("direct-tmp.sqlite3");
        self.run_rebuild_protocol(&tmp_path, "direct writer", |tmp_path| {
            tracing::info!(
                tmp = %tmp_path.display(),
                "direct writer: creating high-speed temp database"
            );

            crate::direct_writer::DirectWriter::write_db(tmp_path, FULL_SCHEMA_SQL, |tx| {
                // Set schema version inside the transaction
                tx.pragma_update(
                    None,
                    "user_version",
                    crate::index_migrate::CURRENT_SCHEMA_VERSION,
                )
                .map_err(|e| CcError::Database(format!("set user_version: {}", e)))?;

                // Delegate to caller's write function.
                // Transaction derefs to Connection, so write_fn(&tx) works.
                write_fn(tx)
            })?;

            tracing::info!("direct writer: temp database written, swapping into place");
            Ok(())
        })?;

        tracing::info!("direct writer: swap complete");
        Ok(())
    }
}
