//! Rust parser using tree-sitter-rust.

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::edge::{
    CallEdgeRecord, DataFlowEdgeRecord, DispatchKind, ImportRecord, ResolutionKind,
    SemanticEdgeRecord, SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static IMPL_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"impl(?:\s*<[^>]+>)?(?:\s+[^\{]+?\s+for\s+)?\s*([A-Za-z_][A-Za-z0-9_]*)")
        .expect("valid impl regex")
});

/// Matches `impl Trait for Struct` — captures trait name and struct name.
static RS_IMPL_TRAIT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^\s*impl(?:\s*<[^>]*>)?\s+([A-Za-z_][A-Za-z0-9_:]*)\s+for\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("rust impl trait re")
});

/// Matches `#[derive(Trait1, Trait2)]` — captures the list inside parens.
static RS_DERIVE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s*#\[derive\(([^)]+)\)\]").expect("rust derive re"));
static RS_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_:]*)\s*\(").expect("rust call regex"));
static RS_IDENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]*\b").expect("rust ident regex"));
static RS_KEYWORDS: &[&str] = &[
    "fn", "let", "mut", "pub", "impl", "struct", "enum", "trait", "use", "mod", "match", "if",
    "else", "loop", "while", "for", "return", "self", "Self", "crate", "super",
];

/// Matches `env::var("KEY")`, `env::var_os("KEY")`, `std::env::var("KEY")`, etc.
static RUST_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?:std::)?env::var(?:_os)?\("(\w+)"\)"#).expect("rust env access regex")
});

pub struct RustParser {
    language: tree_sitter::Language,
    chunker: Chunker,
}

