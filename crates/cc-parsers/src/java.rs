//! Java parser using regex-based outline extraction (Heuristic tier).

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

/// Matches class declarations (with optional modifiers and generics):
///   `public class Foo`, `abstract class Foo<T>`, `static class Inner`
static JAVA_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|abstract|static|final|strictfp)\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("java class re")
});

/// Matches interface declarations.
static JAVA_INTERFACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|abstract|static|strictfp)\s+)*interface\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("java interface re")
});

/// Matches enum declarations.
static JAVA_ENUM_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|static|strictfp)\s+)*enum\s+([A-Za-z_][A-Za-z0-9_]*)",
    )
    .expect("java enum re")
});

/// Matches method declarations:
///   `public void foo(`, `static int bar(`, `private List<String> baz(`
/// Captures the method name.
static JAVA_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|static|final|abstract|synchronized|native|strictfp|default)\s+)*(?:<[^>]+>\s+)?(?:[A-Za-z_][A-Za-z0-9_<>,\[\]\s]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
    )
    .expect("java method re")
});

/// Single-line import: `import foo.bar.Baz;`
static JAVA_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^import\s+(?:static\s+)?([A-Za-z_][A-Za-z0-9_.*]*)\s*;")
        .expect("java import re")
});

/// Matches `class Foo extends Bar` — captures class name and parent.
static JAVA_EXTENDS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|abstract|static|final|strictfp)\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^>]*>)?\s+extends\s+([A-Za-z_][A-Za-z0-9_.]*)",
    )
    .expect("java extends re")
});

/// Matches `class Foo implements Bar, Baz` — captures class name and the full implements list.
static JAVA_IMPLEMENTS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|abstract|static|final|strictfp)\s+)*class\s+([A-Za-z_][A-Za-z0-9_]*)\s*(?:<[^>]*>)?[^{]*\bimplements\s+([A-Za-z_][A-Za-z0-9_.,\s<>]*)",
    )
    .expect("java implements re")
});

/// Matches `@AnnotationName` on its own line.
static JAVA_ANNOTATION_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*@([A-Za-z_][A-Za-z0-9_.]*)").expect("java annotation re")
});

/// Matches `throws ExceptionA, ExceptionB` in a method signature.
/// Captures the exception list between `throws` and `{` or `;`.
static JAVA_THROWS_DECL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\)\s*throws\s+([\w\s,]+?)(?:\s*\{|\s*;)").expect("java throws decl re")
});

/// Matches `throw new ExceptionType(...)` statements.
/// Captures the exception class name.
static JAVA_THROW_NEW_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)\bthrow\s+new\s+([A-Z][A-Za-z0-9_]*)").expect("java throw new re")
});

/// Extended method regex: captures return type (group 1) and method name (group 2)
/// and the rest of the line after `(` (group 3 — parameter text until `)` or end of line).
static JAVA_METHOD_TYPED_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|protected|private|static|final|abstract|synchronized|native|strictfp|default)\s+)*(?:<[^>]+>\s+)?([A-Za-z_][A-Za-z0-9_<>,\[\]\s?]*?)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(([^)]*)\)",
    )
    .expect("java method typed re")
});

/// Call pattern: `name(`
static JAVA_CALL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("java call re"));

static JAVA_KEYWORDS: &[&str] = &[
    "abstract",
    "assert",
    "boolean",
    "break",
    "byte",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "final",
    "finally",
    "float",
    "for",
    "goto",
    "if",
    "implements",
    "import",
    "instanceof",
    "int",
    "interface",
    "long",
    "native",
    "new",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "short",
    "static",
    "strictfp",
    "super",
    "switch",
    "synchronized",
    "this",
    "throw",
    "throws",
    "transient",
    "try",
    "void",
    "volatile",
    "while",
    "true",
    "false",
    "null",
    "var",
    "yield",
    "record",
    "sealed",
    "permits",
    "String",
    "Object",
    "System",
    "Override",
];

/// Names that look like type names rather than method names when matched by JAVA_METHOD_RE.
static JAVA_NON_METHOD: &[&str] = &[
    "class",
    "interface",
    "enum",
    "if",
    "for",
    "while",
    "switch",
    "catch",
    "return",
    "new",
    "throw",
    "import",
    "package",
];

