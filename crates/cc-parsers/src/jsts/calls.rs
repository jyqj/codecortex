//! Call-edge extraction for JS/TS — member and identifier call
//! expressions, constructor calls, HTTP client / fetch detection, broker
//! calls, EventEmitter registration/dispatch and React state-setter sites.

use super::extras::{extract_handler_expr, is_event_dispatch, is_event_registration};
use super::routes::{ROUTER_OBJECTS, ROUTE_METHODS};
use super::{
    child_by_kind, count_args, extract_fetch_method, node_text, ExtractCtx, JsTsParser,
    AST_CALL_CONFIDENCE, JS_KEYWORDS, STATE_SETTER_NAME_CONFIDENCE,
};
use crate::http_call_helpers::*;
use cc_model::dispatch_site::{DispatchSiteKind, DispatchSiteRecord};
use cc_model::edge::{CallEdgeRecord, DispatchKind, HttpCallEdgeRecord, ResolutionKind};
use cc_model::id::StableId;
use cc_model::{ElementKind, ParserTier};

impl JsTsParser {
    // -------------------------------------------------------------------
    // Call expression analysis
    // -------------------------------------------------------------------

    pub(super) fn visit_call_expression(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        is_awaited: bool,
    ) {
        let func_node = match node.child(0) {
            Some(n) => n,
            None => return,
        };

        let is_optional = func_node.kind() == "optional_chain_expression"
            || func_node.kind().contains("optional");

        if func_node.kind() == "member_expression" {
            self.handle_member_call_expression(
                node,
                &func_node,
                source,
                file_path,
                ctx,
                container,
                is_awaited,
                is_optional,
            );
            return;
        }

        if func_node.kind() == "identifier" {
            self.handle_identifier_call_expression(
                node, &func_node, source, file_path, ctx, container, is_awaited,
            );
            return;
        }

        // Nested call: foo()()
        if func_node.kind() == "call_expression" {
            self.visit_call_expression(&func_node, source, file_path, ctx, container, is_awaited);
        }
    }

