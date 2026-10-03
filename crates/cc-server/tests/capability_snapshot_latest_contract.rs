//! Apply unchanged independent oracles to the actual current production source.
//! The original frozen review source and all oracle files remain byte-identical.
#![cfg(feature = "semantic")]
pub use cc_server::{engine, semantic_runtime, semantic_wiring};
mod service_factory {
    pub use cc_server::service_factory::{SemanticDegradation, SemanticWiredInfo};
    pub struct QueryServices(pub std::sync::Arc<cc_server::service_factory::QueryServices>);
    impl std::ops::Deref for QueryServices {
        type Target = cc_server::service_factory::QueryServices;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl Default for QueryServices {
        fn default() -> Self {
            Self(std::sync::Arc::new(Default::default()))
        }
    }
    impl QueryServices {
        pub fn query_encoding_active(&self) -> bool {
            panic!("query-auth private path is excluded by these fixture configs")
        }
    }
}
#[path = "../../../artifacts/checkpoints/capability-snapshot-optimization-20261003/current-status-under-test.rs"]
mod actual_status;
#[path = "support/p7_status_review/legacy.rs"]
mod legacy_status;
use actual_status::{snapshot, snapshot_observing};
use cc_db::index_db::IndexDb;
use cc_model::config::ProjectConfig;
use serde_json::{json, Value};
use service_factory::QueryServices;
#[path = "support/p7_status_review/checks.rs"]
mod independent;

#[test]
fn candidate_source_is_exact_current_production_prefix() {
    let production = include_str!("../src/capability_status.rs");
    let candidate = include_str!("../../../artifacts/checkpoints/capability-snapshot-optimization-20261003/current-status-under-test.rs");
    let marker = "#[cfg(test)]\nmod tests {";
    assert_eq!(candidate, production.split(marker).next().unwrap(),
        "the isolated current-source oracle must never use a stale frozen copy");
}
