use cc_model::declaration_identity::{DeclarationLimits, DeclarationSnapshot};
use std::collections::BTreeMap;
fn main() {
    let files = BTreeMap::<String, Vec<u8>>::new();
    let shared = DeclarationSnapshot::with_limits(
        "owner".into(),
        &files,
        vec![],
        DeclarationLimits::PROTOTYPE,
    )
    .unwrap();
    assert_eq!(shared.limits(), DeclarationLimits::PROTOTYPE);
    drop(shared);
    let owned: DeclarationSnapshot =
        DeclarationSnapshot::new("owner".into(), files, vec![]).unwrap();
    assert_eq!(owned.limits(), DeclarationLimits::PROTOTYPE);
}
