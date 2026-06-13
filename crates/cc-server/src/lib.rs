//! cc-server library re-exports for use by cc-eval and the binary crate.

pub mod engine;
pub mod handlers;
pub mod mcp;
pub mod project_session;
pub mod tools;

pub(crate) mod engine_query;
pub(crate) mod graph_cycles;
pub(crate) mod graph_flow;
pub(crate) mod graph_read_model;
pub(crate) mod graph_trace;
pub(crate) mod graph_type_hierarchy;
pub(crate) mod graph_types;
pub(crate) mod graph_walk;
pub(crate) mod impact;
pub(crate) mod path_guard;
pub(crate) mod symbol_extract;
pub(crate) mod symbol_resolution;
pub(crate) mod watcher;

/// Test-only seeding support shared by this crate's unit-test fixtures.
#[cfg(test)]
pub(crate) mod test_seed {
    /// Writable side connection for seeding test fixtures directly.
    ///
    /// The cc-db read pool is `query_only`, so fixtures can no longer write
    /// through `reads().read_conn()`. Seeding through a dedicated connection
    /// intentionally bypasses `WriteOps` and therefore does NOT bump the
    /// epoch vector (matching the previous fixture behavior); tests that
    /// assert epoch-keyed cache semantics must seed through `writes()`.
    pub(crate) fn seed_conn(db: &cc_db::index_db::IndexDb) -> rusqlite::Connection {
        rusqlite::Connection::open(db.admin().db_path()).expect("open test seed connection")
    }
}
