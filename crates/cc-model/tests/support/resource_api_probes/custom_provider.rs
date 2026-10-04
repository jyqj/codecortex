use cc_model::declaration_identity::DeclarationInventory;
use std::collections::BTreeMap;
struct Changing(BTreeMap<String, Vec<u8>>);
impl DeclarationInventory for Changing {
    fn inventory(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.0
    }
}
fn main() {}
