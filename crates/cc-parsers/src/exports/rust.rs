//! Rust declared interfaces. Lexical visibility is recorded, not adjudicated
//! across a workspace. cfg text is retained; macros/proc attributes are unknown.
use super::{add_entry, field, is_comment, named, signature, text};
use cc_model::{
    public_surface::{PublicSurface, SurfaceForward, VisibilityDomain},
    ImportRecord,
};
use tree_sitter::{Node, Tree};

pub(crate) fn extract(
    tree: &Tree,
    source: &[u8],
    path: &str,
    imports: &mut [ImportRecord],
) -> PublicSurface {
    let mut out = PublicSurface::new("rust", path, "rust-declared-v3");
    if tree.root_node().has_error() {
        out.mark_unknown("syntax_error");
    }
    visit(tree.root_node(), source, "", &[], 0, &mut out);
    super::mark_forwards(&out, imports);
    out.normalize();
    out
}
fn visibility(node: Node<'_>, source: &[u8]) -> VisibilityDomain {
    let Some(vis) = named(node)
        .into_iter()
        .find(|n| n.kind() == "visibility_modifier")
    else {
        return VisibilityDomain::Module;
    };
    let raw: String = text(vis, source)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    match raw.as_str() {
        "pub" => VisibilityDomain::Exported,
        "pub(crate)" => VisibilityDomain::Crate,
        "pub(self)" => VisibilityDomain::Module,
        _ => VisibilityDomain::Restricted(raw),
    }
}
/// Shared by top-level visiting and nested signature walking. Attributes on
/// methods/associated items can also transform syntax, including omitted bodies.
pub(super) fn attribute_needs_expansion(node: Node<'_>, source: &[u8]) -> bool {
    let raw = text(node, source);
    let attr = raw
        .trim_start_matches('#')
        .trim_start_matches('!')
        .trim_start_matches('[')
        .trim_start()
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .next()
        .unwrap_or("");
    !matches!(
        attr,
        "cfg"
            | "allow"
            | "warn"
            | "deny"
            | "forbid"
            | "doc"
            | "inline"
            | "repr"
            | "must_use"
            | "test"
            | "cold"
    )
}

/// The source remains the legacy resolver's whole use-tree route. Individual
/// binding/alias evidence is split without claiming new project resolution.
fn forward(
    node: Node<'_>,
    source: &[u8],
    route: &str,
    domain: VisibilityDomain,
    depth: usize,
    out: &mut PublicSurface,
) {
    if depth > 128 {
        out.mark_unknown("surface_nesting_limit");
        return;
    }
    match node.kind() {
        "scoped_use_list" | "use_list" => {
            let list = node.child_by_field_name("list").unwrap_or(node);
            for child in named(list) {
                forward(child, source, route, domain.clone(), depth + 1, out);
            }
        }
        _ => {
            let (imported, exported) = if node.kind() == "use_as_clause" {
                (
                    field(node, "path", source).to_string(),
                    field(node, "alias", source).to_string(),
                )
            } else {
                let raw = text(node, source);
                (
                    raw.to_string(),
                    raw.rsplit("::").next().unwrap_or(raw).to_string(),
                )
            };
            out.forwards.push(SurfaceForward {
                source: route.into(),
                imported_name: imported,
                exported_name: exported,
                visibility: domain,
                type_only: false,
            });
        }
    }
}
fn visit(
    node: Node<'_>,
    source: &[u8],
    owner: &str,
    inherited: &[String],
    depth: usize,
    out: &mut PublicSurface,
) {
    if depth > 128 {
        out.mark_unknown("surface_nesting_limit");
        return;
    }
    let mut attributes = inherited.to_vec();
    for item in named(node) {
        if is_comment(item) {
            continue;
        }
        if matches!(item.kind(), "attribute_item" | "inner_attribute_item") {
            let tokens = signature(item, source, false, out);
            let value = serde_json::to_string(&tokens).expect("tokens serialize");
            if item.kind() == "inner_attribute_item" {
                out.conditions.push(value.clone());
            }
            if attribute_needs_expansion(item, source) {
                out.mark_unknown("attribute_expansion_not_evaluated");
            }
            attributes.push(value);
            continue;
        }
        let name = field(item, "name", source);
        let qualified = if owner.is_empty() {
            name.to_string()
        } else {
            format!("{owner}::{name}")
        };
        match item.kind() {
            "function_item"
            | "function_signature_item"
            | "struct_item"
            | "enum_item"
            | "trait_item"
            | "type_item"
            | "associated_type"
            | "const_item"
            | "static_item"
            | "extern_crate_declaration" => {
                add_entry(
                    out,
                    item,
                    source,
                    &qualified,
                    name,
                    visibility(item, source),
                    false,
                );
                out.entries.last_mut().unwrap().conditions = attributes.clone();
            }
            "mod_item" => {
                add_entry(
                    out,
                    item,
                    source,
                    &qualified,
                    name,
                    visibility(item, source),
                    true,
                );
                out.entries.last_mut().unwrap().conditions = attributes.clone();
                if let Some(body) = item.child_by_field_name("body") {
                    visit(body, source, &qualified, &attributes, depth + 1, out);
                }
            }
            "impl_item" => {
                let target = field(item, "type", source);
                let trait_name = field(item, "trait", source);
                let key = format!("{owner}::impl<{trait_name}>:{target}");
                add_entry(
                    out,
                    item,
                    source,
                    &key,
                    target,
                    VisibilityDomain::Module,
                    false,
                );
                out.entries.last_mut().unwrap().conditions = attributes.clone();
            }
            "use_declaration" => {
                let target = field(item, "argument", source);
                add_entry(
                    out,
                    item,
                    source,
                    &format!("{owner}::use:{target}"),
                    target,
                    visibility(item, source),
                    false,
                );
                out.entries.last_mut().unwrap().conditions = attributes.clone();
                if named(item)
                    .iter()
                    .any(|n| n.kind() == "visibility_modifier")
                {
                    if let Some(argument) = item.child_by_field_name("argument") {
                        forward(argument, source, target, visibility(item, source), 0, out);
                    }
                }
            }
            "empty_statement" => {}
            _ => {
                out.mark_unknown("rust_item_or_macro_not_modeled");
            }
        }
        attributes = inherited.to_vec();
    }
}
