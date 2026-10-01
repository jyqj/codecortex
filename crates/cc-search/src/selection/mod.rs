//! Evidence-set selection is separate from rank scoring and source authority.
pub mod budget;
pub mod coverage;
pub mod overlap;

pub const SELECTION_SPEC: &str = "anchored-facets-distinctive-source-union-v2";
pub const BUDGET_SPEC: &str = cc_model::context::CONTEXT_PACKING_SPEC;
