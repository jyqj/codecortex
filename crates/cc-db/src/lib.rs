//! SQLite index storage: r2d2 read pool (`query_only=ON`) + single guarded
//! write connection, WAL, FTS5, the epoch twin-clock (`index_epoch` /
//! `evidence_epoch`), `UnitOfWork` as the only multi-statement write seam,
//! and the rebuild protocol. Capability facets (`reads()` / `writes()` /
//! `admin()` / `retrieval()` / `graph_reads()`) split the method surface.
//! Deep dive: `docs/internals/STORAGE.md`.

pub mod capability_read;
mod community_carry;
pub mod direct_writer;
pub mod document_store;
pub mod epoch_rules;
mod file_state_cache;
mod framework_scan;
mod freshness_store;
pub mod fts;
pub mod index_db;
mod index_db_arch;
mod index_db_edges;
mod index_db_frontier;
mod index_db_graph;
mod index_db_graph_read;
mod index_db_multi_insert;
mod index_db_query;
mod index_db_rebuild;
mod index_db_retrieval;
mod index_db_snapshot_insert;
mod index_db_types;
mod index_db_write_batch;
mod statement_work;
mod symbol_identity_store;

pub use framework_scan::{FileFrameworkAggregate, FrameworkScanSession};
pub use index_db::{MaintenanceOps, ReadOps, WriteOps};
pub use index_db_graph_read::GraphReads;
pub use index_db_retrieval::{
    ChunkScope, FtsCandidates, GrepChunkRow, GrepScanReport, HydratedChunks, RetrievalReadModel,
};
pub use snapshot_write_txn::SnapshotWriteTxn;
pub mod index_migrate;
mod public_surface_store;
mod resolution_dependency_store;
pub use public_surface_store::BoundSurfaceAddresses;
mod read_generation;
mod rows;
mod seed_symbol_cache;
pub use seed_symbol_cache::{seed_cache_max_symbols, SeedRows};
pub mod signature_agg;
pub use signature_agg::{GraphSignatureAggregates, RowAgg};
pub mod semantic_coverage;
pub mod semantic_gc_reads;
pub mod semantic_manifest_reads;
pub mod semantic_outbox;
pub mod semantic_publish;
pub mod semantic_queue;
pub mod semantic_rebuild;
pub mod semantic_recovery;
pub mod semantic_space_switch;
pub mod snapshot_write_txn;
pub mod sql_util;
pub mod unit_of_work;
