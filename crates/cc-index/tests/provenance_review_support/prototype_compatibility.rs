use cc_model::{declaration_identity::*, module_inputs::*};
use std::collections::BTreeMap;
#[test]
fn production_empty_root_is_accepted() {
    let config = b"[tool.setuptools.package-dir]\n\"\"='.'\n";
    let files = BTreeMap::from([("pyproject.toml".into(), config.to_vec())]);
    let root = |directory: &str| ConfiguredRoot {
        directory: directory.into(),
        config_path: "pyproject.toml".into(),
        config_digest: content_digest(config),
        directive: "/tool/setuptools/package-dir/".into(),
    };
    assert!(DeclarationSnapshot::new("fixture".into(), files.clone(), vec![root("")]).is_ok());
    assert!(DeclarationSnapshot::new("fixture".into(), files, vec![root(".")]).is_ok());
    let old: PythonProject =
        serde_json::from_str(r#"{"roots":{"": ["src"]},"diagnostics":{}}"#).unwrap();
    assert!(old.provenance.is_empty());
}
