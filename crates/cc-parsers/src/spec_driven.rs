//! Spec-driven parser — uses LangSpec metadata + regex heuristics for symbol extraction.
//!
//! Sits between GenericParser (line-based chunking only) and full tree-sitter parsers.
//! Provides Heuristic-tier parsing for languages that have a LangSpec but no tree-sitter
//! grammar wired up (C#, PHP, Ruby, Swift, Kotlin).

use crate::chunker::Chunker;
use crate::lang_spec::LangSpec;
use crate::traits::FileParser;
use cc_model::edge::{DataFlowEdgeRecord, ImportRecord};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::{CcResult, Language, ParseOutcome, ParserTier};
use regex::Regex;
use std::sync::LazyLock;

/// A parser driven by a static `LangSpec` table, using regex-based heuristics
/// for symbol and import extraction. Produces `ParserTier::Heuristic` output.
pub struct SpecDrivenParser {
    spec: &'static LangSpec,
    chunker: Chunker,
}

// ── Per-language regex patterns ─────────────────────────────────────────

/// C#: function/method declarations with optional access modifiers and return type.
static CS_METHOD_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|protected|internal|static|virtual|override|abstract|async|sealed|partial|new)\s+)*(?:\w+(?:<[^>]+>)?(?:\[\])?)\s+([A-Za-z_]\w*)\s*(?:<[^>]*>)?\s*\(",
    )
    .expect("cs method regex")
});

/// C#: class/struct/interface/enum declarations.
static CS_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|protected|internal|static|abstract|sealed|partial|new)\s+)*(?:class|struct|interface|enum|record)\s+([A-Za-z_]\w*)",
    )
    .expect("cs class regex")
});

/// C#: namespace declarations.
static CS_NAMESPACE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\S\n]*namespace\s+([\w.]+)").expect("cs namespace regex"));

/// C#: using directives.
static CS_USING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*using\s+(?:static\s+)?([\w.]+)\s*;").expect("cs using regex")
});

/// C#: Environment.GetEnvironmentVariable("KEY") calls.
static CSHARP_ENV_ACCESS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"Environment\.GetEnvironmentVariable\(\s*"(\w+)"\s*\)"#)
        .expect("csharp env access regex")
});

/// PHP: function definitions.
static PHP_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|protected|static|abstract|final)\s+)*function\s+([A-Za-z_]\w*)\s*\(",
    )
    .expect("php func regex")
});

/// PHP: class/interface/trait/enum declarations.
static PHP_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:abstract|final)\s+)?(?:class|interface|trait|enum)\s+([A-Za-z_]\w*)",
    )
    .expect("php class regex")
});

/// PHP: namespace declarations.
static PHP_NAMESPACE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*namespace\s+([\w\\]+)\s*;").expect("php namespace regex")
});

/// PHP: use/require/include statements.
static PHP_USE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?m)^[^\S\n]*(?:use\s+([\w\\]+)|(?:require|include)(?:_once)?\s+['"]([^'"]+)['"])"#,
    )
    .expect("php use regex")
});

/// Ruby: method definitions.
static RB_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*def\s+(?:self\.)?([A-Za-z_]\w*[?!=]?)").expect("ruby def regex")
});

/// Ruby: class/module declarations.
static RB_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*(?:class|module)\s+([A-Z]\w*(?:::[A-Z]\w*)*)")
        .expect("ruby class regex")
});

/// Ruby: require/require_relative/load.
static RB_REQUIRE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[^\S\n]*(?:require|require_relative|load)\s+['"]([^'"]+)['"]"#)
        .expect("ruby require regex")
});

/// Swift: func declarations.
static SWIFT_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|internal|fileprivate|open|static|class|override|mutating|@\w+\s+)*\s*)func\s+([A-Za-z_]\w*)",
    )
    .expect("swift func regex")
});

/// Swift: class/struct/protocol/enum declarations.
static SWIFT_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|internal|fileprivate|open|final)\s+)?(?:class|struct|protocol|enum)\s+([A-Za-z_]\w*)",
    )
    .expect("swift class regex")
});

/// Swift: import declarations.
static SWIFT_IMPORT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\S\n]*import\s+(\w+)").expect("swift import regex"));

/// Kotlin: fun declarations.
static KT_FUN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|protected|internal|open|abstract|override|inline|suspend|operator|infix|tailrec)\s+)*fun\s+(?:<[^>]+>\s+)?([A-Za-z_]\w*)",
    )
    .expect("kt fun regex")
});

