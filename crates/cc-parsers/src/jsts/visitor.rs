//! Top-level AST traversal for JS/TS — walks declarations, class bodies
//! (with decorator handling) and expression trees, dispatching to the
//! symbol / call-edge / route / import extraction submodules as it descends.

use super::routes::NESTJS_DECORATORS;
use super::{
    child_by_kind, classify_framework_role, node_text, ExtractCtx, ExtractResult, JsTsParser,
};
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::id::StableId;
use cc_model::symbol::SymbolKind;
use cc_model::{ElementKind, ParserTier};

impl JsTsParser {
    // -------------------------------------------------------------------
    // Top-level AST extraction
    // -------------------------------------------------------------------

    pub(super) fn extract_all(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> ExtractResult {
        let mut ctx = ExtractCtx::new(file_path);
        let root = tree.root_node();
        self.visit_node(&root, source, file_path, &mut ctx, None, false, false);
        // Apply pending exports
        self.apply_pending_exports(&mut ctx);
        ctx
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_node(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        in_export: bool,
        is_default_export: bool,
    ) {
        match node.kind() {
            "function_declaration" | "generator_function_declaration" => {
                let prev_uid = ctx.current_symbol_uid.take();
                let mut func_name: Option<String> = None;
                if let Some(mut sym) = self.extract_function(node, source, file_path, container) {
                    // Framework role
                    if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, container);
                    }
                    if in_export {
                        sym.export_name = Some(sym.name.clone());
                        sym.is_default_export = is_default_export;
                    }
                    func_name = Some(sym.name.clone());
                    ctx.current_symbol_uid = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit function body for calls/routes — use function name as container
                let body_container = func_name.as_deref().or(container);
                if let Some(body) = node.child_by_field_name("body") {
                    self.visit_expression_tree(&body, source, file_path, ctx, body_container);
                }
                // Children visited via generic walk below; uid restored after walk
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(&child, source, file_path, ctx, body_container, false, false);
                }
                ctx.current_symbol_uid = prev_uid;
                return;
            }
            "class_declaration" => {
                if let Some(mut sym) = self.extract_class(node, source, file_path) {
                    if sym.framework_role.is_none() {
                        sym.framework_role = classify_framework_role(&sym.name, sym.kind, None);
                    }
                    if in_export {
                        sym.export_name = Some(sym.name.clone());
                        sym.is_default_export = is_default_export;
                    }
                    let name = sym.name.clone();
                    ctx.symbols.push(sym);
                    if let Some(body) = node.child_by_field_name("body") {
                        self.visit_class_body(&body, source, file_path, ctx, &name);
                    }
                    return; // already visited children
                }
            }
            "method_definition" => {
                let prev_uid = ctx.current_symbol_uid.take();
                let mut method_name: Option<String> = None;
                if let Some(mut sym) = self.extract_method(node, source, file_path, container) {
                    if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, container);
                    }
                    method_name = Some(sym.name.clone());
                    ctx.current_symbol_uid = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit method body for calls/routes — use method name as container
                let body_container = method_name.as_deref().or(container);
                if let Some(body) = child_by_kind(node, "statement_block") {
                    self.visit_expression_tree(&body, source, file_path, ctx, body_container);
                }
                // Children visited via generic walk below; uid restored after walk
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(&child, source, file_path, ctx, body_container, false, false);
                }
                ctx.current_symbol_uid = prev_uid;
                return;
            }
            "lexical_declaration" | "variable_declaration" => {
                self.extract_variable_declarations(
                    node,
                    source,
                    file_path,
                    ctx,
                    container,
                    in_export,
                    is_default_export,
                );
            }
            "export_statement" => {
                let (export_name, is_default) = self.visit_export(node, source, ctx, file_path);
                let _ = export_name; // used via pending_exports
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    let k = child.kind();
                    if !matches!(
                        k,
                        "export"
                            | "default"
                            | "string"
                            | "export_clause"
                            | ";"
                            | "{"
                            | "}"
                            | ","
                            | "from"
                            | "*"
                    ) {
                        self.visit_node(
                            &child, source, file_path, ctx, container, true, is_default,
                        );
                    }
                }
                return;
            }
            "import_statement" => {
                if let Some(imp) = self.extract_import(node, source, file_path) {
                    // Check if import path matches a known broker pattern
                    if let Some(broker_match) =
                        crate::broker_patterns::match_broker(&imp.import_string)
                    {
                        // Register all imported names as broker-associated
                        if let Some(ref name) = imp.imported_name {
                            // For `import X from "kafkajs"`, X is broker-associated
                            ctx.broker_imports
                                .insert(name.clone(), broker_match.broker_type.to_string());
                        }
                        if let Some(ref alias) = imp.alias {
                            ctx.broker_imports
                                .insert(alias.clone(), broker_match.broker_type.to_string());
                        }
                    }
                    let import_idx = ctx.imports.len();
                    for binding in Self::collect_import_local_bindings(node, source) {
                        ctx.import_bindings.entry(binding).or_insert(import_idx);
                    }
                    ctx.imports.push(imp);
                }
            }
            "expression_statement" => {
                // Visit expression children for call/route extraction
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
                return;
            }
            _ => {}
        }

        // Visit children (except for class which we handle above)
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.visit_node(&child, source, file_path, ctx, container, false, false);
        }
    }

    // -------------------------------------------------------------------
    // Class body with decorator support
    // -------------------------------------------------------------------

    fn visit_class_body(
        &self,
        body: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        class_name: &str,
    ) {
        let mut pending_decorators: Vec<tree_sitter::Node> = Vec::new();
        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            if member.kind() == "decorator" {
                pending_decorators.push(member);
                continue;
            }

            let decorators = std::mem::take(&mut pending_decorators);

            if member.kind() == "method_definition" {
                let mut sym_uid_for_body: Option<String> = None;
                if let Some(mut sym) =
                    self.extract_method(&member, source, file_path, Some(class_name))
                {
                    // Check decorators for NestJS route handlers
                    let mut fw_role: Option<String> = None;
                    for dec in &decorators {
                        if let Some(dec_text) = node_text(dec, source) {
                            let dec_clean = dec_text.trim_start_matches('@');
                            let dec_name = dec_clean
                                .split('(')
                                .next()
                                .unwrap_or("")
                                .trim()
                                .to_lowercase();
                            if NESTJS_DECORATORS
                                .iter()
                                .any(|(k, _)| *k == dec_name.as_str())
                            {
                                fw_role = Some("route_handler".to_string());
                                self.try_extract_nestjs_decorator_route(
                                    dec, &member, class_name, source, file_path, ctx,
                                );
                            }
                        }
                    }
                    if fw_role.is_some() {
                        sym.framework_role = fw_role;
                    } else if sym.framework_role.is_none() {
                        sym.framework_role =
                            classify_framework_role(&sym.name, sym.kind, Some(class_name));
                    }
                    sym_uid_for_body = sym.symbol_uid.clone();
                    ctx.symbols.push(sym);
                }
                // Visit method body
                if let Some(block) = child_by_kind(&member, "statement_block") {
                    let prev_uid = ctx.current_symbol_uid.take();
                    ctx.current_symbol_uid = sym_uid_for_body;
                    self.visit_expression_tree(&block, source, file_path, ctx, Some(class_name));
                    ctx.current_symbol_uid = prev_uid;
                }
            } else if member.kind() == "field_definition" {
                // field_definition may contain arrow functions
                if let Some(name_node) = member.child_by_field_name("name") {
                    if let Some(name) = node_text(&name_node, source) {
                        // Check for arrow function value
                        let mut has_fn = false;
                        let mut inner_cursor = member.walk();
                        for child in member.children(&mut inner_cursor) {
                            if matches!(child.kind(), "arrow_function" | "function_expression") {
                                has_fn = true;
                                let params = child
                                    .child_by_field_name("parameters")
                                    .and_then(|n| n.utf8_text(source).ok())
                                    .unwrap_or("()");
                                let qname = format!("{}.{}", class_name, name);
                                let mut sym = self.make_symbol(
                                    &member,
                                    file_path,
                                    name,
                                    SymbolKind::Method,
                                    &qname,
                                    Some(class_name),
                                    Some(&format!("{}{}", name, params)),
                                    source,
                                );
                                sym.framework_role = classify_framework_role(
                                    name,
                                    SymbolKind::Method,
                                    Some(class_name),
                                );
                                let field_sym_uid = sym.symbol_uid.clone();
                                ctx.symbols.push(sym);
                                // Visit body
                                if let Some(block) = child_by_kind(&child, "statement_block") {
                                    let prev_uid = ctx.current_symbol_uid.take();
                                    ctx.current_symbol_uid = field_sym_uid;
                                    self.visit_expression_tree(
                                        &block,
                                        source,
                                        file_path,
                                        ctx,
                                        Some(class_name),
                                    );
                                    ctx.current_symbol_uid = prev_uid;
                                }
                                break;
                            }
                        }
                        if !has_fn {
                            // Non-function field — still visit value for calls
                            let mut inner_cursor2 = member.walk();
                            for child in member.children(&mut inner_cursor2) {
                                if child.kind() != "property_identifier"
                                    && child.kind() != "="
                                    && child.kind() != ";"
                                {
                                    self.visit_expression_tree(
                                        &child,
                                        source,
                                        file_path,
                                        ctx,
                                        Some(class_name),
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------
    // Expression tree walking — extracts calls, routes, literals
    // -------------------------------------------------------------------

    pub(super) fn visit_expression_tree(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
    ) {
        match node.kind() {
            "call_expression" => {
                self.visit_call_expression(node, source, file_path, ctx, container, false);
                // Also visit arguments for nested refs
                if let Some(args) = child_by_kind(node, "arguments") {
                    let mut cursor = args.walk();
                    for child in args.children(&mut cursor) {
                        if !matches!(child.kind(), "(" | ")" | ",") {
                            self.visit_expression_tree(&child, source, file_path, ctx, container);
                        }
                    }
                }
            }
            "new_expression" => {
                self.visit_new_expression(node, source, file_path, ctx, container);
            }
            "await_expression" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "call_expression" {
                        self.visit_call_expression(&child, source, file_path, ctx, container, true);
                    } else if child.kind() != "await" {
                        self.visit_expression_tree(&child, source, file_path, ctx, container);
                    }
                }
            }
            "string" => {
                if let Some(text) = node_text(node, source) {
                    let value =
                        text.trim_matches(|c: char| c == '\'' || c == '"' || c == '\u{0060}');
                    if (3..=160).contains(&value.len()) {
                        self.add_literal(value, node, file_path, ctx, container);
                    }
                }
            }
            "template_string" => {
                if let Some(text) = node_text(node, source) {
                    let value = text.trim_matches('\u{0060}');
                    if (3..=160).contains(&value.len()) {
                        self.add_literal(value, node, file_path, ctx, container);
                    }
                }
            }
            // JSX tag extraction: <ComponentName /> or <ComponentName>...</ComponentName>
            "jsx_opening_element" | "jsx_self_closing_element" => {
                if let Some(tag_node) = node.child_by_field_name("name") {
                    let tag_text = node_text(&tag_node, source).unwrap_or("");
                    // Only PascalCase = user components (not div, span, etc.)
                    if !tag_text.is_empty()
                        && tag_text.chars().next().is_some_and(|c| c.is_uppercase())
                    {
                        let line = node.start_position().row as u32 + 1;
                        let col = node.start_position().column as u32;
                        ctx.dispatch_sites.push(DispatchSiteRecord {
                            site_id: StableId::edge_id("dsite", file_path, line, col),
                            file_path: file_path.to_string(),
                            line,
                            col,
                            enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                            receiver_expr: None,
                            site_kind: DispatchSiteKind::JsxTag,
                            key: tag_text.to_string(),
                            handler_expr: None,
                            handler_symbol_uid: None,
                            confidence: ParserTier::Semantic
                                .element_confidence(ElementKind::DispatchSite),
                        });
                    }
                }
                // Recurse into children for nested JSX expressions
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            // JSX element containers: recurse into children to find nested JSX tags
            "jsx_element" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            // Named function expressions passed as callbacks: set up proper caller context
            "function_expression" | "function" => {
                let func_name = node
                    .child_by_field_name("name")
                    .and_then(|n| node_text(&n, source))
                    .map(|s| s.to_string());
                if let Some(ref fname) = func_name {
                    if !fname.is_empty() {
                        // Check if this symbol was already registered (e.g., by try_extract_route)
                        let already_registered = ctx
                            .symbols
                            .iter()
                            .any(|s| s.name == *fname && s.file_path == file_path);
                        if !already_registered {
                            let qname = fname.to_string();
                            let sym = self.make_symbol(
                                node,
                                file_path,
                                fname,
                                SymbolKind::Function,
                                &qname,
                                container,
                                None,
                                source,
                            );
                            let prev_uid = ctx.current_symbol_uid.take();
                            ctx.current_symbol_uid = sym.symbol_uid.clone();
                            ctx.symbols.push(sym);
                            // Visit body with the function as container
                            if let Some(body) = node.child_by_field_name("body") {
                                self.visit_expression_tree(
                                    &body,
                                    source,
                                    file_path,
                                    ctx,
                                    Some(fname),
                                );
                            }
                            ctx.current_symbol_uid = prev_uid;
                            return; // Already handled children
                        } else {
                            // Already registered — just set the UID context and visit body
                            let existing_uid = ctx
                                .symbols
                                .iter()
                                .rev()
                                .find(|s| s.name == *fname && s.file_path == file_path)
                                .and_then(|s| s.symbol_uid.clone());
                            let prev_uid = ctx.current_symbol_uid.take();
                            ctx.current_symbol_uid = existing_uid;
                            if let Some(body) = node.child_by_field_name("body") {
                                self.visit_expression_tree(
                                    &body,
                                    source,
                                    file_path,
                                    ctx,
                                    Some(fname),
                                );
                            }
                            ctx.current_symbol_uid = prev_uid;
                            return; // Already handled children
                        }
                    }
                }
                // Anonymous function: just recurse
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
            _ => {
                // Recurse into all children
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_expression_tree(&child, source, file_path, ctx, container);
                }
            }
        }
    }
}
