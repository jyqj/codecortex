//! Go parser using regex-based outline extraction (Heuristic tier).

use crate::chunker::Chunker;
use crate::traits::FileParser;
use cc_model::edge::{
    CallEdgeRecord, DispatchKind, ImportRecord, ResolutionKind, SemanticEdgeRecord,
    SemanticRelation,
};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord, SymbolRefRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

// --- regex patterns ---

/// Matches top-level functions:  `func Name(`
/// and methods:                  `func (recv Type) Name(`
static GO_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^func\s+(?:\([^)]*\)\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("go func re")
});

/// Matches method receiver:  `func (recv Type) Name(`
static GO_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^func\s+\(\s*\w+\s+\*?([A-Za-z_][A-Za-z0-9_]*)\s*\)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    )
    .expect("go method re")
});

/// `type Name struct`
static GO_STRUCT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+struct\b").expect("go struct re")
});

/// `type Name interface`
static GO_INTERFACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+interface\b").expect("go interface re")
});

/// `type Name = ...` or `type Name underlying` (post-filter struct/interface)
static GO_TYPEDEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^type\s+([A-Za-z_][A-Za-z0-9_]*)\s+(\S+)").expect("go typedef re")
});

/// Single-line import: `import "path"`
static GO_IMPORT_SINGLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?m)^import\s+"([^"]+)""#).expect("go import single re"));

/// Multi-line import block entries: `"path"` inside `import ( ... )`
static GO_IMPORT_ENTRY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^\s+(?:[A-Za-z_][A-Za-z0-9_]*\s+)?"([^"]+)""#).expect("go import entry re")
});

/// Matches struct embedding: a bare type name inside a struct body (e.g., `Bar` in `type Foo struct { Bar }`).
static GO_EMBED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^\s+\*?([A-Z][A-Za-z0-9_]*)\s*$").expect("go embed re"));

/// Matches full Go function signature: captures params and optional return type.
/// `func Name(params) returnType`  or  `func (recv Type) Name(params) (returnTypes)`
static GO_FUNC_SIG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^func\s+(?:\([^)]*\)\s+)?[A-Za-z_][A-Za-z0-9_]*\s*\(([^)]*)\)\s*(.*?)\s*\{")
        .expect("go func sig re")
});

/// Call pattern: `name(`
static GO_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("go call re"));

static GO_KEYWORDS: &[&str] = &[
    "func",
    "var",
    "const",
    "type",
    "struct",
    "interface",
    "map",
    "chan",
    "go",
    "select",
    "case",
    "switch",
    "if",
    "else",
    "for",
    "range",
    "return",
    "break",
    "continue",
    "defer",
    "import",
    "package",
    "fallthrough",
    "goto",
    "default",
    "make",
    "new",
    "len",
    "cap",
    "append",
    "copy",
    "delete",
    "close",
    "panic",
    "recover",
    "print",
    "println",
    "nil",
    "true",
    "false",
    "iota",
    "string",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "uint",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "float32",
    "float64",
    "bool",
    "byte",
    "rune",
    "error",
];

pub struct GoParser {
    chunker: Chunker,
}

impl GoParser {
    pub fn new() -> Self {
        Self {
            chunker: Chunker::default(),
        }
    }

    /// Extract Go parameter types from a raw params string like "a int, b string, c bool".
    /// Returns comma-separated types like "int, string, bool".
    fn extract_go_param_types(raw: &str) -> String {
        if raw.trim().is_empty() {
            return String::new();
        }
        // Split by comma, each part is "name type" or just "type" (for unnamed params)
        raw.split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                // Last word is the type (handles "a int", "b *MyStruct", "c ...string")
                let tokens: Vec<&str> = part.split_whitespace().collect();
                if tokens.len() >= 2 {
                    Some(tokens[tokens.len() - 1].to_string())
                } else if tokens.len() == 1 {
                    // Could be just a type (unnamed parameter) or a single-token like "error"
                    Some(tokens[0].to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Find the end line of a brace-delimited block starting at `start_line` (0-indexed).
    fn find_block_end(lines: &[&str], start_line: usize) -> usize {
        let mut depth: i32 = 0;
        for (i, line) in lines[start_line..].iter().enumerate() {
            for ch in line.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth <= 0 {
                            return start_line + i;
                        }
                    }
                    _ => {}
                }
            }
        }
        // If no closing brace found, return last line
        lines.len().saturating_sub(1)
    }

