//! Isolated compile of exact status code and existing private observer.
//! Real cc-server worker/DB; no production edit or public/private API widening.
#![cfg(feature = "semantic")]
pub use cc_server::semantic_wiring;
mod service_factory {
    pub use cc_server::service_factory::SemanticWiredInfo;
    pub struct QueryServices(pub std::sync::Arc<cc_server::service_factory::QueryServices>);
    impl std::ops::Deref for QueryServices {
        type Target = cc_server::service_factory::QueryServices;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    impl QueryServices {
        pub fn query_encoding_active(&self) -> bool {
            panic!("unrelated private query-auth path must not execute in status snapshot fixtures")
        }
    }
}
#[path = "support/p7_status_review/legacy.rs"]
mod legacy_status;
#[path = "support/p7_status_review/candidate.rs"]
mod reviewed_status;
