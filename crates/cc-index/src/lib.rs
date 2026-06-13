pub(crate) mod build_plan;
pub(crate) mod community;
pub(crate) mod config_linker;
pub(crate) mod dirty_closure;
pub(crate) mod dirty_reload_policy;
pub(crate) mod dispatch_synthesis;
pub mod framework_registry;
pub mod framework_resolvers;
pub(crate) mod git_cochange;
pub(crate) mod hierarchy;
pub mod indexer;
mod indexer_phases;
pub(crate) mod infra_docker;
pub(crate) mod infra_k8s;
pub(crate) mod infra_pass;
pub(crate) mod infra_terraform;
pub(crate) mod memory_budget;
pub(crate) mod pass_gate;
pub(crate) mod resolver;
pub mod scanner;
pub(crate) mod synthesis_pipeline;
pub(crate) mod synthesis_symbol_resolver;
pub(crate) mod type_catalog;

pub use build_plan::{PreparedBuild, StagedPostprocess, WrittenBuild};
pub use dirty_closure::DirtyPropagationStatus;
pub use framework_registry::FileFrameworkDetection;
pub use indexer::{IndexReport, Indexer};
pub use scanner::{ScannedFile, Scanner};

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
