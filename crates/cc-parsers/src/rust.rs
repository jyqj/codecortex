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
}