    fn extract_symbols_and_imports(
        &self,
        content: &str,
        file_path: &str,
    ) -> (Vec<SymbolRecord>, Vec<ImportRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        // --- imports ---
        for cap in GO_IMPORT_SINGLE_RE.captures_iter(content) {
            imports.push(ImportRecord {
                file_path: file_path.to_string(),
                import_string: cap[1].to_string(),
                resolved_path: None,
                imported_name: None,
                alias: None,
                is_namespace: false,
                is_default: false,
                is_reexport: false,
            });
        }

        // Multi-line import blocks
        let mut in_import_block = false;
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("import (") || trimmed == "import (" {
                in_import_block = true;
                continue;
            }
            if in_import_block {
                if trimmed == ")" {
                    in_import_block = false;
                    continue;
                }
                if let Some(cap) = GO_IMPORT_ENTRY_RE.captures(line) {
                    imports.push(ImportRecord {
                        file_path: file_path.to_string(),
                        import_string: cap[1].to_string(),
                        resolved_path: None,
                        imported_name: None,
                        alias: None,
                        is_namespace: false,
                        is_default: false,
                        is_reexport: false,
                    });
                }
            }
        }

        // --- structs ---
        for cap in GO_STRUCT_RE.captures_iter(content) {
            let name = &cap[1];
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            let end_line = Self::find_block_end(&lines, start_line);
            symbols.push(self.make_symbol(
                file_path,
                name,
                SymbolKind::Class,
                name,
                None,
                start_line as u32 + 1,
                end_line as u32 + 1,
                0,
                Some(&format!("type {} struct", name)),
            ));
        }

        // --- interfaces ---
        for cap in GO_INTERFACE_RE.captures_iter(content) {
            let name = &cap[1];
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            let end_line = Self::find_block_end(&lines, start_line);
            symbols.push(self.make_symbol(
                file_path,
                name,
                SymbolKind::Interface,
                name,
                None,
                start_line as u32 + 1,
                end_line as u32 + 1,
                0,
                Some(&format!("type {} interface", name)),
            ));
        }

        // --- type aliases (skip struct/interface already matched) ---
        let struct_names: HashSet<String> = symbols.iter().map(|s| s.name.clone()).collect();
        for cap in GO_TYPEDEF_RE.captures_iter(content) {
            let name = &cap[1];
            let following = &cap[2];
            if struct_names.contains(name) || following == "struct" || following == "interface" {
                continue;
            }
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            symbols.push(self.make_symbol(
                file_path,
                name,
                SymbolKind::TypeAlias,
                name,
                None,
                start_line as u32 + 1,
                start_line as u32 + 1,
                0,
                Some(&format!("type {}", name)),
            ));
        }

        // --- functions & methods ---
        for cap in GO_FUNC_RE.captures_iter(content) {
            let func_name = &cap[1];
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            let end_line = Self::find_block_end(&lines, start_line);

            // Determine if it's a method
            let line_text = lines.get(start_line).unwrap_or(&"");
            let (kind, qname, container) = if let Some(mc) = GO_METHOD_RE.captures(line_text) {
                let receiver = mc[1].to_string();
                let mname = &mc[2];
                (
                    SymbolKind::Method,
                    format!("{}.{}", receiver, mname),
                    Some(receiver),
                )
            } else {
                (SymbolKind::Function, func_name.to_string(), None)
            };

            // Extract parameter types and return type from the signature line
            let (param_types, return_type, param_count) =
                if let Some(sc) = GO_FUNC_SIG_RE.captures(line_text) {
                    let raw_params = sc[1].trim();
                    let raw_ret = sc[2].trim().trim_start_matches('(').trim_end_matches(')');
                    let pt = Self::extract_go_param_types(raw_params);
                    let pc = if raw_params.is_empty() {
                        0u32
                    } else {
                        pt.split(", ").count() as u32
                    };
                    let rt = if raw_ret.is_empty() {
                        None
                    } else {
                        Some(raw_ret.to_string())
                    };
                    (if pt.is_empty() { None } else { Some(pt) }, rt, Some(pc))
                } else {
                    (None, None, None)
                };

            let sig = format!("func {}", func_name);
            let mut sym = self.make_symbol(
                file_path,
                func_name,
                kind,
                &qname,
                container.as_deref(),
                start_line as u32 + 1,
                end_line as u32 + 1,
                0,
                Some(&sig),
            );
            sym.receiver_type = container.clone();
            sym.param_types = param_types;
            sym.return_type = return_type;
            sym.param_count = param_count;
            symbols.push(sym);
        }

