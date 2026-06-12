//! Generate hierarchy edges (DEFINES, DEFINES_METHOD, CONTAINS_FILE)
//! from existing symbol relationships.

use std::collections::{HashMap, HashSet};

use cc_model::edge::{SemanticEdgeRecord, SemanticRelation};
use cc_model::id::StableId;
use cc_model::symbol::{SymbolKind, SymbolRecord};
use cc_model::ParserTier;

/// Generate hierarchy semantic edges from parsed symbols.
///
/// Rules:
/// 1. File -> top-level Function/Class/Interface/Enum/Constant -> Defines edge
/// 2. Class/Interface -> Method -> DefinesMethod edge
/// 3. Folder -> File -> ContainsFile edge (based on file_path hierarchy)
///
/// Every rule is file-local: Defines/DefinesMethod endpoints live in the
/// symbol's own file (the container index is keyed by `(file_path, name)`),
/// and ContainsFile links a file's immediate parent directory to the file
/// itself. Incremental builds therefore pass only the batch files' symbols
/// and paths — the resulting edges are exactly those files' edges, and the
/// per-file `semantic_edges` deletes in the write batch keep the stored set
/// consistent (a directory "node" exists only through its files' edges, so
/// it appears/disappears with the first/last file in it).
pub fn generate_hierarchy_edges(
    symbols: &[SymbolRecord],
    file_paths: &[String],
) -> Vec<SemanticEdgeRecord> {
    let mut edges = Vec::new();

    // Pre-build container index: (file_path, name) -> &SymbolRecord  [O(n) once]
    let container_index: HashMap<(&str, &str), &SymbolRecord> = symbols
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Component
                    | SymbolKind::Controller
            )
        })
        .map(|s| ((s.file_path.as_str(), s.name.as_str()), s))
        .collect();

    // Rule 1 & 2: Symbol containment
    for sym in symbols {
        // Rule 2: If symbol has a container (e.g. "MyClass"), find the container symbol
        // and generate a DefinesMethod edge.
        if let Some(ref container) = sym.container {
            if matches!(sym.kind, SymbolKind::Method | SymbolKind::Function) {
                if let Some(parent) =
                    container_index.get(&(sym.file_path.as_str(), container.as_str()))
                {
                    edges.push(SemanticEdgeRecord {
                        edge_id: StableId::edge_id("hier", &sym.file_path, sym.start_line, 0),
                        file_path: sym.file_path.clone(),
                        source_symbol: parent.name.clone(),
                        source_symbol_uid: parent.symbol_uid.clone(),
                        target_symbol: sym.name.clone(),
                        target_symbol_uid: sym.symbol_uid.clone(),
                        relation_kind: SemanticRelation::DefinesMethod,
                        line: sym.start_line,
                        confidence: 0.95,
                        parser_tier: sym.parser_tier,
                    });
                    // Skip Rule 1 for this symbol since it has a container
                    continue;
                }
            }
        }

        // Rule 1: File -> top-level symbol (no container and no parent_symbol_id = top-level)
        if sym.container.is_none() && sym.parent_symbol_id.is_none() {
            // Only generate Defines edges for meaningful top-level symbols
            if matches!(
                sym.kind,
                SymbolKind::Function
                    | SymbolKind::Class
                    | SymbolKind::Interface
                    | SymbolKind::Enum
                    | SymbolKind::Constant
                    | SymbolKind::TypeAlias
                    | SymbolKind::Component
                    | SymbolKind::Controller
                    | SymbolKind::Middleware
                    | SymbolKind::Hook
                    | SymbolKind::RouteHandler
            ) {
                let file_pseudo_name = format!("file::{}", sym.file_path);
                let file_pseudo_uid = format!("file::{}", sym.file_path);
                edges.push(SemanticEdgeRecord {
                    edge_id: StableId::edge_id("hier", &sym.file_path, sym.start_line, 1),
                    file_path: sym.file_path.clone(),
                    source_symbol: file_pseudo_name,
                    source_symbol_uid: Some(file_pseudo_uid),
                    target_symbol: sym.name.clone(),
                    target_symbol_uid: sym.symbol_uid.clone(),
                    relation_kind: SemanticRelation::Defines,
                    line: sym.start_line,
                    confidence: 0.90,
                    parser_tier: sym.parser_tier,
                });
            }
        }
    }

    // Rule 3: Folder -> File (ContainsFile)
    let mut seen_pairs: HashSet<String> = HashSet::new();
    for path in file_paths {
        if let Some(parent_dir) = std::path::Path::new(path).parent() {
            let dir_str = parent_dir.to_string_lossy().to_string();
            let pair_key = format!("{}→{}", dir_str, path);
            if seen_pairs.insert(pair_key) {
                let dir_name = format!("dir::{}", dir_str);
                let dir_uid = format!("dir::{}", dir_str);
                let file_name = format!("file::{}", path);
                let file_uid = format!("file::{}", path);
                edges.push(SemanticEdgeRecord {
                    edge_id: StableId::edge_id("contains", path, 0, 0),
                    file_path: path.clone(),
                    source_symbol: dir_name,
                    source_symbol_uid: Some(dir_uid),
                    target_symbol: file_name,
                    target_symbol_uid: Some(file_uid),
                    relation_kind: SemanticRelation::ContainsFile,
                    line: 0,
                    confidence: 1.0,
                    parser_tier: ParserTier::Verified,
                });
            }
        }
    }

    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(
        name: &str,
        kind: SymbolKind,
        file_path: &str,
        container: Option<&str>,
        parent_symbol_id: Option<&str>,
        start_line: u32,
    ) -> SymbolRecord {
        SymbolRecord {
            symbol_id: format!("sym_{}_{}", file_path, name),
            file_path: file_path.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(|s| s.to_string()),
            start_line,
            end_line: start_line + 10,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.8,
            qname: Some(format!("{}.{}", file_path, name)),
            parent_symbol_id: parent_symbol_id.map(|s| s.to_string()),
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: Some(format!("uid:test_{}", name)),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    #[test]
    fn generates_defines_for_top_level_function() {
        let symbols = vec![make_symbol(
            "my_func",
            SymbolKind::Function,
            "src/main.rs",
            None,
            None,
            1,
        )];
        let edges = generate_hierarchy_edges(&symbols, &[]);
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].relation_kind, SemanticRelation::Defines);
        assert_eq!(edges[0].target_symbol, "my_func");
        assert!(edges[0].source_symbol.starts_with("file::"));
    }

    #[test]
    fn generates_defines_method_for_class_method() {
        let symbols = vec![
            make_symbol("MyClass", SymbolKind::Class, "src/lib.rs", None, None, 1),
            make_symbol(
                "do_stuff",
                SymbolKind::Method,
                "src/lib.rs",
                Some("MyClass"),
                None,
                5,
            ),
        ];
        let edges = generate_hierarchy_edges(&symbols, &[]);
        // Should have: Defines(file->MyClass) + DefinesMethod(MyClass->do_stuff)
        let defines: Vec<_> = edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::Defines)
            .collect();
        let defines_method: Vec<_> = edges
            .iter()
            .filter(|e| e.relation_kind == SemanticRelation::DefinesMethod)
            .collect();
        assert_eq!(defines.len(), 1);
        assert_eq!(defines_method.len(), 1);
        assert_eq!(defines_method[0].source_symbol, "MyClass");
        assert_eq!(defines_method[0].target_symbol, "do_stuff");
    }

    #[test]
    fn generates_contains_file_edges() {
        let file_paths = vec![
            "src/main.rs".to_string(),
            "src/lib.rs".to_string(),
            "tests/test_a.rs".to_string(),
        ];
        let edges = generate_hierarchy_edges(&[], &file_paths);
        assert_eq!(edges.len(), 3);
        for e in &edges {
            assert_eq!(e.relation_kind, SemanticRelation::ContainsFile);
            assert_eq!(e.confidence, 1.0);
        }
    }

    #[test]
    fn skips_variable_and_property_for_defines() {
        let symbols = vec![
            make_symbol("x", SymbolKind::Variable, "a.ts", None, None, 1),
            make_symbol("p", SymbolKind::Property, "a.ts", None, None, 2),
        ];
        let edges = generate_hierarchy_edges(&symbols, &[]);
        assert!(edges.is_empty());
    }
}
