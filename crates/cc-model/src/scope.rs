use serde::{Deserialize, Serialize};

/// A name binding within a scope.
///
/// Experimental / inactive: no parser currently emits lexical scopes, so the
/// resolver's scope map is always empty and scope-aware resolution (closures,
/// block-level shadowing, import-alias + local shadowing) does not yet engage.
/// This type is retained as the contract for a future parser scope producer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeBinding {
    pub name: String,
    pub kind: String,
    pub symbol_uid: Option<String>,
}
