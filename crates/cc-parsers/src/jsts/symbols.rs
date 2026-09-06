//! Symbol extraction for JS/TS — functions, classes, methods, variable
//! declarations (incl. CommonJS `require()` imports and `useState` setter
//! bindings), TS param/return type info, and `SymbolRecord` construction.

use super::{
    classify_framework_role, node_text, ExtractCtx, JsTsParser, STATE_SETTER_BINDING_CONFIDENCE,
};
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::ImportRecord;
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{ElementKind, ParserTier};

impl JsTsParser {
    // -------------------------------------------------------------------
    // Symbol extraction (unchanged core logic, enhanced with framework_role)
    // -------------------------------------------------------------------

    pub(super) fn extract_function(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let params_node = node.child_by_field_name("parameters");
        let params = params_node
            .as_ref()
            .and_then(|n| n.utf8_text(source).ok())
            .unwrap_or("()");
        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };
        let kind = if container.is_some() {
            SymbolKind::Method
        } else {
            SymbolKind::Function
        };

        // Extract TS type annotations if available
        let (param_types, param_count) = Self::extract_ts_param_info(params_node.as_ref(), source);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.trim().trim_start_matches(':').trim().to_string())
            .filter(|s| !s.is_empty());

        let mut sym = self.make_symbol(
            node,
            file_path,
            name,
            kind,
            &qname,
            container,
            Some(&format!("function {}{}", name, params)),
            source,
        );
        sym.receiver_type = container.map(String::from);
        sym.param_types = param_types;
        sym.return_type = return_type;
        sym.param_count = param_count;
        Some(sym)
    }

    pub(super) fn extract_class(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        Some(self.make_symbol(
            node,
            file_path,
            name,
            SymbolKind::Class,
            name,
            None,
            Some(&format!("class {}", name)),
            source,
        ))
    }

    pub(super) fn extract_method(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        container: Option<&str>,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        let qname = match container {
            Some(c) => format!("{}.{}", c, name),
            None => name.to_string(),
        };

        // Extract param info for methods
        let params_node = node.child_by_field_name("parameters");
        let (param_types, param_count) = Self::extract_ts_param_info(params_node.as_ref(), source);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.trim().trim_start_matches(':').trim().to_string())
            .filter(|s| !s.is_empty());

        let mut sym = self.make_symbol(
            node,
            file_path,
            name,
            SymbolKind::Method,
            &qname,
            container,
            None,
            source,
        );
        sym.receiver_type = container.map(String::from);
        sym.param_types = param_types;
        sym.return_type = return_type;
        sym.param_count = param_count;
        Some(sym)
    }

    /// Extract TypeScript parameter type annotations from a function's parameters node.
    /// For plain JS this will typically return (None, Some(count)).
    fn extract_ts_param_info(
        params_node: Option<&tree_sitter::Node>,
        source: &[u8],
    ) -> (Option<String>, Option<u32>) {
        let params = match params_node {
            Some(n) => n,
            None => return (None, Some(0)),
        };
        let mut types = Vec::new();
        let mut count = 0u32;
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            match child.kind() {
                "required_parameter" | "optional_parameter" | "rest_parameter" => {
                    count += 1;
                    // Try to get the type annotation
                    if let Some(type_ann) = child.child_by_field_name("type") {
                        if let Ok(t) = type_ann.utf8_text(source) {
                            let t = t.trim().trim_start_matches(':').trim();
                            if !t.is_empty() {
                                types.push(t.to_string());
                            }
                        }
                    }
                }
                // Plain JS parameters (identifier nodes)
                "identifier" | "assignment_pattern" | "object_pattern" | "array_pattern" => {
                    count += 1;
                }
                _ => {}
            }
        }
        let pt = if types.is_empty() {
            None
        } else {
            Some(types.join(", "))
        };
        (pt, Some(count))
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn extract_variable_declarations(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        in_export: bool,
        is_default_export: bool,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "variable_declarator" {
                // CommonJS require() import extraction:
                // const X = require('mod')  or  const { A, B } = require('./mod')
                if let Some(val_node) = child.child_by_field_name("value") {
                    if val_node.kind() == "call_expression" {
                        let callee = val_node.child(0).and_then(|c| node_text(&c, source));
                        if callee == Some("require") {
                            if let Some(src) = self.extract_first_string_arg(&val_node, source) {
                                if let Some(name_node) = child.child_by_field_name("name") {
                                    if name_node.kind() == "object_pattern" {
                                        // const { A, B } = require('./mod')
                                        let mut oc = name_node.walk();
                                        for prop in name_node.named_children(&mut oc) {
                                            if prop.kind()
                                                == "shorthand_property_identifier_pattern"
                                                || prop.kind() == "shorthand_property_identifier"
                                            {
                                                if let Some(iname) = node_text(&prop, source) {
                                                    ctx.imports.push(
                                                        crate::import_common::make_import(
                                                            file_path,
                                                            src.clone(),
                                                            Some(iname.to_string()),
                                                            None,
                                                            false,
                                                        ),
                                                    );
                                                }
                                            } else if prop.kind() == "pair_pattern"
                                                || prop.kind() == "pair"
                                            {
                                                // const { A: alias } = require(...)
                                                let key = prop
                                                    .child_by_field_name("key")
                                                    .and_then(|k| node_text(&k, source));
                                                let value = prop
                                                    .child_by_field_name("value")
                                                    .and_then(|v| node_text(&v, source));
                                                if let (Some(k), Some(v)) = (key, value) {
                                                    ctx.imports.push(
                                                        crate::import_common::make_import(
                                                            file_path,
                                                            src.clone(),
                                                            Some(k.to_string()),
                                                            Some(v.to_string()),
                                                            false,
                                                        ),
                                                    );
                                                }
                                            }
                                        }
                                    } else {
                                        // const X = require('mod') — default/namespace import
                                        if let Some(iname) = node_text(&name_node, source) {
                                            ctx.imports.push(ImportRecord {
                                                file_path: file_path.to_string(),
                                                import_string: src.clone(),
                                                resolved_path: None,
                                                imported_name: Some(iname.to_string()),
                                                alias: None,
                                                is_namespace: true,
                                                is_default: true,
                                                is_reexport: false,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(name_node) = child.child_by_field_name("name") {
                    // useState setter detection: const [state, setState] = useState(...)
                    if name_node.kind() == "array_pattern" {
                        let value_node = child.child_by_field_name("value");
                        if let Some(ref val) = value_node {
                            if val.kind() == "call_expression" {
                                let callee_text = val
                                    .child(0)
                                    .and_then(|c| node_text(&c, source))
                                    .unwrap_or("");
                                if callee_text == "useState" {
                                    // Extract the setter name (second element of array pattern)
                                    let mut ap_cursor = name_node.walk();
                                    let elements: Vec<_> =
                                        name_node.named_children(&mut ap_cursor).collect();
                                    if elements.len() >= 2 {
                                        if let Some(setter_name) = node_text(&elements[1], source) {
                                            let line = child.start_position().row as u32 + 1;
                                            let col = child.start_position().column as u32;
                                            ctx.dispatch_sites.push(DispatchSiteRecord {
                                                site_id: StableId::edge_id(
                                                    "dsite", file_path, line, col,
                                                ),
                                                file_path: file_path.to_string(),
                                                line,
                                                col,
                                                enclosing_symbol_uid: ctx
                                                    .current_symbol_uid
                                                    .clone(),
                                                receiver_expr: None,
                                                site_kind: DispatchSiteKind::StateSetterBinding,
                                                key: setter_name.to_string(),
                                                handler_expr: None,
                                                handler_symbol_uid: None,
                                                confidence: STATE_SETTER_BINDING_CONFIDENCE,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        // Visit RHS for calls/routes even for destructured vars
                        if let Some(val) = value_node {
                            self.visit_expression_tree(&val, source, file_path, ctx, container);
                        }
                    } else if let Ok(name) = name_node.utf8_text(source) {
                        // Check if RHS is arrow function or function expression
                        let value_node = child.child_by_field_name("value");
                        let kind = value_node
                            .as_ref()
                            .map(|v| match v.kind() {
                                "arrow_function" | "function" | "function_expression" => {
                                    SymbolKind::Function
                                }
                                _ => SymbolKind::Variable,
                            })
                            .unwrap_or(SymbolKind::Variable);

                        let qname = match container {
                            Some(c) => format!("{}.{}", c, name),
                            None => name.to_string(),
                        };

                        // Each declarator owns its identity/span, not the whole const
                        // statement. Multiple bindings must not share an id; an
                        // arrow body must map back to the variable that names it.
                        let mut sym = self.make_symbol(
                            &child, file_path, name, kind, &qname, container, None, source,
                        );
                        sym.framework_role = classify_framework_role(name, kind, container);
                        if in_export {
                            sym.export_name = Some(name.to_string());
                            sym.is_default_export = is_default_export;
                        }
                        ctx.symbols.push(sym);

                        // Visit RHS for calls/routes
                        if let Some(val) = value_node {
                            self.visit_expression_tree(&val, source, file_path, ctx, container);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn make_symbol(
        &self,
        node: &tree_sitter::Node,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        qname: &str,
        container: Option<&str>,
        signature: Option<&str>,
        _source: &[u8],
    ) -> SymbolRecord {
        let sid = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let uid = StableId::symbol_uid(file_path, qname, kind.as_str(), signature);
        SymbolRecord {
            symbol_id: sid,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line: node.start_position().row as u32 + 1,
            end_line: node.end_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_col: node.end_position().column as u32,
            signature: signature.map(String::from),
            doc: None,
            parser_tier: ParserTier::Semantic,
            parser_confidence: ParserTier::Semantic.element_confidence(ElementKind::Symbol),
            qname: Some(qname.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(uid),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }
}