        (symbols, imports)
    }

    fn extract_calls(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
    ) -> (Vec<SymbolRefRecord>, Vec<CallEdgeRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let keywords: HashSet<&str> = GO_KEYWORDS.iter().copied().collect();
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
                for cap in GO_CALL_RE.captures_iter(line) {
                    let Some(m) = cap.get(1) else { continue };
                    let callee = m.as_str();
                    if keywords.contains(callee) {
                        continue;
                    }
                    let start_col = m.start() as u32;
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
                        parser_tier: ParserTier::Heuristic,
                        parser_confidence: 0.6,
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
                        parser_tier: ParserTier::Heuristic,
                        parser_confidence: 0.6,
                        synthesized_by: None,
                        synthesis_key: None,
                        registered_file: None,
                        registered_line: None,
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
        symbols: &[SymbolRecord],
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let lines: Vec<&str> = content.lines().collect();
        let mut edges = Vec::new();

        // Find struct embeddings: for each struct symbol, scan its body for embedded types
        for sym in symbols.iter().filter(|s| s.kind == SymbolKind::Class) {
            let start = sym.start_line.saturating_sub(1) as usize;
            let end = (sym.end_line as usize).min(lines.len());
            for (offset, line) in lines[start..end].iter().enumerate() {
                if let Some(cap) = GO_EMBED_RE.captures(line) {
                    let embedded = &cap[1];
                    // Skip the struct's own name
                    if embedded == sym.name {
                        continue;
                    }
                    let line_no = (start + offset + 1) as u32;
                    edges.push(SemanticEdgeRecord {
                        edge_id: format!("se-{}:{}:inherits:{}", file_path, line_no, embedded),
                        file_path: file_path.to_string(),
                        source_symbol: sym.name.clone(),
                        source_symbol_uid: sym.symbol_uid.clone(),
                        target_symbol: embedded.to_string(),
                        target_symbol_uid: None,
                        relation_kind: SemanticRelation::Inherits,
                        line: line_no,
                        confidence: 0.95,
                        parser_tier: tier,
                    });
                }
            }
        }

        edges
    }

    #[allow(clippy::too_many_arguments)]
    fn make_symbol(
        &self,
        file_path: &str,
        name: &str,
        kind: SymbolKind,
        qname: &str,
        container: Option<&str>,
        start_line: u32,
        end_line: u32,
        start_col: u32,
        signature: Option<&str>,
    ) -> SymbolRecord {
        let symbol_id = StableId::edge_id("sym", file_path, start_line, start_col);
        let symbol_uid = StableId::symbol_uid(file_path, qname, kind.as_str(), signature);
        SymbolRecord {
            symbol_id,
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line,
            end_line,
            start_col,
            end_col: 0,
            signature: signature.map(String::from),
            doc: None,
            parser_tier: ParserTier::Heuristic,
            parser_confidence: 0.6,
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
}

impl Default for GoParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for GoParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let (symbols, imports) = self.extract_symbols_and_imports(content, file_path);
        let (symbol_refs, call_edges) = self.extract_calls(content, file_path, &symbols);
        let tier = ParserTier::Heuristic;
        let confidence = 0.6;
        let semantic_edges = self.extract_semantic_edges(content, file_path, &symbols, tier);
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (go, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );
        let is_test = file_path.ends_with("_test.go");

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
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
        let started = std::time::Instant::now();
        let (symbols, imports) = self.extract_symbols_and_imports(content, file_path);
        let (symbol_refs, call_edges) = self.extract_calls(content, file_path, &symbols);
        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "go parser timeout exceeded".to_string(),
            });
        }

        let tier = ParserTier::Heuristic;
        let confidence = 0.6;
        let semantic_edges = self.extract_semantic_edges(content, file_path, &symbols, tier);
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (go, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );
        let is_test = file_path.ends_with("_test.go");

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "go parser timeout exceeded".to_string(),
            });
        }

        Ok(ParseOutcome {
            summary,
            chunks,
            symbols,
            imports,
            symbol_refs,
            call_edges,
            semantic_edges,
            parser_tier: tier,
            parser_confidence: confidence,
            is_test_file: is_test,
            ..Default::default()
        })
    }

    fn supported_languages(&self) -> &[Language] {
        &[Language::Go]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Heuristic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_go() {
        let p = GoParser::new();
        let code = r#"package main

import (
    "fmt"
    "os"
)

type Greeter struct {
    Name string
}

type Reader interface {
    Read(p []byte) (n int, err error)
}

type ID = string

func (g *Greeter) Greet() string {
    return fmt.Sprintf("hello %s", g.Name)
}

func main() {
    g := Greeter{Name: "world"}
    fmt.Println(g.Greet())
}
"#;
        let outcome = p.parse("main.go", code, Language::Go).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Greeter"),
            "missing Greeter, got: {:?}",
            names
        );
        assert!(
            names.contains(&"Reader"),
            "missing Reader, got: {:?}",
            names
        );
        assert!(names.contains(&"Greet"), "missing Greet, got: {:?}", names);
        assert!(names.contains(&"main"), "missing main, got: {:?}", names);
        assert!(!outcome.imports.is_empty(), "imports should not be empty");
        assert!(!outcome.chunks.is_empty(), "chunks should not be empty");
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);
        assert!(!outcome.is_test_file);

        // Test file detection
        let test_outcome = p.parse("main_test.go", code, Language::Go).unwrap();
        assert!(test_outcome.is_test_file);
    }
}
