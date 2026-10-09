//! cc-server library re-exports for use by cc-eval and the binary crate.

pub mod capability_status;
pub mod engine;
pub mod handlers;
pub mod mcp;
pub mod project_session;
pub mod query_handle;
#[cfg(feature = "semantic-http")]
pub mod semantic_http_transport;
#[cfg(feature = "semantic")]
pub mod semantic_provider_factory;
#[cfg(feature = "semantic")]
pub mod semantic_query_encoding;
#[cfg(feature = "semantic")]
pub mod semantic_runtime;
/// Optional semantic-subsystem composition-root wiring (P7-010 minimal
/// assembly leg). Compiled only with the opt-in `semantic` feature; the
/// default build contains none of this code and no cc-semantic dependency.
#[cfg(feature = "semantic")]
pub mod semantic_scope_guard;
#[cfg(feature = "semantic")]
pub mod semantic_wiring;
pub mod service_factory;
pub(crate) mod session_tasks;
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
#[cfg(test)]
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

/// Explicit opt-in diagnostic logging; ordinary server behavior is unchanged.
pub mod runtime_diagnostics;