/// Kotlin: class/object/interface declarations.
static KT_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:public|private|protected|internal|open|abstract|sealed|data|enum|inner|annotation)\s+)*(?:class|object|interface)\s+([A-Za-z_]\w*)",
    )
    .expect("kt class regex")
});

/// Kotlin: package declaration.
static KT_PACKAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\S\n]*package\s+([\w.]+)").expect("kt package regex"));

/// Kotlin: import declarations.
static KT_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*import\s+([\w.]+(?:\.\*)?)").expect("kt import regex")
});

/// Dart: function/method declarations.
static DART_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:static|abstract|external|@\w+\s+)*\s*)(?:\w+(?:<[^>]+>)?(?:\?)?)\s+([A-Za-z_]\w*)\s*(?:<[^>]*>)?\s*\(",
    )
    .expect("dart func regex")
});

/// Dart: class/enum/mixin/extension declarations.
static DART_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:abstract\s+)?(?:class|enum|mixin|extension)\s+([A-Za-z_]\w*)",
    )
    .expect("dart class regex")
});

/// Dart: library declarations.
static DART_LIBRARY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\S\n]*library\s+([\w.]+)\s*;").expect("dart library regex"));

/// Dart: import/export declarations.
static DART_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[^\S\n]*(?:import|export)\s+['"]([^'"]+)['"]"#).expect("dart import regex")
});

/// Scala: def declarations.
static SCALA_DEF_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:private|protected|override|abstract|final|implicit|lazy)\s+)*def\s+([A-Za-z_]\w*)",
    )
    .expect("scala def regex")
});

/// Scala: class/object/trait/enum declarations.
static SCALA_CLASS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:(?:abstract|sealed|final|private|protected|case|implicit)\s+)*(?:class|object|trait|enum)\s+([A-Za-z_]\w*)",
    )
    .expect("scala class regex")
});

/// Scala: package declaration.
static SCALA_PACKAGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^[^\S\n]*package\s+([\w.]+)").expect("scala package regex"));

/// Scala: import declarations.
static SCALA_IMPORT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[^\S\n]*import\s+([\w.]+(?:\.\{[^}]+\}|\.\_)?)").expect("scala import regex")
});

/// Lua: function declarations (both `function name()` and `local function name()`).
static LUA_FUNC_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?m)^[^\S\n]*(?:local\s+)?function\s+([A-Za-z_][\w.:]*)\s*\(",
    )
    .expect("lua func regex")
});

/// Lua: require() calls.
static LUA_REQUIRE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)(?:require)\s*\(?['"]([^'"]+)['"]\)?"#).expect("lua require regex")
});

impl SpecDrivenParser {
    pub fn new(spec: &'static LangSpec) -> Self {
        Self {
            spec,
            chunker: Chunker::default(),
        }
    }

