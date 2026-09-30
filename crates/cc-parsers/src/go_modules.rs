//! Compact package/condition evidence, sharing the existing AST surface extractor.
use cc_model::{go_project::GoSourceFacts, CcError, CcResult};
pub fn extract(content: &str, file: &str) -> CcResult<GoSourceFacts> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_go::LANGUAGE.into())
        .map_err(|e| CcError::Other(e.to_string()))?;
    let tree = parser
        .parse(content, None)
        .ok_or_else(|| CcError::Other("Go syntax parse unavailable".into()))?;
    let s = crate::exports::go::extract(&tree, content.as_bytes(), file);
    let package = cc_model::package_surface::PackageKey::from_surface(&s).map(|k| k.name);
    let mut conditions = s.conditions;
    // cgo requires a selected target/toolchain and is never an unconditional pure-Go package.
    let mut cursor = tree.root_node().walk();
    for item in tree.root_node().named_children(&mut cursor) {
        if item.kind() == "import_declaration" {
            let mut pending = vec![item];
            while let Some(n) = pending.pop() {
                if n.kind() == "import_spec"
                    && n.child_by_field_name("path")
                        .and_then(|p| p.utf8_text(content.as_bytes()).ok())
                        .is_some_and(|p| p == "\"C\"" || p == "`C`")
                {
                    conditions.push("cgo".into());
                }
                let mut c = n.walk();
                pending.extend(n.named_children(&mut c));
            }
        }
    }
    conditions.sort();
    conditions.dedup();
    Ok(GoSourceFacts {
        unknown: tree.root_node().has_error() || package.is_none(),
        package,
        conditions,
    })
}
