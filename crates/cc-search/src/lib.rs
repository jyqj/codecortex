pub mod cypher;
pub mod dsl;
pub mod engine;
mod enrich;
mod lanes;
mod plan;
pub mod preselect;
pub mod rrf;
mod score_trace;

pub use engine::SearchEngine;
pub use enrich::GraphEnrichment;

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