    /// Extract symbol definitions using language-specific regex patterns.
    fn extract_symbols(&self, content: &str, file_path: &str) -> Vec<SymbolRecord> {
        let tier = ParserTier::Heuristic;
        let confidence = tier.default_confidence();
        let lines: Vec<&str> = content.lines().collect();

        match self.spec.language {
            Language::CSharp => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &CS_METHOD_RE,
                &CS_CLASS_RE,
                Some(&CS_NAMESPACE_RE),
            ),
            Language::Php => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &PHP_FUNC_RE,
                &PHP_CLASS_RE,
                Some(&PHP_NAMESPACE_RE),
            ),
            Language::Ruby => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &RB_DEF_RE,
                &RB_CLASS_RE,
                None,
            ),
            Language::Swift => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &SWIFT_FUNC_RE,
                &SWIFT_CLASS_RE,
                None,
            ),
            Language::Kotlin => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &KT_FUN_RE,
                &KT_CLASS_RE,
                Some(&KT_PACKAGE_RE),
            ),
            Language::Dart => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &DART_FUNC_RE,
                &DART_CLASS_RE,
                Some(&DART_LIBRARY_RE),
            ),
            Language::Scala => self.extract_with_regexes(
                file_path,
                &lines,
                tier,
                confidence,
                &SCALA_DEF_RE,
                &SCALA_CLASS_RE,
                Some(&SCALA_PACKAGE_RE),
            ),
            Language::Lua => {
                // Lua has no class declarations; extract only functions.
                let content_str = lines.join("\n");
                let mut symbols = Vec::new();
                for cap in LUA_FUNC_RE.captures_iter(&content_str) {
                    let name = cap[1].to_string();
                    let match_start = cap.get(0).unwrap().start();
                    let line_num = content_str[..match_start].matches('\n').count() as u32 + 1;
                    let end_line = Self::estimate_end_line(&lines, line_num);

                    let line_text = lines.get((line_num - 1) as usize).unwrap_or(&"");
                    let indent = line_text.len() - line_text.trim_start().len();
                    let kind = if name.contains(':') || name.contains('.') || indent > 0 {
                        SymbolKind::Method
                    } else {
                        SymbolKind::Function
                    };

                    let qname = name.clone();
                    let symbol_id = format!("spec:{}:{}:{}", file_path, kind.as_str(), name);
                    let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), None);

                    symbols.push(SymbolRecord {
                        symbol_id,
                        file_path: file_path.to_string(),
                        name,
                        kind,
                        container: None,
                        start_line: line_num,
                        end_line,
                        start_col: 0,
                        end_col: 0,
                        signature: None,
                        doc: None,
                        parser_tier: tier,
                        parser_confidence: confidence,
                        qname: Some(qname),
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
                    });
                }
                symbols
            }
            _ => Vec::new(),
        }
    }

    /// Generic helper: run function regex and class regex over content, produce SymbolRecords.
    fn extract_with_regexes(
        &self,
        file_path: &str,
        lines: &[&str],
        tier: ParserTier,
        confidence: f64,
        func_re: &Regex,
        class_re: &Regex,
        namespace_re: Option<&Regex>,
    ) -> Vec<SymbolRecord> {
        let content = lines.join("\n");
        let mut symbols = Vec::new();

        // Detect namespace/module context (first match only).
        let namespace =
            namespace_re.and_then(|re| re.captures(&content).map(|cap| cap[1].to_string()));

        // Extract class-like symbols.
        for cap in class_re.captures_iter(&content) {
            let name = cap[1].to_string();
            let match_start = cap.get(0).unwrap().start();
            let line_num = content[..match_start].matches('\n').count() as u32 + 1;
            let end_line = Self::estimate_end_line(lines, line_num);

            let kind = Self::classify_class_kind(&cap[0]);
            let qname = self.build_qname(namespace.as_deref(), &name);

            let symbol_id = format!("spec:{}:{}:{}", file_path, kind.as_str(), name);
            let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), None);

            symbols.push(SymbolRecord {
                symbol_id,
                file_path: file_path.to_string(),
                name,
                kind,
                container: namespace.clone(),
                start_line: line_num,
                end_line,
                start_col: 0,
                end_col: 0,
                signature: None,
                doc: None,
                parser_tier: tier,
                parser_confidence: confidence,
                qname: Some(qname),
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
            });
        }

        // Extract function/method symbols.
        for cap in func_re.captures_iter(&content) {
            let name = cap[1].to_string();
            let match_start = cap.get(0).unwrap().start();
            let line_num = content[..match_start].matches('\n').count() as u32 + 1;
            let end_line = Self::estimate_end_line(lines, line_num);

            // Determine if this function is inside a class (heuristic: indented).
            let line_text = lines.get((line_num - 1) as usize).unwrap_or(&"");
            let indent = line_text.len() - line_text.trim_start().len();
            let kind = if indent > 0 {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };

            let qname = self.build_qname(namespace.as_deref(), &name);
            let symbol_id = format!("spec:{}:{}:{}", file_path, kind.as_str(), name);
            let symbol_uid = StableId::symbol_uid(file_path, &qname, kind.as_str(), None);

            symbols.push(SymbolRecord {
                symbol_id,
                file_path: file_path.to_string(),
                name,
                kind,
                container: namespace.clone(),
                start_line: line_num,
                end_line,
                start_col: 0,
                end_col: 0,
                signature: None,
                doc: None,
                parser_tier: tier,
                parser_confidence: confidence,
                qname: Some(qname),
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
            });
        }

        symbols
    }

    /// Extract import records using language-specific regex patterns.
    fn extract_imports(&self, content: &str, file_path: &str) -> Vec<ImportRecord> {
        match self.spec.language {
            Language::CSharp => Self::extract_imports_re(content, file_path, &CS_USING_RE),
            Language::Php => Self::extract_imports_re(content, file_path, &PHP_USE_RE),
            Language::Ruby => Self::extract_imports_re(content, file_path, &RB_REQUIRE_RE),
            Language::Swift => Self::extract_imports_re(content, file_path, &SWIFT_IMPORT_RE),
            Language::Kotlin => Self::extract_imports_re(content, file_path, &KT_IMPORT_RE),
            Language::Dart => Self::extract_imports_re(content, file_path, &DART_IMPORT_RE),
            Language::Scala => Self::extract_imports_re(content, file_path, &SCALA_IMPORT_RE),
            Language::Lua => Self::extract_imports_re(content, file_path, &LUA_REQUIRE_RE),
            _ => Vec::new(),
        }
    }

    fn extract_imports_re(content: &str, file_path: &str, re: &Regex) -> Vec<ImportRecord> {
        re.captures_iter(content)
            .filter_map(|cap| {
                // Take the first non-None capture group after group 0.
                let import_string =
                    (1..cap.len()).find_map(|i| cap.get(i).map(|m| m.as_str().to_string()))?;
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
            })
            .collect()
    }

    /// Build a qualified name using the spec's qname_separator.
    fn build_qname(&self, namespace: Option<&str>, name: &str) -> String {
        match namespace {
            Some(ns) => format!("{}{}{}", ns, self.spec.qname_separator, name),
            None => name.to_string(),
        }
    }

    /// Heuristic: estimate the end line of a definition by looking for closing brace
    /// or `end` keyword, or falling back to a fixed lookahead.
    fn estimate_end_line(lines: &[&str], start_line: u32) -> u32 {
        let start_idx = (start_line - 1) as usize;
        if start_idx >= lines.len() {
            return start_line;
        }

        let start_indent = lines[start_idx].len() - lines[start_idx].trim_start().len();
        let max_scan = (start_idx + 200).min(lines.len());

        for idx in (start_idx + 1)..max_scan {
            let line = lines[idx];
            let trimmed = line.trim();

            // Closing brace at same or lesser indent.
            if trimmed == "}" || trimmed == "end" || trimmed.starts_with("end ") {
                let indent = line.len() - line.trim_start().len();
                if indent <= start_indent {
                    return (idx + 1) as u32;
                }
            }
        }

        // Fallback: 30 lines or end of file.
        ((start_idx + 30).min(lines.len())) as u32
    }

    /// Extract environment variable accesses for languages that support it.
    ///
    /// Currently supports C# (`Environment.GetEnvironmentVariable("KEY")`).
    fn extract_env_accesses(
        &self,
        content: &str,
        symbols: &[SymbolRecord],
        file_path: &str,
    ) -> Vec<DataFlowEdgeRecord> {
        let re = match self.spec.language {
            Language::CSharp => &*CSHARP_ENV_ACCESS_RE,
            _ => return Vec::new(),
        };

        let mut edges = Vec::new();

        for cap in re.captures_iter(content) {
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

    /// Classify a class-like declaration into the appropriate SymbolKind.
    fn classify_class_kind(decl_text: &str) -> SymbolKind {
        if decl_text.contains("interface") {
            SymbolKind::Interface
        } else if decl_text.contains("enum") {
            SymbolKind::Enum
        } else if decl_text.contains("module") {
            SymbolKind::Module
        } else if decl_text.contains("struct") || decl_text.contains("record") {
            SymbolKind::Class
        } else if decl_text.contains("protocol") {
            SymbolKind::Interface
        } else if decl_text.contains("trait") {
            SymbolKind::Interface
        } else if decl_text.contains("object") {
            SymbolKind::Class
        } else {
            SymbolKind::Class
        }
    }
}

impl FileParser for SpecDrivenParser {
    fn parse(&self, file_path: &str, content: &str, language: Language) -> CcResult<ParseOutcome> {
        let tier = ParserTier::Heuristic;
        let confidence = tier.default_confidence();

        let symbols = self.extract_symbols(content, file_path);
        let imports = self.extract_imports(content, file_path);
        let data_flow_edges = self.extract_env_accesses(content, &symbols, file_path);

        let chunks = if symbols.is_empty() {
            self.chunker
                .chunk_by_lines(file_path, content, language, tier, confidence)
        } else {
            self.chunker
                .chunk_with_symbols(file_path, content, language, &symbols, tier, confidence)
        };

        let line_count = content.lines().count();
        let summary = format!(
            "{} ({} lines, {} symbols via {})",
            file_path,
            line_count,
            symbols.len(),
            self.spec.grammar_name,
        );

        Ok(ParseOutcome {
            summary,
            symbols,
            imports,
            chunks,
            data_flow_edges,
            parser_tier: tier,
            parser_confidence: confidence,
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

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "spec-driven parser timeout exceeded".to_string(),
            });
        }

        let result = self.parse(file_path, content, language);

        if timeout_micros.is_some_and(|limit| started.elapsed().as_micros() as u64 > limit) {
            return Err(cc_model::CcError::Parse {
                file: file_path.to_string(),
                message: "spec-driven parser timeout exceeded".to_string(),
            });
        }

        result
    }

    fn supported_languages(&self) -> &[Language] {
        std::slice::from_ref(&self.spec.language)
    }

    fn tier(&self) -> ParserTier {
        ParserTier::Heuristic
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang_spec;

    #[test]
    fn csharp_extracts_class_and_method() {
        let spec = lang_spec::spec_for_language(Language::CSharp).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
using System;

namespace MyApp.Services
{
    public class UserService
    {
        public async Task<User> GetUser(int id)
        {
            return await _repo.Find(id);
        }

        private void Validate(User user)
        {
            // validation
        }
    }
}
"#;
        let outcome = parser
            .parse("UserService.cs", content, Language::CSharp)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);
        assert!(outcome.parser_confidence > 0.3);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!class_syms.is_empty(), "should find UserService class");
        assert_eq!(class_syms[0].name, "UserService");

        let func_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Method || s.kind == SymbolKind::Function)
            .collect();
        assert!(
            func_syms.len() >= 2,
            "should find GetUser and Validate methods"
        );

        assert!(!outcome.imports.is_empty(), "should find using System");

        // Check qname uses '.' separator from spec
        let class_qname = class_syms[0].qname.as_deref().unwrap();
        assert!(class_qname.contains('.'), "qname should use '.' separator");
    }

    #[test]
    fn ruby_extracts_class_and_def() {
        let spec = lang_spec::spec_for_language(Language::Ruby).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
require 'json'
require_relative 'base_service'

class UserService
  def initialize(repo)
    @repo = repo
  end

  def find_user(id)
    @repo.find(id)
  end

  def self.create(params)
    new(params)
  end
end
"#;
        let outcome = parser
            .parse("user_service.rb", content, Language::Ruby)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!class_syms.is_empty(), "should find UserService class");

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(
            func_count >= 2,
            "should find at least initialize and find_user"
        );

        assert!(
            outcome.imports.len() >= 2,
            "should find require and require_relative"
        );
    }

    #[test]
    fn kotlin_extracts_class_and_fun() {
        let spec = lang_spec::spec_for_language(Language::Kotlin).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
package com.example.service

import com.example.model.User
import com.example.repo.UserRepo

class UserService(private val repo: UserRepo) {
    suspend fun getUser(id: Int): User {
        return repo.find(id)
    }

    fun deleteUser(id: Int) {
        repo.delete(id)
    }
}
"#;
        let outcome = parser
            .parse("UserService.kt", content, Language::Kotlin)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!class_syms.is_empty(), "should find UserService class");

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(func_count >= 2, "should find getUser and deleteUser");

        assert!(outcome.imports.len() >= 2, "should find imports");

        // qname should use '.' separator
        let class_qname = class_syms[0].qname.as_deref().unwrap();
        assert!(
            class_qname.contains("com.example.service."),
            "qname should include package"
        );
    }

    #[test]
    fn php_extracts_class_and_function() {
        let spec = lang_spec::spec_for_language(Language::Php).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"<?php
namespace App\Services;

use App\Models\User;

class UserService
{
    public function getUser(int $id): User
    {
        return User::find($id);
    }

    private function validate(User $user): void
    {
        // ...
    }
}
"#;
        let outcome = parser
            .parse("UserService.php", content, Language::Php)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!class_syms.is_empty(), "should find UserService class");

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(func_count >= 2, "should find getUser and validate");

        assert!(!outcome.imports.is_empty(), "should find use statement");
    }

    #[test]
    fn swift_extracts_class_and_func() {
        let spec = lang_spec::spec_for_language(Language::Swift).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
import Foundation

class UserService {
    func getUser(id: Int) -> User {
        return repo.find(id)
    }

    private func validate(_ user: User) {
        // ...
    }
}
"#;
        let outcome = parser
            .parse("UserService.swift", content, Language::Swift)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .collect();
        assert!(!class_syms.is_empty(), "should find UserService class");

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(func_count >= 2, "should find getUser and validate");

        assert!(!outcome.imports.is_empty(), "should find import Foundation");
    }

    #[test]
    fn spec_driven_produces_chunks() {
        let spec = lang_spec::spec_for_language(Language::Ruby).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = "def foo\n  42\nend\n\ndef bar\n  43\nend\n";
        let outcome = parser.parse("test.rb", content, Language::Ruby).unwrap();
        assert!(!outcome.chunks.is_empty(), "should produce chunks");
        assert!(outcome.summary.contains("test.rb"));
        assert!(outcome.summary.contains("via ruby"));
    }

    #[test]
    fn spec_driven_empty_content() {
        let spec = lang_spec::spec_for_language(Language::CSharp).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let outcome = parser.parse("empty.cs", "", Language::CSharp).unwrap();
        assert!(outcome.symbols.is_empty());
        assert!(outcome.chunks.is_empty());
    }

    #[test]
    fn dart_extracts_class_and_function() {
        let spec = lang_spec::spec_for_language(Language::Dart).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
import 'package:flutter/material.dart';
import 'dart:async';

class UserService {
    Future<User> getUser(int id) {
        return _repo.find(id);
    }

    void deleteUser(int id) {
        _repo.delete(id);
    }
}

enum Status {
    active,
    inactive,
}
"#;
        let outcome = parser
            .parse("user_service.dart", content, Language::Dart)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class || s.kind == SymbolKind::Enum)
            .collect();
        assert!(
            class_syms.len() >= 2,
            "should find UserService class and Status enum, got {:?}",
            class_syms.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(
            func_count >= 2,
            "should find getUser and deleteUser"
        );

        assert!(
            outcome.imports.len() >= 2,
            "should find import statements"
        );
    }

    #[test]
    fn scala_extracts_object_and_def() {
        let spec = lang_spec::spec_for_language(Language::Scala).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
package com.example.service

import com.example.model.User
import com.example.repo.UserRepo

object UserService {
    def getUser(id: Int): User = {
        UserRepo.find(id)
    }

    def deleteUser(id: Int): Unit = {
        UserRepo.delete(id)
    }
}

trait Serializable {
    def serialize(): String
}
"#;
        let outcome = parser
            .parse("UserService.scala", content, Language::Scala)
            .unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let class_syms: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| {
                s.kind == SymbolKind::Class
                    || s.kind == SymbolKind::Interface
                    || s.kind == SymbolKind::Module
            })
            .collect();
        assert!(
            class_syms.len() >= 2,
            "should find UserService object and Serializable trait, got {:?}",
            class_syms.iter().map(|s| &s.name).collect::<Vec<_>>()
        );

        let func_count = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .count();
        assert!(
            func_count >= 2,
            "should find getUser, deleteUser, and serialize"
        );

        assert!(
            outcome.imports.len() >= 2,
            "should find import statements"
        );

        // qname should include package
        let obj = class_syms.iter().find(|s| s.name == "UserService").unwrap();
        let qname = obj.qname.as_deref().unwrap();
        assert!(
            qname.contains("com.example.service."),
            "qname should include package, got: {}",
            qname
        );
    }

    #[test]
    fn lua_extracts_functions() {
        let spec = lang_spec::spec_for_language(Language::Lua).unwrap();
        let parser = SpecDrivenParser::new(spec);
        let content = r#"
local json = require('cjson')
local utils = require 'utils'

function greet(name)
    print("Hello, " .. name)
end

local function helper(x)
    return x * 2
end

function MyModule.doStuff(a, b)
    return a + b
end
"#;
        let outcome = parser.parse("main.lua", content, Language::Lua).unwrap();
        assert_eq!(outcome.parser_tier, ParserTier::Heuristic);

        let func_names: Vec<_> = outcome
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function || s.kind == SymbolKind::Method)
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            func_names.len() >= 3,
            "should find greet, helper, and MyModule.doStuff, got {:?}",
            func_names
        );

        assert!(
            outcome.imports.len() >= 2,
            "should find require calls, got {}",
            outcome.imports.len()
        );
    }
}
