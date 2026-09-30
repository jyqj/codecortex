//! Compact module-declaration syntax, not macro expansion or Rust execution.
use cc_model::{module_inputs::*, CcError, CcResult};
use tree_sitter::{Node, Tree};
fn text<'a>(n: Node<'_>, s: &'a [u8]) -> &'a str {
    n.utf8_text(s).unwrap_or("")
}
fn attribute(
    raw: &str,
    conditions: &mut Vec<String>,
    path: &mut Option<String>,
    unsupported: &mut Option<String>,
) {
    let raw = raw
        .trim()
        .trim_start_matches('#')
        .trim_start_matches('!')
        .trim();
    let a = raw
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .unwrap_or(raw)
        .trim();
    if let Some(p) = a.strip_prefix("cfg(").and_then(|s| s.strip_suffix(')')) {
        conditions.push(p.trim().into());
    } else if let Some(v) = a
        .strip_prefix("path")
        .and_then(|s| s.trim_start().strip_prefix('='))
    {
        match serde_json::from_str::<String>(v.trim()) {
            Ok(p) if path.is_none() => *path = Some(p),
            _ => *unsupported = Some("rust_path_attribute_unsupported".into()),
        }
    } else if a.starts_with("cfg_attr") {
        *unsupported = Some("rust_cfg_attr_not_expanded".into());
    } else if ![
        "doc",
        "allow",
        "warn",
        "deny",
        "forbid",
        "deprecated",
        "rustfmt",
        "expect",
    ]
    .iter()
    .any(|p| a == *p || a.starts_with(&format!("{p}(")) || a.starts_with(&format!("{p} ")))
    {
        *unsupported = Some("rust_module_attribute_not_expanded".into());
    }
}
fn attrs(node: Node<'_>, s: &[u8]) -> (Vec<String>, Option<String>, Option<String>) {
    let mut siblings = Vec::new();
    let mut n = node.prev_named_sibling();
    while let Some(x) = n {
        if matches!(x.kind(), "line_comment" | "block_comment") {
            n = x.prev_named_sibling();
            continue;
        }
        if x.kind() != "attribute_item" {
            break;
        }
        siblings.push(x);
        n = x.prev_named_sibling();
    }
    siblings.reverse();
    let (mut c, mut p, mut u) = (vec![], None, None);
    for a in siblings {
        attribute(text(a, s), &mut c, &mut p, &mut u);
    }
    (c, p, u)
}
pub(crate) fn import_context(node: Node<'_>, s: &[u8]) -> ImportContext {
    let mut context = ImportContext::default();
    let mut n = Some(node);
    while let Some(x) = n {
        let (c, _, u) = attrs(x, s);
        context.conditions.extend(c);
        if let Some(u) = u {
            context.conditions.push(format!("unsupported:{u}"));
        }
        if x.kind() == "mod_item" {
            if let Some(name) = x.child_by_field_name("name") {
                context.lexical_module.push(text(name, s).into());
            }
        }
        n = x.parent();
    }
    context.lexical_module.reverse();
    context.conditions.sort();
    context.conditions.dedup();
    context
}
fn walk(
    node: Node<'_>,
    s: &[u8],
    parent: &[String],
    inherited: &[String],
    facts: &mut RustSourceFacts,
    depth: usize,
) {
    if depth > 64 || facts.modules.len() >= 4096 {
        facts.syntax_error = true;
        return;
    }
    let mut cursor = node.walk();
    let mut inherited = inherited.to_vec();
    for n in node.named_children(&mut cursor) {
        if n.kind() == "inner_attribute_item" {
            let (mut c, mut p, mut u) = (vec![], None, None);
            attribute(text(n, s), &mut c, &mut p, &mut u);
            inherited.extend(c);
            if let Some(u) = u {
                inherited.push(format!("unsupported:{u}"));
            }
            if parent.is_empty() {
                facts.inner_conditions = inherited.clone();
            }
        }
        if n.kind() != "mod_item" {
            continue;
        }
        let Some(name) = n.child_by_field_name("name") else {
            continue;
        };
        let name = text(name, s).to_owned();
        let (mut conditions, path, unsupported) = attrs(n, s);
        conditions.extend(inherited.iter().cloned());
        let body = n.child_by_field_name("body");
        facts.modules.push(RustModuleDecl {
            name: name.clone(),
            parent: parent.into(),
            inline: body.is_some(),
            path,
            conditions: conditions.clone(),
            unsupported,
        });
        if let Some(body) = body {
            let mut p = parent.to_vec();
            p.push(name);
            walk(body, s, &p, &conditions, facts, depth + 1);
        }
    }
}
pub fn extract_from_tree(tree: &Tree, source: &[u8]) -> RustSourceFacts {
    let mut out = RustSourceFacts {
        syntax_error: tree.root_node().has_error(),
        ..Default::default()
    };
    walk(tree.root_node(), source, &[], &[], &mut out, 0);
    out
}
pub fn extract(source: &str) -> CcResult<RustSourceFacts> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|e| CcError::Other(e.to_string()))?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| CcError::Other("Rust module parse failed".into()))?;
    Ok(extract_from_tree(&tree, source.as_bytes()))
}