pub struct JavaParser {
    chunker: Chunker,
}

impl JavaParser {
    pub fn new() -> Self {
        Self {
            chunker: Chunker::default(),
        }
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
        lines.len().saturating_sub(1)
    }

    /// Extract Java parameter types from a raw params string like "String name, int age".
    /// Returns comma-separated types like "String, int".
    fn extract_java_param_types(raw: &str) -> String {
        if raw.trim().is_empty() {
            return String::new();
        }
        raw.split(',')
            .filter_map(|part| {
                let part = part.trim();
                if part.is_empty() {
                    return None;
                }
                // Java params are "Type name" or "Type... name" — first token(s) minus last = type
                let tokens: Vec<&str> = part.split_whitespace().collect();
                if tokens.len() >= 2 {
                    // Everything except the last token is the type
                    Some(tokens[..tokens.len() - 1].join(" "))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn extract_symbols_and_imports(
        &self,
        content: &str,
        file_path: &str,
    ) -> (Vec<SymbolRecord>, Vec<ImportRecord>) {
        let lines: Vec<&str> = content.lines().collect();
        let mut symbols = Vec::new();
        let mut imports = Vec::new();
        let non_methods: HashSet<&str> = JAVA_NON_METHOD.iter().copied().collect();

        // --- imports ---
        for cap in JAVA_IMPORT_RE.captures_iter(content) {
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

        // --- classes ---
        for cap in JAVA_CLASS_RE.captures_iter(content) {
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
                Some(&format!("class {}", name)),
            ));
        }

        // --- interfaces ---
        for cap in JAVA_INTERFACE_RE.captures_iter(content) {
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
                Some(&format!("interface {}", name)),
            ));
        }

        // --- enums ---
        for cap in JAVA_ENUM_RE.captures_iter(content) {
            let name = &cap[1];
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            let end_line = Self::find_block_end(&lines, start_line);
            symbols.push(self.make_symbol(
                file_path,
                name,
                SymbolKind::Enum,
                name,
                None,
                start_line as u32 + 1,
                end_line as u32 + 1,
                0,
                Some(&format!("enum {}", name)),
            ));
        }

        // --- methods ---
        // We need to figure out which class each method belongs to.
        // Simple heuristic: a method belongs to the innermost class whose line range contains it.
        let class_symbols: Vec<(String, u32, u32)> = symbols
            .iter()
            .filter(|s| {
                matches!(
                    s.kind,
                    SymbolKind::Class | SymbolKind::Interface | SymbolKind::Enum
                )
            })
            .map(|s| (s.name.clone(), s.start_line, s.end_line))
            .collect();

        // Build a map of method line → (return_type, param_types) from the typed regex
        let mut method_type_info: HashMap<usize, (String, String, u32)> = HashMap::new();
        for cap in JAVA_METHOD_TYPED_RE.captures_iter(content) {
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count();
            let ret_type = cap[1].trim().to_string();
            let raw_params = cap[3].trim().to_string();
            let param_types = Self::extract_java_param_types(&raw_params);
            let pc = if raw_params.is_empty() {
                0u32
            } else {
                param_types.split(", ").filter(|s| !s.is_empty()).count() as u32
            };
            method_type_info.insert(line, (ret_type, param_types, pc));
        }

        for cap in JAVA_METHOD_RE.captures_iter(content) {
            let method_name = &cap[1];
            if non_methods.contains(method_name) {
                continue;
            }
            let m = cap.get(0).unwrap();
            let start_line = content[..m.start()].matches('\n').count();
            let end_line = Self::find_block_end(&lines, start_line);
            let line_1based = start_line as u32 + 1;

            // Find containing class
            let container = class_symbols
                .iter()
                .filter(|(_, cs, ce)| line_1based >= *cs && line_1based <= *ce)
                .min_by_key(|(_, cs, ce)| ce - cs)
                .map(|(name, _, _)| name.clone());

            let qname = match &container {
                Some(c) => format!("{}.{}", c, method_name),
                None => method_name.to_string(),
            };

            let kind = if container.is_some() {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };

            let mut sym = self.make_symbol(
                file_path,
                method_name,
                kind,
                &qname,
                container.as_deref(),
                line_1based,
                end_line as u32 + 1,
                0,
                Some(&format!(
                    "{} {}",
                    if container.is_some() {
                        "method"
                    } else {
                        "func"
                    },
                    method_name
                )),
            );
            // Set receiver_type for methods
            sym.receiver_type = container.clone();
            // Set param/return types if available
            if let Some((ret, pt, pc)) = method_type_info.get(&start_line) {
                if ret != "void" {
                    sym.return_type = Some(ret.clone());
                }
                if !pt.is_empty() {
                    sym.param_types = Some(pt.clone());
                }
                sym.param_count = Some(*pc);
            }
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
        let keywords: HashSet<&str> = JAVA_KEYWORDS.iter().copied().collect();
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
                for cap in JAVA_CALL_RE.captures_iter(line) {
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
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();

        // extends
        for cap in JAVA_EXTENDS_RE.captures_iter(content) {
            let class_name = &cap[1];
            let parent_name = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:inherits:{}", file_path, line, parent_name),
                file_path: file_path.to_string(),
                source_symbol: class_name.to_string(),
                source_symbol_uid: None,
                target_symbol: parent_name.to_string(),
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Inherits,
                line,
                confidence: 0.95,
                parser_tier: tier,
            });
        }

        // implements
        for cap in JAVA_IMPLEMENTS_RE.captures_iter(content) {
            let class_name = &cap[1];
            let impl_list = &cap[2];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;
            // Split by comma, strip generics and whitespace
            for iface in impl_list.split(',') {
                let iface = iface.trim();
                // Strip generics: `Comparable<Foo>` -> `Comparable`
                let iface_name = iface.split('<').next().unwrap_or(iface).trim();
                if iface_name.is_empty() {
                    continue;
                }
                edges.push(SemanticEdgeRecord {
                    edge_id: format!("se-{}:{}:implements:{}", file_path, line, iface_name),
                    file_path: file_path.to_string(),
                    source_symbol: class_name.to_string(),
                    source_symbol_uid: None,
                    target_symbol: iface_name.to_string(),
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Implements,
                    line,
                    confidence: 0.95,
                    parser_tier: tier,
                });
            }
        }

        // annotations (decorates)
        let lines_vec: Vec<&str> = content.lines().collect();
        for cap in JAVA_ANNOTATION_RE.captures_iter(content) {
            let ann_name = &cap[1];
            // Skip common non-semantic annotations
            if ann_name == "Override" || ann_name == "SuppressWarnings" || ann_name == "Deprecated"
            {
                continue;
            }
            let m = cap.get(0).unwrap();
            let line_idx = content[..m.start()].matches('\n').count();
            let line = line_idx as u32 + 1;
            // Find the decorated symbol: look at the next non-annotation, non-blank line
            let mut target_name = String::new();
            for next_line in lines_vec.iter().skip(line_idx + 1) {
                let trimmed = next_line.trim();
                if trimmed.is_empty() || trimmed.starts_with('@') {
                    continue;
                }
                // Try to extract class/method name from the line
                if let Some(c) = JAVA_CLASS_RE.captures(next_line) {
                    target_name = c[1].to_string();
                } else if let Some(c) = JAVA_METHOD_RE.captures(next_line) {
                    target_name = c[1].to_string();
                }
                break;
            }
            if target_name.is_empty() {
                continue;
            }
            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:decorates:{}", file_path, line, ann_name),
                file_path: file_path.to_string(),
                source_symbol: ann_name.to_string(),
                source_symbol_uid: None,
                target_symbol: target_name,
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Decorates,
                line,
                confidence: 0.95,
                parser_tier: tier,
            });
        }

