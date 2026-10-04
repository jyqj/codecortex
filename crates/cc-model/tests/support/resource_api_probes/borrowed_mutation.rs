use cc_model::declaration_identity::{DeclarationLimits, DeclarationSnapshot};
use std::collections::BTreeMap;
fn main() {
    let mut files = BTreeMap::<String, Vec<u8>>::new();
    let snapshot = DeclarationSnapshot::with_limits(
        "owner".into(),
        &files,
        vec![],
        DeclarationLimits::PROTOTYPE,
    )
    .unwrap();
    files.insert("changed".into(), vec![1]);
    assert_eq!(snapshot.limits(), DeclarationLimits::PROTOTYPE);
}
