//! File-local Go declarations, reused AST only. Package aggregation is separate.
//! This is syntax evidence, not go/types, constant evaluation or build selection.
use super::{add_entry, field, is_comment, named, signature, text};
use cc_model::public_surface::{PublicSurface, SurfaceEntry, SurfaceToken, VisibilityDomain};
use regex::Regex;
use std::sync::LazyLock;
use tree_sitter::{Node, Tree};
static EXPORTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\p{Lu}").unwrap());
fn domain(name: &str) -> VisibilityDomain {
    if EXPORTED.is_match(name) {
        VisibilityDomain::Exported
    } else {
        VisibilityDomain::Module
    }
}
pub(crate) fn extract(tree: &Tree, source: &[u8], path: &str) -> PublicSurface {
    let mut out = PublicSurface::new("go", path, "go-declared-v1");
    if tree.root_node().has_error() {
        out.mark_unknown("syntax_error");
    }
    let filename = path.rsplit('/').next().unwrap_or(path);
    let platform_parts = filename.trim_end_matches(".go").trim_end_matches("_test");
    if platform_parts.split('_').skip(1).any(|s| {
        matches!(
            s,
            "aix"
                | "android"
                | "darwin"
                | "dragonfly"
                | "freebsd"
                | "illumos"
                | "ios"
                | "js"
                | "linux"
                | "netbsd"
                | "openbsd"
                | "plan9"
                | "solaris"
                | "wasip1"
                | "windows"
                | "386"
                | "amd64"
                | "arm"
                | "arm64"
                | "loong64"
                | "mips"
                | "mipsle"
                | "mips64"
                | "mips64le"
                | "ppc64"
                | "ppc64le"
                | "riscv64"
                | "s390x"
                | "wasm"
        )
    }) {
        out.conditions.push(format!("go_filename:{filename}"));
        out.mark_unknown("go_build_selection_not_evaluated");
    }
    for item in named(tree.root_node()) {
        if is_comment(item) {
            let raw = text(item, source);
            if raw.starts_with("//go:") || raw.starts_with("// +build") {
                out.conditions.push(raw.into());
                out.mark_unknown("go_directive_not_evaluated");
            }
            continue;
        }
        match item.kind() {
            "package_clause" => {
                let name = named(item)
                    .into_iter()
                    .find(|n| n.kind() == "package_identifier")
                    .map(|n| text(n, source))
                    .unwrap_or("");
                if name.is_empty() || name == "_" {
                    out.mark_unknown("go_package_not_identified");
                }
                out.entries.push(SurfaceEntry {
                    qualified_name: name.into(),
                    exported_name: String::new(),
                    kind: "go_package".into(),
                    visibility: VisibilityDomain::Module,
                    signature: vec![SurfaceToken {
                        kind: "package_identifier".into(),
                        text: name.into(),
                    }],
                    conditions: vec![],
                });
            }
            "function_declaration" | "method_declaration" => {
                let name = field(item, "name", source);
                if name == "_" {
                    continue;
                }
                let receiver = item.child_by_field_name("receiver").map(|n| {
                    serde_json::to_string(&signature(n, source, false, &mut out)).unwrap()
                });
                let qname = receiver
                    .map(|r| format!("{r}.{name}"))
                    .unwrap_or_else(|| name.into());
                add_entry(&mut out, item, source, &qname, name, domain(name), false);
            }
            "type_declaration" => {
                for spec in named(item) {
                    if is_comment(spec) {
                        continue;
                    }
                    if !matches!(spec.kind(), "type_spec" | "type_alias") {
                        out.mark_unknown("go_type_form_not_modeled");
                        continue;
                    }
                    let name = field(spec, "name", source);
                    if name == "_" {
                        continue;
                    }
                    add_entry(&mut out, spec, source, name, name, domain(name), false);
                }
            }
            "var_declaration" | "const_declaration" => {
                // Keep the entire group so iota/implicit repeated expressions and
                // declaration order cannot be erased by sorting individual names.
                let tokens = signature(item, source, false, &mut out);
                let mut pending = vec![item];
                while let Some(n) = pending.pop() {
                    if matches!(n.kind(), "var_spec" | "const_spec") {
                        let mut cursor = n.walk();
                        for name in n.children_by_field_name("name", &mut cursor) {
                            if name.kind() != "identifier" || text(name, source) == "_" {
                                continue;
                            }
                            let name = text(name, source);
                            out.entries.push(SurfaceEntry {
                                qualified_name: name.into(),
                                exported_name: name.into(),
                                kind: item.kind().into(),
                                visibility: domain(name),
                                signature: tokens.clone(),
                                conditions: vec![],
                            });
                        }
                        if n.child_by_field_name("type").is_none()
                            && n.child_by_field_name("value")
                                .is_some_and(has_dynamic_expression)
                        {
                            out.mark_unknown("go_inferred_binding_type_not_evaluated");
                        }
                    } else {
                        pending.extend(named(n));
                    }
                }
            }
            "import_declaration" => {
                let mut pending = vec![item];
                while let Some(n) = pending.pop() {
                    if n.kind() == "import_spec" {
                        let raw = field(n, "path", source);
                        let alias = field(n, "name", source);
                        if raw.trim_matches(['\"', '`']) == "C" || alias == "." {
                            out.mark_unknown("go_cgo_or_dot_import_not_modeled");
                        }
                        add_entry(
                            &mut out,
                            n,
                            source,
                            &format!("import:{raw}:{alias}"),
                            "",
                            VisibilityDomain::Module,
                            false,
                        );
                    } else {
                        pending.extend(named(n));
                    }
                }
            }
            "empty_statement" => {}
            _ => out.mark_unknown("go_declaration_not_modeled"),
        }
    }
    if out
        .entries
        .iter()
        .filter(|e| e.kind == "go_package")
        .count()
        != 1
    {
        out.mark_unknown("go_package_not_identified");
    }
    out.normalize();
    out
}
fn has_dynamic_expression(root: Node<'_>) -> bool {
    let mut stack = vec![root];
    let mut count = 0;
    while let Some(n) = stack.pop() {
        count += 1;
        if count > 100_000 {
            return true;
        }
        if matches!(n.kind(), "call_expression" | "func_literal") {
            return true;
        }
        stack.extend(named(n));
    }
    false
}
