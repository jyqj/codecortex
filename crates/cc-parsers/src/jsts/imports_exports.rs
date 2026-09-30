//! Import/export extraction for JS/TS — ES `import` statements, `export`
//! clauses and re-exports (including two-step forwarding of imported
//! bindings), deferred export application, and local import bindings.

use super::{node_text, ExtractCtx, JsTsParser, PendingExport};
use cc_model::edge::ImportRecord;
use std::collections::HashMap;

impl JsTsParser {
    // -------------------------------------------------------------------
    // Export handling
    // -------------------------------------------------------------------

    /// Process export statement; returns (export_name, is_default).
    pub(super) fn visit_export(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        ctx: &mut ExtractCtx,
        file_path: &str,
    ) -> (Option<String>, bool) {
        let mut is_default = false;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "default" {
                is_default = true;
            }
        }

        // Check for re-export: export { ... } from "..."
        let source_node = {
            let mut cursor2 = node.walk();
            let mut found = None;
            for child in node.children(&mut cursor2) {
                if child.kind() == "string" {
                    found = Some(child);
                    break;
                }
            }
            found
        };

        if let Some(src_node) = source_node {
            let src = node_text(&src_node, source)
                .unwrap_or("")
                .trim_matches(|c| c == '\'' || c == '"')
                .to_string();

            // export * from "..."
            let mut cursor3 = node.walk();
            let has_star = node.children(&mut cursor3).any(|c| c.kind() == "*");
            if has_star {
                ctx.imports.push(ImportRecord {
                    context: cc_model::module_inputs::ImportContext {
                        syntax: cc_model::module_inputs::ImportSyntax::Static,
                        ..Default::default()
                    },
                    file_path: file_path.to_string(),
                    import_string: src,
                    resolved_path: None,
                    imported_name: None,
                    alias: None,
                    is_namespace: true,
                    is_default: false,
                    is_reexport: true,
                });
                return (None, false);
            }

            // export { a as b } from "..."
            let mut cursor4 = node.walk();
            for child in node.children(&mut cursor4) {
                if child.kind() == "export_clause" {
                    let mut spec_cursor = child.walk();
                    for spec in child.children(&mut spec_cursor) {
                        if spec.kind() == "export_specifier" {
                            let names: Vec<tree_sitter::Node> = {
                                let mut sc = spec.walk();
                                spec.children(&mut sc)
                                    .filter(|c| {
                                        matches!(c.kind(), "identifier" | "property_identifier")
                                    })
                                    .collect()
                            };
                            let (imported, exported) = if names.len() >= 2 {
                                (
                                    node_text(&names[0], source).unwrap_or("").to_string(),
                                    node_text(&names[1], source).unwrap_or("").to_string(),
                                )
                            } else if names.len() == 1 {
                                let n = node_text(&names[0], source).unwrap_or("").to_string();
                                (n.clone(), n)
                            } else {
                                continue;
                            };
                            ctx.imports.push(ImportRecord {
                                context: cc_model::module_inputs::ImportContext {
                                    syntax: cc_model::module_inputs::ImportSyntax::Static,
                                    ..Default::default()
                                },
                                file_path: file_path.to_string(),
                                import_string: src.clone(),
                                resolved_path: None,
                                imported_name: Some(imported),
                                alias: Some(exported),
                                is_namespace: false,
                                is_default: false,
                                is_reexport: true,
                            });
                        }
                    }
                }
            }
            return (None, false);
        }