    /// Handle `obj.method(args)` call expressions (member expression callee).
    #[allow(clippy::too_many_arguments)]
    fn handle_member_call_expression(
        &self,
        node: &tree_sitter::Node,
        func_node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        is_awaited: bool,
        is_optional: bool,
    ) {
        let obj_node = func_node.child(0);
        let prop_node = {
            let mut found = None;
            let mut cursor = func_node.walk();
            for ch in func_node.children(&mut cursor) {
                if matches!(ch.kind(), "property_identifier" | "identifier")
                    && Some(ch.id()) != obj_node.as_ref().map(|n| n.id())
                {
                    found = Some(ch);
                    break;
                }
            }
            found
        };

        let (obj, prop) = match (obj_node, prop_node) {
            (Some(o), Some(p)) => (o, p),
            _ => return,
        };

        let obj_text = node_text(&obj, source).unwrap_or("");
        let prop_text = node_text(&prop, source).unwrap_or("");
        let callee = format!("{}.{}", obj_text, prop_text);

        let obj_lower = obj_text.to_lowercase();
        let prop_lower = prop_text.to_lowercase();
        let is_router = ROUTER_OBJECTS.contains(&obj_lower.as_str());

        // Middleware detection: app.use(handler)
        if is_router && prop_lower == "use" {
            self.try_extract_middleware(node, obj_text, source, file_path, ctx);
        }
        // Route detection: app.get("/path", handler)
        else if is_router && ROUTE_METHODS.contains(&prop_lower.as_str()) {
            self.try_extract_route(node, obj_text, prop_text, source, file_path, ctx);
        }
        // Broker detection: producer.send(), pubsub.publish(), etc.
        else if self.try_emit_broker_edge(node, obj_text, prop_text, source, file_path, ctx) {
            // Broker edge already emitted inside try_emit_broker_edge.
        }
        // HTTP client call detection: axios.get("/api/users"), client.post(url)
        else if is_http_client_object(obj_text) && is_http_verb_method(prop_text) {
            if let Some(url) = self.extract_first_string_arg(node, source) {
                if looks_like_url_or_path(&url) {
                    let line = node.start_position().row as u32 + 1;
                    let col = node.start_position().column as u32;
                    let normalized = normalize_template_to_path(&url);
                    ctx.http_call_edges.push(HttpCallEdgeRecord {
                        edge_id: StableId::edge_id("http_call", file_path, line, col),
                        file_path: file_path.to_string(),
                        caller_symbol_uid: ctx.current_symbol_uid.clone(),
                        url_or_path: url.to_string(),
                        normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                            &normalized,
                        )),
                        method: infer_http_method(prop_text).map(|m| m.to_string()),
                        call_kind: "http".to_string(),
                        line,
                        confidence: ParserTier::TreeSitter
                            .element_confidence(ElementKind::HttpCall),
                        parser_tier: ParserTier::TreeSitter,
                        broker_type: None,
                    });
                }
            }
        }

        // EventEmitter registration: emitter.on('event', handler)
        if is_event_registration(prop_text) {
            if let Some(event_name) = self.extract_first_string_arg(node, source) {
                let handler_expr = extract_handler_expr(node, source);
                let line = node.start_position().row as u32 + 1;
                let col = node.start_position().column as u32;
                ctx.dispatch_sites.push(DispatchSiteRecord {
                    site_id: StableId::edge_id("dsite", file_path, line, col),
                    file_path: file_path.to_string(),
                    line,
                    col,
                    enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                    receiver_expr: Some(obj_text.to_string()),
                    site_kind: DispatchSiteKind::EventOn,
                    key: event_name,
                    handler_expr,
                    handler_symbol_uid: None,
                    confidence: ParserTier::Semantic.element_confidence(ElementKind::DispatchSite),
                });
            }
        }

        // EventEmitter dispatch: emitter.emit('event', data)
        if is_event_dispatch(prop_text) {
            if let Some(event_name) = self.extract_first_string_arg(node, source) {
                let line = node.start_position().row as u32 + 1;
                let col = node.start_position().column as u32;
                ctx.dispatch_sites.push(DispatchSiteRecord {
                    site_id: StableId::edge_id("dsite", file_path, line, col),
                    file_path: file_path.to_string(),
                    line,
                    col,
                    enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                    receiver_expr: Some(obj_text.to_string()),
                    site_kind: DispatchSiteKind::EventEmit,
                    key: event_name,
                    handler_expr: None,
                    handler_symbol_uid: None,
                    confidence: ParserTier::Semantic.element_confidence(ElementKind::DispatchSite),
                });
            }
        }

        // Class component this.setState() detection
        if obj_text == "this" && prop_text == "setState" {
            let line = node.start_position().row as u32 + 1;
            let col = node.start_position().column as u32;
            ctx.dispatch_sites.push(DispatchSiteRecord {
                site_id: StableId::edge_id("dsite", file_path, line, col),
                file_path: file_path.to_string(),
                line,
                col,
                enclosing_symbol_uid: ctx.current_symbol_uid.clone(),
                receiver_expr: Some("this".to_string()),
                site_kind: DispatchSiteKind::StateSetterCall,
                key: "setState".to_string(),
                handler_expr: None,
                handler_symbol_uid: None,
                confidence: ParserTier::Semantic.element_confidence(ElementKind::DispatchSite),
            });
        }

        // Always emit call edge for member expressions
        let arg_count = count_args(node);
        let dispatch = if is_optional {
            DispatchKind::OptionalChain
        } else {
            DispatchKind::Dynamic // member dispatch
        };
        ctx.call_edges.push(CallEdgeRecord {
            edge_id: StableId::edge_id(
                "call",
                file_path,
                node.start_position().row as u32 + 1,
                node.start_position().column as u32,
            ),
            file_path: file_path.to_string(),
            caller_symbol: container.map(String::from),
            callee_symbol: callee,
            line: node.start_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_line: Some(node.end_position().row as u32 + 1),
            end_col: node.end_position().column as u32,
            target_symbol_id: None,
            target_file_path: None,
            caller_symbol_id: None,
            caller_symbol_uid: ctx.current_symbol_uid.clone(),
            callee_symbol_uid: None,
            callee_ref_id: None,
            dispatch_kind: dispatch,
            call_kind: "member".into(),
            resolution_kind: ResolutionKind::Unresolved,
            resolution_confidence: 0.0,
            resolution_strategy: "unresolved".into(),
            receiver_expr: Some(obj_text.to_string()),
            arg_count: Some(arg_count),
            is_optional_chain: is_optional,
            is_awaited,
            is_constructor: false,
            parser_tier: ParserTier::Semantic,
            parser_confidence: AST_CALL_CONFIDENCE,
            synthesized_by: None,
            synthesis_key: None,
            registered_file: None,
            registered_line: None,
        });
    }

    /// Handle `identifier(args)` call expressions (direct function call).
    #[allow(clippy::too_many_arguments)]
    fn handle_identifier_call_expression(
        &self,
        node: &tree_sitter::Node,
        func_node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
        is_awaited: bool,
    ) {
        let callee = node_text(func_node, source).unwrap_or("");
        if JS_KEYWORDS.contains(&callee) {
            return;
        }

        // Standalone HTTP call detection: fetch("/api/users")
        if is_standalone_http_call(callee) {
            if let Some(url) = self.extract_first_string_arg(node, source) {
                if looks_like_url_or_path(&url) {
                    let line = node.start_position().row as u32 + 1;
                    let col = node.start_position().column as u32;
                    let normalized = normalize_template_to_path(&url);
                    ctx.http_call_edges.push(HttpCallEdgeRecord {
                        edge_id: StableId::edge_id("http_call", file_path, line, col),
                        file_path: file_path.to_string(),
                        caller_symbol_uid: ctx.current_symbol_uid.clone(),
                        url_or_path: url.to_string(),
                        normalized_path: Some(cc_model::route_normalize::normalize_route_path(
                            &normalized,
                        )),
                        method: Some(
                            extract_fetch_method(node, source).unwrap_or_else(|| "GET".to_string()),
                        ),
                        call_kind: "http".to_string(),
                        line,
                        confidence: ParserTier::TreeSitter
                            .element_confidence(ElementKind::HttpCall),
                        parser_tier: ParserTier::TreeSitter,
                        broker_type: None,
                    });
                }
            }
        }

        // React state setter call detection: setCount(...), setName(...), etc.
        // Heuristic: identifier starting with "set" followed by an uppercase letter.
        let callee_bytes = callee.as_bytes();
        if callee_bytes.len() > 3
            && callee_bytes.starts_with(b"set")
            && callee_bytes[3].is_ascii_uppercase()
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
                site_kind: DispatchSiteKind::StateSetterCall,
                key: callee.to_string(),
                handler_expr: None,
                handler_symbol_uid: None,
                confidence: STATE_SETTER_NAME_CONFIDENCE,
            });
        }

        let arg_count = count_args(node);
        ctx.call_edges.push(CallEdgeRecord {
            edge_id: StableId::edge_id(
                "call",
                file_path,
                node.start_position().row as u32 + 1,
                node.start_position().column as u32,
            ),
            file_path: file_path.to_string(),
            caller_symbol: container.map(String::from),
            callee_symbol: callee.to_string(),
            line: node.start_position().row as u32 + 1,
            start_col: node.start_position().column as u32,
            end_line: Some(node.end_position().row as u32 + 1),
            end_col: node.end_position().column as u32,
            target_symbol_id: None,
            target_file_path: None,
            caller_symbol_id: None,
            caller_symbol_uid: ctx.current_symbol_uid.clone(),
            callee_symbol_uid: None,
            callee_ref_id: None,
            dispatch_kind: DispatchKind::Direct,
            call_kind: "direct".into(),
            resolution_kind: ResolutionKind::Unresolved,
            resolution_confidence: 0.0,
            resolution_strategy: "unresolved".into(),
            receiver_expr: None,
            arg_count: Some(arg_count),
            is_optional_chain: false,
            is_awaited,
            is_constructor: false,
            parser_tier: ParserTier::Semantic,
            parser_confidence: AST_CALL_CONFIDENCE,
            synthesized_by: None,
            synthesis_key: None,
            registered_file: None,
            registered_line: None,
        });
    }

    /// Extract the first string/template argument from a call expression.
    /// Returns the stripped string value, or None if the first arg is not a literal.
    pub(super) fn extract_first_string_arg(
        &self,
        call_node: &tree_sitter::Node,
        source: &[u8],
    ) -> Option<String> {
        let args_node = child_by_kind(call_node, "arguments")?;
        let mut cursor = args_node.walk();
        let first_arg = args_node
            .children(&mut cursor)
            .find(|c| !matches!(c.kind(), "(" | ")" | ","))?;
        match first_arg.kind() {
            "string" => {
                let text = node_text(&first_arg, source)?;
                Some(strip_string_delimiters(text).to_string())
            }
            "template_string" => {
                let text = node_text(&first_arg, source)?;
                let stripped = strip_string_delimiters(text);
                Some(normalize_template_to_path(stripped))
            }
            _ => None,
        }
    }

    /// Handle: new Foo(args)
    pub(super) fn visit_new_expression(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        ctx: &mut ExtractCtx,
        container: Option<&str>,
    ) {
        // children: "new" <constructor> <arguments>
        let constructor_node = if node.child_count() >= 2 {
            let first = node.child(0).unwrap();
            if first.kind() == "new" {
                node.child(1)
            } else {
                Some(first)
            }
        } else {
            None
        };

        if let Some(con) = constructor_node {
            let name = node_text(&con, source).unwrap_or("");
            if !name.is_empty() && !JS_KEYWORDS.contains(&name) {
                let arg_count = count_args(node);
                ctx.call_edges.push(CallEdgeRecord {
                    edge_id: StableId::edge_id(
                        "call",
                        file_path,
                        node.start_position().row as u32 + 1,
                        node.start_position().column as u32,
                    ),
                    file_path: file_path.to_string(),
                    caller_symbol: container.map(String::from),
                    callee_symbol: name.to_string(),
                    line: node.start_position().row as u32 + 1,
                    start_col: node.start_position().column as u32,
                    end_line: Some(node.end_position().row as u32 + 1),
                    end_col: node.end_position().column as u32,
                    target_symbol_id: None,
                    target_file_path: None,
                    caller_symbol_id: None,
                    caller_symbol_uid: ctx.current_symbol_uid.clone(),
                    callee_symbol_uid: None,
                    callee_ref_id: None,
                    dispatch_kind: DispatchKind::Constructor,
                    call_kind: "constructor".into(),
                    resolution_kind: ResolutionKind::Unresolved,
                    resolution_confidence: 0.0,
                    resolution_strategy: "unresolved".into(),
                    receiver_expr: None,
                    arg_count: Some(arg_count),
                    is_optional_chain: false,
                    is_awaited: false,
                    is_constructor: true,
                    parser_tier: ParserTier::Semantic,
                    parser_confidence: AST_CALL_CONFIDENCE,
                    synthesized_by: None,
                    synthesis_key: None,
                    registered_file: None,
                    registered_line: None,
                });
            }
        }
    }
}