impl RustParser {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_rust::LANGUAGE.into(),
            chunker: Chunker::default(),
        }
    }

    fn extract_symbols(
        &self,
        tree: &tree_sitter::Tree,
        source: &[u8],
        file_path: &str,
    ) -> (Vec<SymbolRecord>, Vec<ImportRecord>) {
        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            self.visit_node(&child, source, file_path, &mut symbols, &mut imports, None);
        }
        (symbols, imports)
    }

    fn visit_node(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        symbols: &mut Vec<SymbolRecord>,
        imports: &mut Vec<ImportRecord>,
        container: Option<&str>,
    ) {
        match node.kind() {
            "function_item" => {
                if let Some(sym) = self.extract_function(node, source, file_path, container) {
                    symbols.push(sym);
                }
            }
            "struct_item" => {
                if let Some(sym) =
                    self.extract_named_item(node, source, file_path, SymbolKind::Class, "struct")
                {
                    symbols.push(sym);
                }
            }
            "enum_item" => {
                if let Some(sym) =
                    self.extract_named_item(node, source, file_path, SymbolKind::Enum, "enum")
                {
                    symbols.push(sym);
                }
            }
            "trait_item" => {
                if let Some(sym) =
                    self.extract_named_item(node, source, file_path, SymbolKind::Interface, "trait")
                {
                    symbols.push(sym);
                }
            }
            "impl_item" => {
                let impl_container = self.impl_container_name(node, source);
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(
                        &child,
                        source,
                        file_path,
                        symbols,
                        imports,
                        impl_container.as_deref(),
                    );
                }
            }
            "declaration_list" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    self.visit_node(&child, source, file_path, symbols, imports, container);
                }
            }
            "use_declaration" => {
                if let Some(imp) = self.extract_import(node, source, file_path) {
                    imports.push(imp);
                }
            }
            _ => {}
        }
    }

    fn extract_function(
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

        // Extract parameter types and return type from tree-sitter nodes
        let (param_types, param_count) =
            Self::extract_rust_param_info(params_node.as_ref(), source);
        let return_type = node
            .child_by_field_name("return_type")
            .and_then(|n| n.utf8_text(source).ok())
            .map(|s| s.trim().to_string());

        let mut sym = self.make_symbol(
            node,
            file_path,
            name,
            kind,
            &qname,
            container,
            Some(&format!("fn {}{}", name, params)),
        );
        sym.receiver_type = container.map(String::from);
        sym.param_types = param_types;
        sym.return_type = return_type;
        sym.param_count = param_count;
        Some(sym)
    }

    /// Extract parameter type info from a Rust function's parameters node.
    fn extract_rust_param_info(
        params_node: Option<&tree_sitter::Node>,
        source: &[u8],
    ) -> (Option<String>, Option<u32>) {
        let params = match params_node {
            Some(n) => n,
            None => return (None, Some(0)),
        };
        let mut types = Vec::new();
        let mut cursor = params.walk();
        for child in params.children(&mut cursor) {
            match child.kind() {
                "parameter" => {
                    // pattern: type
                    if let Some(type_node) = child.child_by_field_name("type") {
                        if let Ok(t) = type_node.utf8_text(source) {
                            types.push(t.trim().to_string());
                        }
                    }
                }
                "self_parameter" => {
                    // &self, &mut self, self — skip counting as a user param
                }
                _ => {}
            }
        }
        let count = types.len() as u32;
        let pt = if types.is_empty() {
            None
        } else {
            Some(types.join(", "))
        };
        (pt, Some(count))
    }

    fn extract_named_item(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
        kind: SymbolKind,
        keyword: &str,
    ) -> Option<SymbolRecord> {
        let name_node = node.child_by_field_name("name")?;
        let name = name_node.utf8_text(source).ok()?;
        Some(self.make_symbol(
            node,
            file_path,
            name,
            kind,
            name,
            None,
            Some(&format!("{} {}", keyword, name)),
        ))
    }

    fn extract_import(
        &self,
        node: &tree_sitter::Node,
        source: &[u8],
        file_path: &str,
    ) -> Option<ImportRecord> {
        let text = node.utf8_text(source).ok()?;
        let import_string = text
            .trim()
            .trim_start_matches("use ")
            .trim_end_matches(';')
            .trim()
            .to_string();
        Some(ImportRecord {
            file_path: file_path.to_string(),
            import_string,
            resolved_path: None,
            imported_name: None,
            alias: None,
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        })
    }

    fn impl_container_name(&self, node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
        let text = node.utf8_text(source).ok()?;
        IMPL_TYPE_RE
            .captures(text)
            .and_then(|cap| cap.get(1).map(|m| m.as_str().to_string()))
    }

    fn extract_refs_and_calls(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let keywords: HashSet<&str> = RS_KEYWORDS.iter().copied().collect();
        let mut refs = Vec::new();
        let mut calls = Vec::new();

        let mut by_name: HashMap<String, (&str, &str)> = HashMap::new();
        for sym in symbols {
            if let Some(uid) = &sym.symbol_uid {
                by_name
                    .entry(sym.name.clone())
                    .or_insert((sym.symbol_id.as_str(), uid.as_str()));
            }
        }

        for sym in symbols
            .iter()
            .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        {
            let start = sym.start_line.saturating_sub(1) as usize;
            let end = (sym.end_line as usize).min(lines.len());
            for (offset, line) in lines[start..end].iter().enumerate() {
                let line_no = (start + offset + 1) as u32;
                let mut call_starts = HashSet::new();
                for cap in RS_CALL_RE.captures_iter(line) {
                    let Some(m) = cap.get(1) else { continue };
                    let raw = m.as_str();
                    let callee = raw.split("::").last().unwrap_or(raw);
                    if keywords.contains(callee) {
                        continue;
                    }
                    let start_col = m.start() as u32;
                    call_starts.insert(start_col);
                    let target = by_name.get(callee);
                    let ref_id = StableId::ref_id(file_path, callee, line_no, start_col);
                    refs.push(SymbolRefRecord {
                        ref_id: ref_id.clone(),
                        file_path: file_path.to_string(),
                        symbol_name: callee.to_string(),
                        container: sym.qname.clone(),
                        ref_kind: "call".into(),
                        line: line_no,
                        column: start_col,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        ref_name: Some(callee.to_string()),
                        scope_id: sym.scope_id.clone(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        ref_end_line: Some(line_no),
                        ref_end_col: Some(m.end() as u32),
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.7,
                    });
                    calls.push(CallEdgeRecord {
                        edge_id: StableId::edge_id("call", file_path, line_no, start_col),
                        file_path: file_path.to_string(),
                        caller_symbol: Some(sym.name.clone()),
                        callee_symbol: callee.to_string(),
                        line: line_no,
                        start_col,
                        end_line: Some(line_no),
                        end_col: m.end() as u32,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        caller_symbol_id: Some(sym.symbol_id.clone()),
                        caller_symbol_uid: sym.symbol_uid.clone(),
                        callee_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        callee_ref_id: Some(ref_id),
                        dispatch_kind: DispatchKind::Direct,
                        call_kind: "direct".into(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        receiver_expr: None,
                        arg_count: None,
                        is_optional_chain: false,
                        is_awaited: false,
                        is_constructor: false,
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.7,
                        synthesized_by: None,
                        synthesis_key: None,
                        registered_file: None,
                        registered_line: None,
                    });
                }

                for m in RS_IDENT_RE.find_iter(line) {
                    let ident = m.as_str();
                    if keywords.contains(ident)
                        || (line_no == sym.start_line && ident == sym.name)
                        || call_starts.contains(&(m.start() as u32))
                    {
                        continue;
                    }
                    let target = by_name.get(ident);
                    refs.push(SymbolRefRecord {
                        ref_id: StableId::ref_id(file_path, ident, line_no, m.start() as u32),
                        file_path: file_path.to_string(),
                        symbol_name: ident.to_string(),
                        container: sym.qname.clone(),
                        ref_kind: "identifier".into(),
                        line: line_no,
                        column: m.start() as u32,
                        target_symbol_id: target.map(|(sid, _)| (*sid).to_string()),
                        target_file_path: target.map(|_| file_path.to_string()),
                        target_symbol_uid: target.map(|(_, uid)| (*uid).to_string()),
                        ref_name: Some(ident.to_string()),
                        scope_id: sym.scope_id.clone(),
                        resolution_kind: if target.is_some() {
                            ResolutionKind::Exact
                        } else {
                            ResolutionKind::Unresolved
                        },
                        resolution_confidence: if target.is_some() { 1.0 } else { 0.0 },
                        resolution_strategy: if target.is_some() {
                            "parser_exact".into()
                        } else {
                            "unresolved".into()
                        },
                        ref_end_line: Some(line_no),
                        ref_end_col: Some(m.end() as u32),
                        parser_tier: ParserTier::Semantic,
                        parser_confidence: 0.6,
                    });
                }
            }
        }

        (refs, calls)
    }

    fn extract_semantic_edges(
        &self,
        content: &str,
        file_path: &str,
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let lines: Vec<&str> = content.lines().collect();
        let mut edges = Vec::new();

        // impl Trait for Struct
        for cap in RS_IMPL_TRAIT_RE.captures_iter(content) {
            let trait_name = &cap[1];
            let struct_name = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            // Use the last segment for path-qualified traits (e.g., std::fmt::Display -> Display)
            let trait_short = trait_name.split("::").last().unwrap_or(trait_name);
            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:implements:{}", file_path, line, trait_short),
                file_path: file_path.to_string(),
                source_symbol: struct_name.to_string(),
                source_symbol_uid: None,
                target_symbol: trait_name.to_string(),
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Implements,
                line,
                confidence: 0.95,
                parser_tier: tier,
            });
        }

        // #[derive(Trait1, Trait2)]
        for cap in RS_DERIVE_RE.captures_iter(content) {
            let derive_list = &cap[1];
            let m = cap.get(0).unwrap();
            let line_idx = content[..m.start()].matches('\n').count();
            let line = line_idx as u32 + 1;

            // Find the struct/enum name on the next non-attribute, non-blank line
            let mut target_name = String::new();
            for next_line in lines.iter().skip(line_idx + 1) {
                let trimmed = next_line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
                    continue;
                }
                // Look for struct/enum/union name
                if let Some(pos) = trimmed.find("struct ") {
                    let after = &trimmed[pos + 7..];
                    if let Some(name_end) = after.find(|c: char| !c.is_alphanumeric() && c != '_') {
                        target_name = after[..name_end].to_string();
                    } else {
                        target_name = after.trim().to_string();
                    }
                } else if let Some(pos) = trimmed.find("enum ") {
                    let after = &trimmed[pos + 5..];
                    if let Some(name_end) = after.find(|c: char| !c.is_alphanumeric() && c != '_') {
                        target_name = after[..name_end].to_string();
                    } else {
                        target_name = after.trim().to_string();
                    }
                }
                break;
            }
            if target_name.is_empty() {
                continue;
            }

            for trait_name in derive_list.split(',') {
                let trait_name = trait_name.trim();
                if trait_name.is_empty() {
                    continue;
                }
                edges.push(SemanticEdgeRecord {
                    edge_id: format!("se-{}:{}:decorates:{}", file_path, line, trait_name),
                    file_path: file_path.to_string(),
                    source_symbol: trait_name.to_string(),
                    source_symbol_uid: None,
                    target_symbol: target_name.clone(),
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Decorates,
                    line,
                    confidence: 0.95,
                    parser_tier: tier,
                });
            }
        }

        edges
    }

    #[allow(clippy::too_many_arguments)]
    fn make_symbol(
        &self,
        node: &tree_sitter::Node,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        qname: &str,
        container: Option<&str>,
        signature: Option<&str>,
    ) -> SymbolRecord {
        let symbol_id = StableId::edge_id(
            "sym",
            file_path,
            node.start_position().row as u32 + 1,
            node.start_position().column as u32,
        );
        let symbol_uid = StableId::symbol_uid(file_path, qname, kind.as_str(), signature);
        SymbolRecord {
            symbol_id,
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
            parser_confidence: 0.8,
            qname: Some(qname.to_string()),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(symbol_uid),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    // =========================================================================
    // Environment variable access extraction
    // =========================================================================

    /// Extract environment variable accesses from Rust code.
    ///
    /// Matches `env::var("KEY")`, `env::var_os("KEY")`, `std::env::var("KEY")`, etc.
    fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let mut edges = Vec::new();

        for cap in RUST_ENV_ACCESS_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            let env_key = cap.get(1).map(|m| m.as_str().to_string());

            let source_uid = symbols
                .iter()
                .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
                .filter(|s| s.start_line <= line && s.end_line >= line)
                .min_by_key(|s| s.end_line - s.start_line)
                .and_then(|s| s.symbol_uid.clone());

            edges.push(DataFlowEdgeRecord {
                edge_id: StableId::edge_id("dfe", file_path, line, m.start() as u32),
                file_path: file_path.to_string(),
                source_symbol_uid: source_uid,
                target_symbol_uid: None,
                flow_kind: "env_access".to_string(),
                line,
                confidence: 0.80,
                parser_tier: ParserTier::Heuristic,
                env_key,
            });
        }

        edges
    }
}