        // Local export clause: export { foo, bar as baz }
        let mut cursor5 = node.walk();
        for child in node.children(&mut cursor5) {
            if child.kind() == "export_clause" {
                let mut spec_cursor = child.walk();
                for spec in child.children(&mut spec_cursor) {
                    if spec.kind() == "export_specifier" {
                        let names: Vec<tree_sitter::Node> = {
                            let mut sc = spec.walk();
                            spec.children(&mut sc)
                                .filter(|c| {
                                    matches!(c.kind(), "identifier" | "property_identifier")
                                })
                                .collect()
                        };
                        let (local_name, exported_name) = if names.len() >= 2 {
                            (
                                node_text(&names[0], source).unwrap_or("").to_string(),
                                Some(node_text(&names[1], source).unwrap_or("").to_string()),
                            )
                        } else if names.len() == 1 {
                            let n = node_text(&names[0], source).unwrap_or("").to_string();
                            (n.clone(), Some(n))
                        } else {
                            continue;
                        };
                        ctx.pending_exports.push(PendingExport {
                            local_name,
                            export_name: exported_name,
                            is_default: false,
                        });
                    }
                }
                return (None, false);
            }
        }

        // export default <identifier>
        if is_default {
            let mut cursor6 = node.walk();
            for child in node.children(&mut cursor6) {
                if child.kind() == "identifier" {
                    let local_name = node_text(&child, source).unwrap_or("").to_string();
                    ctx.pending_exports.push(PendingExport {
                        local_name,
                        export_name: None,
                        is_default: true,
                    });
                    break;
                }
            }
        }

        (None, is_default)
    }

    /// Apply deferred export bindings to symbols.
    pub(super) fn apply_pending_exports(&self, ctx: &mut ExtractCtx) {
        if ctx.pending_exports.is_empty() {
            return;
        }
        // Build name→index lookup (top-level only)
        let mut name_to_idx: HashMap<String, usize> = HashMap::new();
        for (idx, sym) in ctx.symbols.iter().enumerate() {
            if sym.container.is_none() {
                name_to_idx.entry(sym.name.clone()).or_insert(idx);
            }
        }

        for pe in &ctx.pending_exports {
            if let Some(&idx) = name_to_idx.get(&pe.local_name) {
                if pe.is_default {
                    ctx.symbols[idx].is_default_export = true;
                    if ctx.symbols[idx].export_name.is_none() {
                        ctx.symbols[idx].export_name = Some(ctx.symbols[idx].name.clone());
                    }
                } else if let Some(ref en) = pe.export_name {
                    ctx.symbols[idx].export_name = Some(en.clone());
                }
            } else if let Some(&imp_idx) = ctx.import_bindings.get(&pe.local_name) {
                // Two-step forwarding: the exported binding has no local
                // symbol and originates from an ES import
                // (`import { x } from './b'; export { x };` or
                // `export default x`). The file's effective export surface
                // therefore depends on the import target, so mark the
                // originating import as a re-export. Only the flag is
                // flipped — all other ImportRecord fields stay as extracted.
                ctx.imports[imp_idx].is_reexport = true;
            }
        }
    }

    pub(super) fn extract_import(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Vec<ImportRecord> {
        if let Some(clause) = super::child_by_kind(node, "import_require_clause") {
            if let Some(src) = clause
                .child_by_field_name("source")
                .and_then(|n| super::node_text(&n, source))
            {
                let src = src.trim_matches(|c| c == '\'' || c == '\"');
                if !src.contains('\\') {
                    let name = clause
                        .named_child(0)
                        .and_then(|n| super::node_text(&n, source))
                        .map(str::to_owned);
                    let mut cursor = node.walk();
                    let type_only = node.children(&mut cursor).any(|c| c.kind() == "type");
                    return vec![ImportRecord {
                        file_path: file_path.into(),
                        import_string: src.into(),
                        imported_name: Some("*".into()),
                        alias: name,
                        is_namespace: true,
                        context: cc_model::module_inputs::ImportContext {
                            syntax: if type_only {
                                cc_model::module_inputs::ImportSyntax::TypeOnlyRequire
                            } else {
                                cc_model::module_inputs::ImportSyntax::Require
                            },
                            ..Default::default()
                        },
                        ..Default::default()
                    }];
                }
            }
            return vec![];
        }
        crate::exports::jsts::ordinary_import_records(*node, source, file_path)
    }
}