        edges
    }

    /// Extract throw/throws edges from Java source.
    ///
    /// Supports:
    /// - `throws IOException, SQLException` (method signature declaration)
    /// - `throw new FooException(...)` (body statement)
    fn extract_throw_edges(
        &self,
        content: &str,
        file_path: &str,
        symbols: &[SymbolRecord],
        tier: ParserTier,
    ) -> Vec<SemanticEdgeRecord> {
        let mut edges = Vec::new();

        // 1) Method signature: `throws ExceptionA, ExceptionB`
        for cap in JAVA_THROWS_DECL_RE.captures_iter(content) {
            let exception_list = &cap[1];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let (source_name, source_uid) = match find_enclosing_method(symbols, line) {
                Some(sym) => (
                    sym.qname.as_deref().unwrap_or(&sym.name).to_string(),
                    sym.symbol_uid.clone(),
                ),
                None => (file_path.to_string(), None),
            };

            for exc in exception_list.split(',') {
                let exc_name = exc.trim();
                if exc_name.is_empty() {
                    continue;
                }
                edges.push(SemanticEdgeRecord {
                    edge_id: format!("se-{}:{}:throws:{}", file_path, line, exc_name),
                    file_path: file_path.to_string(),
                    source_symbol: source_name.clone(),
                    source_symbol_uid: source_uid.clone(),
                    target_symbol: exc_name.to_string(),
                    target_symbol_uid: None,
                    relation_kind: SemanticRelation::Throws,
                    line,
                    confidence: 0.95,
                    parser_tier: tier,
                });
            }
        }

        // 2) Body statement: `throw new ExceptionType(...)`
        for cap in JAVA_THROW_NEW_RE.captures_iter(content) {
            let exception_type = &cap[1];
            let m = cap.get(0).unwrap();
            let line = content[..m.start()].matches('\n').count() as u32 + 1;

            let (source_name, source_uid) = match find_enclosing_method(symbols, line) {
                Some(sym) => (
                    sym.qname.as_deref().unwrap_or(&sym.name).to_string(),
                    sym.symbol_uid.clone(),
                ),
                None => (file_path.to_string(), None),
            };

            edges.push(SemanticEdgeRecord {
                edge_id: format!("se-{}:{}:throws:{}", file_path, line, exception_type),
                file_path: file_path.to_string(),
                source_symbol: source_name,
                source_symbol_uid: source_uid,
                target_symbol: exception_type.to_string(),
                target_symbol_uid: None,
                relation_kind: SemanticRelation::Throws,
                line,
                confidence: 0.9,
                parser_tier: tier,
            });
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

/// Find the innermost enclosing function/method for a given line number.
///
/// Prefers methods whose name starts with a lowercase letter (Java naming convention)
/// to avoid false positives from `throw new ExceptionType(...)` being mis-parsed
/// as method declarations by the method regex.
fn find_enclosing_method(symbols: &[SymbolRecord], line: u32) -> Option<&SymbolRecord> {
    let candidates: Vec<_> = symbols
        .iter()
        .filter(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
        .filter(|s| s.start_line <= line && s.end_line >= line)
        .collect();

    // Prefer candidates with lowercase-start names (real methods).
    let real_methods: Vec<_> = candidates
        .iter()
        .filter(|s| s.name.starts_with(|c: char| c.is_ascii_lowercase()))
        .collect();

    let pool = if real_methods.is_empty() {
        &candidates
    } else {
        // SAFETY: real_methods is Vec<&&SymbolRecord>; we need Vec<&SymbolRecord>
        // Just use the real_methods branch directly.
        return real_methods
            .into_iter()
            .min_by_key(|s| s.end_line - s.start_line)
            .copied();
    };

    pool.iter()
        .min_by_key(|s| s.end_line - s.start_line)
        .copied()
}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FileParser for JavaParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let (symbols, imports) = self.extract_symbols_and_imports(content, file_path);
        let (symbol_refs, call_edges) = self.extract_calls(content, file_path, &symbols);
        let tier = ParserTier::Heuristic;
        let confidence = 0.6;
        let mut semantic_edges = self.extract_semantic_edges(content, file_path, tier);
        semantic_edges.extend(self.extract_throw_edges(content, file_path, &symbols, tier));
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (java, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );

        let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
        let is_test = file_name.starts_with("Test") || file_name.ends_with("Test.java");

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
                message: "java parser timeout exceeded".to_string(),
            });
        }

        let tier = ParserTier::Heuristic;
        let confidence = 0.6;
        let mut semantic_edges = self.extract_semantic_edges(content, file_path, tier);
        semantic_edges.extend(self.extract_throw_edges(content, file_path, &symbols, tier));
        let chunks = self
            .chunker
            .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence);
        let summary = format!(
            "{} (java, {} lines, {} symbols)",
            file_path,
            content.lines().count(),
            symbols.len()
        );

        let file_name = file_path.rsplit('/').next().unwrap_or(file_path);
        let is_test = file_name.starts_with("Test") || file_name.ends_with("Test.java");

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "java parser timeout exceeded".to_string(),
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
        &[Language::Java]
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Heuristic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_java() {
        let p = JavaParser::new();
        let code = r#"package com.example;

import java.util.List;
import java.io.IOException;

public class Greeter {
    private String name;

    public Greeter(String name) {
        this.name = name;
    }

    public String greet() {
        return format("hello %s", name);
    }

    private String format(String fmt, String arg) {
        return String.format(fmt, arg);
    }
}

interface Speaker {
    void speak();
}

enum Color {
    RED, GREEN, BLUE
}
"#;
        let outcome = p.parse("Greeter.java", code, Language::Java).unwrap();
        let names: Vec<&str> = outcome.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names.contains(&"Greeter"),
            "missing Greeter, got: {:?}",
            names
        );
        assert!(
            names.contains(&"Speaker"),
            "missing Speaker, got: {:?}",
            names
        );
        assert!(names.contains(&"Color"), "missing Color, got: {:?}", names);
        assert!(names.contains(&"greet"), "missing greet, got: {:?}", names);
        assert!(
            names.contains(&"format"),
            "missing format, got: {:?}",
            names
        );
        assert!(!outcome.imports.is_empty(), "imports should not be empty");
        assert!(!outcome.chunks.is_empty(), "chunks should not be empty");
        assert!(
            !outcome.call_edges.is_empty(),
            "call edges should not be empty"
        );
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);
        assert!(!outcome.is_test_file);

        // Test file detection
        let test_outcome = p.parse("GreeterTest.java", code, Language::Java).unwrap();
        assert!(test_outcome.is_test_file);

        let test_outcome2 = p.parse("TestGreeter.java", code, Language::Java).unwrap();
        assert!(test_outcome2.is_test_file);
    }

    #[test]
    fn extract_throw_edges_java() {
        let p = JavaParser::new();
        let code = r#"package com.example;

import java.io.IOException;
import java.text.ParseException;

public class Service {
    public void read() throws IOException, ParseException {
        // reading
    }

    public void process() {
        throw new IllegalArgumentException("bad arg");
    }

    public void complex() throws RuntimeException {
        if (true) {
            throw new IllegalStateException("state");
        }
        throw new NullPointerException("null");
    }
}
"#;
        let outcome = p.parse("Service.java", code, Language::Java).unwrap();
        let throw_edges: Vec<_> = outcome
            .semantic_edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Throws)
            .collect();

        // read() throws IOException, ParseException -> 2 edges from declaration
        let read_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.read")
            .collect();
        assert_eq!(
            read_edges.len(),
            2,
            "expected 2 throws edges for read(), got {:?}",
            read_edges
        );
        let read_targets: Vec<&str> = read_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(read_targets.contains(&"IOException"));
        assert!(read_targets.contains(&"ParseException"));
        // throws declaration should have higher confidence
        assert!((read_edges[0].confidence - 0.95).abs() < 0.01);

        // process() -> throw new IllegalArgumentException -> 1 edge
        let process_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.process")
            .collect();
        assert_eq!(process_edges.len(), 1);
        assert_eq!(process_edges[0].target_symbol, "IllegalArgumentException");
        assert!((process_edges[0].confidence - 0.9).abs() < 0.01);

        // complex() -> throws RuntimeException + throw new IllegalStateException + throw new NullPointerException
        let complex_edges: Vec<_> = throw_edges
            .iter()
            .filter(|e| e.source_symbol == "Service.complex")
            .collect();
        assert_eq!(
            complex_edges.len(),
            3,
            "expected 3 throws edges for complex(), got {:?}",
            complex_edges
        );
        let complex_targets: Vec<&str> = complex_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(complex_targets.contains(&"RuntimeException"));
        assert!(complex_targets.contains(&"IllegalStateException"));
        assert!(complex_targets.contains(&"NullPointerException"));
    }
}
