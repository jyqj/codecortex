use cc_model::declaration_identity::{DeclarationLimits, DeclarationSnapshot};
use std::collections::BTreeMap;
fn main() {
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let _ = DeclarationSnapshot::with_limits(
        "owner".into(),
        &mut files,
        vec![],
        DeclarationLimits::PROTOTYPE,
    );
}