impl Default for RustParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for RustParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: e.to_string(),
            })?;

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "tree-sitter parse failed".to_string(),
            })?;

        let (symbols, imports) = self.extract_symbols(&tree, content.as_bytes(), file_path);
        let (symbol_refs, call_edges) = self.extract_refs_and_calls(content, file_path, &symbols);
        let tier = ParserTier::Semantic;
        let confidence = 0.8;
        let semantic_edges = self.extract_semantic_edges(content, file_path, tier);
        let data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (rust, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );
        let is_test = file_path.contains("/tests/") || file_path.ends_with("_test.rs");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            data_flow_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn parse_with_timeout(
        &self,
        file_path: &str,
        content: &str,
        language: Language,
        timeout_micros: Option<u64>,
    ) -> CcResult<ParseOutcome> {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&self.language)
            .map_err(|e| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: e.to_string(),
            })?;
        if let Some(timeout) = timeout_micros {
            parser.set_timeout_micros(timeout);
        }

        let tree = parser
            .parse(content, None)
            .ok_or_else(|| cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: if timeout_micros.is_some() {
                    "tree-sitter parse failed or timed out".to_string()
                } else {
                    "tree-sitter parse failed".to_string()
                },
            })?;

        let (symbols, imports) = self.extract_symbols(&tree, content.as_bytes(), file_path);
        let (symbol_refs, call_edges) = self.extract_refs_and_calls(content, file_path, &symbols);
        let tier = ParserTier::Semantic;
        let confidence = 0.8;
        let semantic_edges = self.extract_semantic_edges(content, file_path, tier);
        let data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (rust, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );
        let is_test = file_path.contains("/tests/") || file_path.ends_with("_test.rs");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            data_flow_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::Rust]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Semantic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_rust() {
        let p = RustParser::new();
        let code = r#"
use crate::db::IndexDb;

struct Greeter;

impl Greeter {
    fn greet(&self, name: &str) -> String {
        format!("hello {}", name)
    }
}

fn top_level() {}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Greeter"));
        assert!(names.contains(&"greet"));
        assert!(names.contains(&"top_level"));
        assert!(!outcome.imports.is_empty());
        assert!(!outcome.chunks.is_empty());
    }

    #[test]
    fn parse_impl_block_and_trait_definition() {
        let p = RustParser::new();
        let code = r#"
trait MyTrait {
    fn do_something(&self);
    fn with_default(&self) {}
}

struct Foo;

impl Foo {
    fn bar(&self) {}
    fn baz(x: u32) -> u32 { x }
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();

        // trait is extracted as Interface
        assert!(
            names.contains(&"MyTrait"),
            "missing MyTrait, got: {:?}",
            names
        );
        let trait_sym = outcome
            .symbols
            .iter()
            .find(|s| s.name == "MyTrait")
            .unwrap();
        assert_eq!(trait_sym.kind, SymbolKind::Interface);

        // struct
        assert!(names.contains(&"Foo"), "missing Foo, got: {:?}", names);
        let foo_sym = outcome.symbols.iter().find(|s| s.name == "Foo").unwrap();
        assert_eq!(foo_sym.kind, SymbolKind::Class);

        // methods inside impl Foo are Methods with container = Foo
        let bar = outcome.symbols.iter().find(|s| s.name == "bar").unwrap();
        assert_eq!(bar.kind, SymbolKind::Method);
        assert_eq!(bar.container.as_deref(), Some("Foo"));
        assert_eq!(bar.qname.as_deref(), Some("Foo.bar"));

        let baz = outcome.symbols.iter().find(|s| s.name == "baz").unwrap();
        assert_eq!(baz.kind, SymbolKind::Method);
        assert_eq!(baz.container.as_deref(), Some("Foo"));
    }

    #[test]
    fn parse_async_fn() {
        let p = RustParser::new();
        let code = r#"
async fn fetch_data(url: &str) -> Result<String, Error> {
    let resp = reqwest::get(url).await?;
    resp.text().await
}

fn sync_fn() {}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"fetch_data"),
            "missing fetch_data, got: {:?}",
            names
        );
        assert!(
            names.contains(&"sync_fn"),
            "missing sync_fn, got: {:?}",
            names
        );
        let fetch = outcome
            .symbols
            .iter()
            .find(|s| s.name == "fetch_data")
            .unwrap();
        assert_eq!(fetch.kind, SymbolKind::Function);
        // Return type should be captured
        assert!(
            fetch.return_type.is_some(),
            "async fn should have return_type"
        );
    }

    #[test]
    fn parse_macro_call_edges() {
        let p = RustParser::new();
        let code = r#"
fn main() {
    println!("hello");
    let v = vec![1, 2, 3];
    my_macro!(v);
}
"#;
        let outcome = p.parse("src/main.rs", code, Language::Rust).unwrap();
        // Macro calls like println! are matched by the call regex as "println!"
        // but the regex captures identifiers before '(' — so println! with '(' triggers it
        // The regex `([A-Za-z_][A-Za-z0-9_:]*)\s*\(` does not see `!` before `(`
        // so `println!("hello")` won't match as a call, but `my_macro!(v)` also won't.
        // However, call edges should still be found for non-macro calls in other tests.
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"), "missing main, got: {:?}", names);
    }

    #[test]
    fn parse_module_and_use_statements() {
        let p = RustParser::new();
        let code = r#"
use std::collections::HashMap;
use crate::bar::*;
use super::baz::Quux;

fn process() {}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        assert_eq!(
            outcome.imports.len(),
            3,
            "expected 3 imports, got: {:?}",
            outcome
                .imports
                .iter()
                .map(|i| &i.import_string)
                .collect::<Vec<_>>()
        );

        let hashmap_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string.contains("HashMap"))
            .expect("missing HashMap import");
        assert_eq!(hashmap_imp.import_string, "std::collections::HashMap");

        let glob_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string.contains("bar"))
            .expect("missing bar glob import");
        assert_eq!(glob_imp.import_string, "crate::bar::*");

        let super_imp = outcome
            .imports
            .iter()
            .find(|i| i.import_string.contains("Quux"))
            .expect("missing super import");
        assert_eq!(super_imp.import_string, "super::baz::Quux");
    }

    #[test]
    fn parse_method_receiver_types() {
        let p = RustParser::new();
        let code = r#"
struct Widget;

impl Widget {
    fn by_ref(&self) {}
    fn by_mut_ref(&mut self) {}
    fn by_value(self) {}
    fn static_method(x: i32) -> i32 { x }
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();

        // All methods inside impl Widget should have container/receiver_type = Widget
        for name in &["by_ref", "by_mut_ref", "by_value", "static_method"] {
            let sym = outcome
                .symbols
                .iter()
                .find(|s| s.name == *name)
                .unwrap_or_else(|| panic!("missing method {}", name));
            assert_eq!(sym.kind, SymbolKind::Method, "{} should be Method", name);
            assert_eq!(
                sym.container.as_deref(),
                Some("Widget"),
                "{} container",
                name
            );
            assert_eq!(
                sym.receiver_type.as_deref(),
                Some("Widget"),
                "{} receiver_type",
                name
            );
        }

        // self parameters should NOT count toward param_count
        let by_ref = outcome.symbols.iter().find(|s| s.name == "by_ref").unwrap();
        assert_eq!(
            by_ref.param_count,
            Some(0),
            "&self should not count as a parameter"
        );

        let static_m = outcome
            .symbols
            .iter()
            .find(|s| s.name == "static_method")
            .unwrap();
        assert_eq!(
            static_m.param_count,
            Some(1),
            "static_method should have 1 param"
        );
        assert_eq!(static_m.param_types.as_deref(), Some("i32"));
    }

    #[test]
    fn parse_test_file_detection() {
        let p = RustParser::new();
        let code = "fn dummy() {}\n";

        // Regular files
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        assert!(!outcome.is_test_file);

        let outcome = p.parse("src/parser.rs", code, Language::Rust).unwrap();
        assert!(!outcome.is_test_file);

        // Test files
        let outcome = p.parse("src/parser_test.rs", code, Language::Rust).unwrap();
        assert!(outcome.is_test_file);

        // The parser checks for "/tests/" (with slashes) in the path
        let outcome = p
            .parse("project/tests/integration.rs", code, Language::Rust)
            .unwrap();
        assert!(outcome.is_test_file);
    }

    #[test]
    fn parse_call_edges() {
        let p = RustParser::new();
        let code = r#"
fn helper(x: i32) -> i32 {
    x + 1
}

fn caller() {
    let a = helper(42);
    let b = helper(a);
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );

        // Calls to helper from caller
        let caller_to_helper: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.callee_symbol == "helper" && e.caller_symbol.as_deref() == Some("caller"))
            .collect();
        assert!(
            caller_to_helper.len() >= 2,
            "expected at least 2 calls from caller to helper, got: {}",
            caller_to_helper.len()
        );

        // Symbol refs should also include call refs
        let call_refs: Vec<_> = outcome
            .symbol_refs
            .iter()
            .filter(|r| r.symbol_name == "helper" && r.ref_kind == "call")
            .collect();
        assert!(
            call_refs.len() >= 2,
            "expected at least 2 call refs to helper"
        );
    }

    #[test]
    fn parse_semantic_edges_impl_trait() {
        let p = RustParser::new();
        let code = r#"
trait Drawable {
    fn draw(&self);
}

struct Circle;

impl Drawable for Circle {
    fn draw(&self) {}
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let impl_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Implements)
            .collect();
        assert!(
            !impl_edges.is_empty(),
            "should detect impl Trait for Struct"
        );
        assert!(
            impl_edges
                .iter()
                .any(|e| e.source_symbol == "Circle" && e.target_symbol == "Drawable"),
            "missing Circle -> Drawable implements, got: {:?}",
            impl_edges
        );
    }

    #[test]
    fn parse_semantic_edges_impl_trait_with_generics() {
        let p = RustParser::new();
        let code = r#"
struct MyVec<T>(Vec<T>);

impl<T: Clone> std::fmt::Display for MyVec<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MyVec")
    }
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let impl_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Implements)
            .collect();
        assert!(
            impl_edges
                .iter()
                .any(|e| e.source_symbol == "MyVec" && e.target_symbol == "std::fmt::Display"),
            "missing MyVec -> std::fmt::Display implements, got: {:?}",
            impl_edges
        );
    }

    #[test]
    fn parse_env_var_access() {
        let p = RustParser::new();
        let code = r#"
fn load_config() {
    let db_url = std::env::var("DATABASE_URL").unwrap();
    let api_key = env::var("API_KEY").expect("missing");
    let opt = env::var_os("OPTIONAL_VAR");
}
"#;
        let outcome = p.parse("src/config.rs", code, Language::Rust).unwrap();
        assert!(
            !outcome.data_flow_edges.is_empty(),
            "should detect env var accesses"
        );

        let env_keys: Vec<&str> = outcome
            .data_flow_edges
            .iter()
            .filter_map(|e| e.env_key.as_deref())
            .collect();
        assert!(
            env_keys.contains(&"DATABASE_URL"),
            "missing DATABASE_URL, got: {:?}",
            env_keys
        );
        assert!(
            env_keys.contains(&"API_KEY"),
            "missing API_KEY, got: {:?}",
            env_keys
        );
        assert!(
            env_keys.contains(&"OPTIONAL_VAR"),
            "missing OPTIONAL_VAR, got: {:?}",
            env_keys
        );

        // Each env access should have a source_symbol_uid since load_config is parsed
        for edge in &outcome.data_flow_edges {
            assert_eq!(edge.flow_kind, "env_access");
            assert!(
                edge.source_symbol_uid.is_some(),
                "env access should have source_symbol_uid"
            );
        }
    }

    #[test]
    fn parse_derive_macros() {
        let p = RustParser::new();
        let code = r#"
#[derive(Debug, Clone, Serialize)]
struct Config {
    name: String,
}

#[derive(PartialEq)]
enum Status {
    Active,
    Inactive,
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let derive_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Decorates)
            .collect();

        // Config should have Debug, Clone, Serialize
        let config_derives: Vec<&str> = derive_edges
            .iter()
            .filter(|e| e.target_symbol == "Config")
            .map(|e| e.source_symbol.as_str())
            .collect();
        assert!(
            config_derives.contains(&"Debug"),
            "missing Debug for Config, got: {:?}",
            config_derives
        );
        assert!(
            config_derives.contains(&"Clone"),
            "missing Clone for Config, got: {:?}",
            config_derives
        );
        assert!(
            config_derives.contains(&"Serialize"),
            "missing Serialize for Config, got: {:?}",
            config_derives
        );

        // Status should have PartialEq
        let status_derives: Vec<&str> = derive_edges
            .iter()
            .filter(|e| e.target_symbol == "Status")
            .map(|e| e.source_symbol.as_str())
            .collect();
        assert!(
            status_derives.contains(&"PartialEq"),
            "missing PartialEq for Status, got: {:?}",
            status_derives
        );
    }

    #[test]
    fn parse_enum_definition() {
        let p = RustParser::new();
        let code = r#"
enum Color {
    Red,
    Green,
    Blue,
}

enum Option<T> {
    Some(T),
    None,
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Color"), "missing Color, got: {:?}", names);
        assert!(
            names.contains(&"Option"),
            "missing Option, got: {:?}",
            names
        );

        let color = outcome.symbols.iter().find(|s| s.name == "Color").unwrap();
        assert_eq!(color.kind, SymbolKind::Enum);
        assert!(
            color
                .signature
                .as_deref()
                .unwrap_or("")
                .contains("enum Color"),
            "enum signature should contain 'enum Color'"
        );
    }

    #[test]
    fn parse_generic_function() {
        let p = RustParser::new();
        let code = r#"
fn process<T: Display>(item: T) -> String {
    item.to_string()
}

fn multi_generic<T, U>(a: T, b: U) -> (T, U) {
    (a, b)
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"process"),
            "missing process, got: {:?}",
            names
        );
        assert!(
            names.contains(&"multi_generic"),
            "missing multi_generic, got: {:?}",
            names
        );

        let process = outcome
            .symbols
            .iter()
            .find(|s| s.name == "process")
            .unwrap();
        assert_eq!(process.kind, SymbolKind::Function);
        assert_eq!(process.param_count, Some(1));
        assert_eq!(process.param_types.as_deref(), Some("T"));

        let multi = outcome
            .symbols
            .iter()
            .find(|s| s.name == "multi_generic")
            .unwrap();
        assert_eq!(multi.param_count, Some(2));
        assert_eq!(multi.param_types.as_deref(), Some("T, U"));
    }

    #[test]
    fn parse_const_and_static() {
        let p = RustParser::new();
        // Note: the current parser does not extract const/static as symbols.
        // This test documents that behavior and verifies no crash occurs.
        let code = r#"
const MAX: u32 = 100;
static COUNTER: u32 = 0;

fn use_them() -> u32 {
    MAX + COUNTER
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        // Function should still be extracted
        assert!(
            names.contains(&"use_them"),
            "missing use_them, got: {:?}",
            names
        );
        // Parser currently does not extract const/static — this is expected behavior
        // The test ensures parsing doesn't crash on these constructs
    }

    #[test]
    fn parse_return_type_extraction() {
        let p = RustParser::new();
        let code = r#"
fn no_return() {}

fn returns_u32() -> u32 {
    42
}

fn returns_result() -> Result<String, Error> {
    Ok("ok".to_string())
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();

        let no_ret = outcome
            .symbols
            .iter()
            .find(|s| s.name == "no_return")
            .unwrap();
        assert_eq!(
            no_ret.return_type, None,
            "no_return should have no return type"
        );

        let ret_u32 = outcome
            .symbols
            .iter()
            .find(|s| s.name == "returns_u32")
            .unwrap();
        assert!(
            ret_u32.return_type.as_deref().unwrap_or("").contains("u32"),
            "returns_u32 should have u32 return type, got: {:?}",
            ret_u32.return_type
        );

        let ret_result = outcome
            .symbols
            .iter()
            .find(|s| s.name == "returns_result")
            .unwrap();
        assert!(
            ret_result
                .return_type
                .as_deref()
                .unwrap_or("")
                .contains("Result"),
            "returns_result should have Result return type, got: {:?}",
            ret_result.return_type
        );
    }

    #[test]
    fn parse_nested_impl_methods() {
        let p = RustParser::new();
        let code = r#"
struct Server {
    port: u16,
}

impl Server {
    fn new(port: u16) -> Self {
        Server { port }
    }

    fn start(&self) {
        self.listen();
    }

    fn listen(&self) {}
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();

        // All methods should be extracted
        let method_names: Vec<&str> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method)
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            method_names.contains(&"new"),
            "missing new, got: {:?}",
            method_names
        );
        assert!(
            method_names.contains(&"start"),
            "missing start, got: {:?}",
            method_names
        );
        assert!(
            method_names.contains(&"listen"),
            "missing listen, got: {:?}",
            method_names
        );

        // new() should have return type Self
        let new_method = outcome.symbols.iter().find(|s| s.name == "new").unwrap();
        assert!(
            new_method
                .return_type
                .as_deref()
                .unwrap_or("")
                .contains("Self"),
            "new should return Self, got: {:?}",
            new_method.return_type
        );
        assert_eq!(new_method.param_count, Some(1));

        // Call from start -> listen should be detected
        let listen_calls: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.callee_symbol == "listen")
            .collect();
        assert!(
            !listen_calls.is_empty(),
            "should detect call from start to listen"
        );
    }

    #[test]
    fn parse_multiple_impls_for_same_struct() {
        let p = RustParser::new();
        let code = r#"
struct Point {
    x: f64,
    y: f64,
}

impl Point {
    fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }
}

impl std::fmt::Display for Point {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {})", self.x, self.y)
    }
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Point"), "missing Point, got: {:?}", names);
        assert!(names.contains(&"new"), "missing new, got: {:?}", names);
        assert!(names.contains(&"fmt"), "missing fmt, got: {:?}", names);

        // Both methods should belong to Point
        let new_sym = outcome.symbols.iter().find(|s| s.name == "new").unwrap();
        assert_eq!(new_sym.container.as_deref(), Some("Point"));

        let fmt_sym = outcome.symbols.iter().find(|s| s.name == "fmt").unwrap();
        assert_eq!(fmt_sym.container.as_deref(), Some("Point"));

        // Should have semantic edge: Point implements Display
        let impl_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Implements)
            .collect();
        assert!(
            impl_edges
                .iter()
                .any(|e| e.source_symbol == "Point" && e.target_symbol == "std::fmt::Display"),
            "missing Point -> Display implements, got: {:?}",
            impl_edges
        );
    }

    #[test]
    fn parse_parser_tier_and_confidence() {
        let p = RustParser::new();
        let code = "fn foo() {}\n";
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Semantic);
        assert!((outcome.parser_confidence - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn parse_with_timeout_works() {
        let p = RustParser::new();
        let code = r#"
fn hello() -> &'static str {
    "world"
}
"#;
        // Large timeout should succeed
        let outcome = p
            .parse_with_timeout("src/lib.rs", code, Language::Rust, Some(5_000_000))
            .unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"), "missing hello, got: {:?}", names);
    }

    #[test]
    fn parse_complex_use_tree() {
        let p = RustParser::new();
        let code = r#"
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Write};

fn process() {}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        // Grouped use statements should be captured
        assert!(
            outcome.imports.len() >= 2,
            "expected at least 2 imports, got: {}",
            outcome.imports.len()
        );

        // The import strings should contain the full tree syntax
        let import_strings: Vec<&str> = outcome
            .imports
            .iter()
            .map(|i| i.import_string.as_str())
            .collect();
        assert!(
            import_strings
                .iter()
                .any(|s| s.contains("HashMap") || s.contains("collections")),
            "missing collections import, got: {:?}",
            import_strings
        );
    }

    #[test]
    fn parse_call_edges_cross_function() {
        let p = RustParser::new();
        let code = r#"
fn alpha() -> i32 {
    beta() + gamma()
}

fn beta() -> i32 {
    gamma()
}

fn gamma() -> i32 {
    42
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();

        // alpha calls beta and gamma
        let alpha_to_beta: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.caller_symbol.as_deref() == Some("alpha") && e.callee_symbol == "beta")
            .collect();
        assert!(!alpha_to_beta.is_empty(), "missing call edge alpha -> beta");

        let alpha_to_gamma: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.caller_symbol.as_deref() == Some("alpha") && e.callee_symbol == "gamma")
            .collect();
        assert!(
            !alpha_to_gamma.is_empty(),
            "missing call edge alpha -> gamma"
        );

        // beta calls gamma
        let beta_to_gamma: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.caller_symbol.as_deref() == Some("beta") && e.callee_symbol == "gamma")
            .collect();
        assert!(!beta_to_gamma.is_empty(), "missing call edge beta -> gamma");

        // Calls to known symbols should be resolved
        for call in &alpha_to_beta {
            assert_eq!(call.resolution_kind, ResolutionKind::Exact);
            assert!((call.resolution_confidence - 1.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn parse_unresolved_call_edges() {
        let p = RustParser::new();
        let code = r#"
fn my_fn() {
    external_call(42);
}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        let ext_calls: Vec<_> = outcome
            .call_edges
            .iter()
            .filter(|e| e.callee_symbol == "external_call")
            .collect();
        assert!(!ext_calls.is_empty(), "should detect call to external_call");
        for call in &ext_calls {
            assert_eq!(
                call.resolution_kind,
                ResolutionKind::Unresolved,
                "external call should be unresolved"
            );
            assert!(
                call.target_symbol_id.is_none(),
                "unresolved call should have no target_symbol_id"
            );
        }
    }

    #[test]
    fn parse_summary_format() {
        let p = RustParser::new();
        let code = r#"
struct A;
struct B;
fn foo() {}
"#;
        let outcome = p.parse("src/lib.rs", code, Language::Rust).unwrap();
        assert!(
            outcome.summary.contains("src/lib.rs"),
            "summary should contain file path"
        );
        assert!(
            outcome.summary.contains("rust"),
            "summary should contain 'rust'"
        );
        assert!(
            outcome.summary.contains("3 symbols"),
            "summary should contain '3 symbols', got: {}",
            outcome.summary
        );
    }

    #[test]
    fn parse_supported_languages() {
        let p = RustParser::new();
        assert_eq!(p.supported_languages(), &[Language::Rust]);
        assert_eq!(p.tier(), ParserTier::Semantic);
    }

    #[test]
    fn parse_empty_file() {
        let p = RustParser::new();
        let outcome = p.parse("src/empty.rs", "", Language::Rust).unwrap();
        assert!(outcome.symbols.is_empty());
        assert!(outcome.imports.is_empty());
        assert!(outcome.call_edges.is_empty());
        assert!(outcome.semantic_edges.is_empty());
        assert!(outcome.data_flow_edges.is_empty());
    }
}
