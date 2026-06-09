//! Type hierarchy queries: ancestors, descendants, implementors, overrides.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use cc_db::index_db::{IndexDb, SymbolRow};
use cc_model::edge::SemanticRelation;
use cc_model::{CcError, CcResult};
use serde_json::json;

use crate::graph_types::{HierarchyNode, OverrideInfo, TypeHierarchyResult, TypeNodeInfo};

/// Query the type hierarchy (ancestors, descendants, overrides) for a given type.
///
/// # Symbol disambiguation
/// 1. If `symbol_uid` is provided, look up the symbol by UID directly.
/// 2. If `file_path` is provided, find by name and filter by file.
/// 3. Otherwise, find by name and filter to class/interface/enum kinds.
/// 4. If multiple candidates remain, return a `{ "candidates": [...] }` JSON.
/// 5. If none found, return an error.
pub fn type_hierarchy(
    db: &Arc<IndexDb>,
    type_name: &str,
    file_path: Option<&str>,
    symbol_uid: Option<&str>,
    direction: &str,
    max_depth: usize,
    include_methods: bool,
) -> CcResult<serde_json::Value> {
    // ── Step 1: Resolve the root symbol ─────────────────────────
    let root = resolve_root_symbol(db, type_name, file_path, symbol_uid)?;
    let root = match root {
        ResolveResult::Single(row) => row,
        ResolveResult::Candidates(candidates) => {
            return Ok(json!({
                "query": type_name,
                "candidates": candidates.iter().map(|c| json!({
                    "name": c.name,
                    "kind": c.kind,
                    "file_path": c.file_path,
                    "uid": c.symbol_uid,
                })).collect::<Vec<_>>(),
            }));
        }
    };

    let root_uid = root
        .symbol_uid
        .as_deref()
        .ok_or_else(|| CcError::Other("root symbol has no UID".into()))?
        .to_string();

    let type_info = TypeNodeInfo {
        name: root.name.clone(),
        kind: root.kind.clone(),
        file_path: root.file_path.clone(),
        uid: root_uid.clone(),
    };

    // ── Step 2: BFS for ancestors / descendants ─────────────────
    let mut ancestors = Vec::new();
    let mut descendants = Vec::new();
    let mut implementors = Vec::new();

    // Collect methods per type UID (for override detection).
    // Key: type_uid, Value: Vec<(method_name, method_uid)>
    let mut type_methods: HashMap<String, Vec<(String, Option<String>)>> = HashMap::new();

    // Collect root type's methods so override detection can compare root vs descendants.
    if include_methods {
        let root_method_edges =
            db.query_semantic_edges(Some(&root_uid), None, Some("defines_method"))?;
        let root_methods: Vec<(String, Option<String>)> = root_method_edges
            .iter()
            .map(|e| (e.target_symbol.clone(), e.target_symbol_uid.clone()))
            .collect();
        type_methods.insert(root_uid.clone(), root_methods);
    }

    if direction == "ancestors" || direction == "up" || direction == "both" {
        ancestors = bfs_ancestors(db, &root_uid, max_depth, include_methods, &mut type_methods)?;
    }
    if direction == "descendants" || direction == "down" || direction == "both" {
        let (desc, impls) =
            bfs_descendants(db, &root_uid, max_depth, include_methods, &mut type_methods)?;
        descendants = desc;
        implementors = impls;
    }

    // ── Step 3: Detect overrides ────────────────────────────────
    // Build a virtual list of "base types" = root + ancestors for override comparison.
    let root_as_node = HierarchyNode {
        name: root.name.clone(),
        kind: root.kind.clone(),
        file_path: root.file_path.clone(),
        uid: root_uid.clone(),
        relation: "self".to_string(),
        depth: 0,
        methods: Vec::new(),
    };
    let overrides = if include_methods {
        let base_types: Vec<&HierarchyNode> = std::iter::once(&root_as_node)
            .chain(ancestors.iter())
            .collect();
        detect_overrides(db, &base_types, &descendants, &type_methods)?
    } else {
        Vec::new()
    };

    let result = TypeHierarchyResult {
        type_info,
        ancestors,
        descendants,
        implementors,
        overrides,
    };
    serde_json::to_value(result).map_err(CcError::Serde)
}

// ── Internal helpers ────────────────────────────────────────────

enum ResolveResult {
    Single(SymbolRow),
    Candidates(Vec<SymbolRow>),
}

fn resolve_root_symbol(
    db: &IndexDb,
    type_name: &str,
    file_path: Option<&str>,
    symbol_uid: Option<&str>,
) -> CcResult<ResolveResult> {
    // 1. By UID
    if let Some(uid) = symbol_uid {
        let map = db.symbol_rows_by_uids(&[uid.to_string()])?;
        if let Some(row) = map.into_values().next() {
            return Ok(ResolveResult::Single(row));
        }
        return Err(CcError::Other(format!("no symbol with uid '{}'", uid)));
    }

    let rows = db.find_symbol(type_name, true, 10)?;
    if rows.is_empty() {
        return Err(CcError::Other(format!("no symbol named '{}'", type_name)));
    }

    // 2. Filter by file_path
    if let Some(fp) = file_path {
        let filtered: Vec<_> = rows.into_iter().filter(|r| r.file_path == fp).collect();
        return match filtered.len() {
            0 => Err(CcError::Other(format!(
                "no symbol named '{}' in file '{}'",
                type_name, fp
            ))),
            1 => Ok(ResolveResult::Single(filtered.into_iter().next().unwrap())),
            _ => Ok(ResolveResult::Candidates(filtered)),
        };
    }

    // 3. Filter to class/interface/enum kinds
    let type_kinds: HashSet<&str> = [
        "class",
        "interface",
        "enum",
        "struct",
        "trait",
        "abstract_class",
    ]
    .iter()
    .copied()
    .collect();
    let filtered: Vec<_> = rows
        .into_iter()
        .filter(|r| type_kinds.contains(r.kind.as_str()))
        .collect();
    match filtered.len() {
        0 => Err(CcError::Other(format!(
            "no class/interface/enum named '{}'",
            type_name
        ))),
        1 => Ok(ResolveResult::Single(filtered.into_iter().next().unwrap())),
        _ => Ok(ResolveResult::Candidates(filtered)),
    }
}

/// BFS upward: follow `inherits` / `implements` edges from source to target.
fn bfs_ancestors(
    db: &IndexDb,
    root_uid: &str,
    max_depth: usize,
    include_methods: bool,
    type_methods: &mut HashMap<String, Vec<(String, Option<String>)>>,
) -> CcResult<Vec<HierarchyNode>> {
    let mut result = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(root_uid.to_string());

    // (uid, depth)
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((root_uid.to_string(), 0));

    while let Some((current_uid, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        // Query outgoing edges from current_uid (source → target = parent)
        let edges = db.query_semantic_edges(Some(&current_uid), None, None)?;
        for edge in &edges {
            let rel = &edge.relation_kind;
            if *rel != SemanticRelation::Inherits && *rel != SemanticRelation::Implements {
                continue;
            }
            let target_uid = match &edge.target_symbol_uid {
                Some(uid) => uid.clone(),
                None => continue,
            };
            if visited.contains(&target_uid) {
                continue;
            }
            visited.insert(target_uid.clone());

            let relation_str = rel.as_str().to_string();
            let (name, kind, file_path, methods) = resolve_node_info(
                db,
                &target_uid,
                &edge.target_symbol,
                include_methods,
                type_methods,
            )?;

            result.push(HierarchyNode {
                name,
                kind,
                file_path,
                uid: target_uid.clone(),
                relation: relation_str,
                depth: depth + 1,
                methods,
            });

            queue.push_back((target_uid, depth + 1));
        }
    }
    Ok(result)
}

/// BFS downward: find edges where target_symbol_uid = our UID.
/// Returns (descendants, implementors) split by relation kind.
fn bfs_descendants(
    db: &IndexDb,
    root_uid: &str,
    max_depth: usize,
    include_methods: bool,
    type_methods: &mut HashMap<String, Vec<(String, Option<String>)>>,
) -> CcResult<(Vec<HierarchyNode>, Vec<HierarchyNode>)> {
    let mut descendants = Vec::new();
    let mut implementors = Vec::new();
    let mut visited = HashSet::new();
    visited.insert(root_uid.to_string());

    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    queue.push_back((root_uid.to_string(), 0));

    while let Some((current_uid, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        // Query edges where target_symbol_uid = current (children point TO us)
        let edges = db.query_semantic_edges(None, Some(&current_uid), None)?;
        for edge in &edges {
            let rel = &edge.relation_kind;
            if *rel != SemanticRelation::Inherits && *rel != SemanticRelation::Implements {
                continue;
            }
            let source_uid = match &edge.source_symbol_uid {
                Some(uid) => uid.clone(),
                None => continue,
            };
            if visited.contains(&source_uid) {
                continue;
            }
            visited.insert(source_uid.clone());

            let relation_str = rel.as_str().to_string();
            let (name, kind, file_path, methods) = resolve_node_info(
                db,
                &source_uid,
                &edge.source_symbol,
                include_methods,
                type_methods,
            )?;

            let node = HierarchyNode {
                name,
                kind,
                file_path,
                uid: source_uid.clone(),
                relation: relation_str.clone(),
                depth: depth + 1,
                methods,
            };

            if relation_str == "implements" {
                implementors.push(node);
            } else {
                descendants.push(node);
            }

            // Continue BFS from this child
            queue.push_back((source_uid, depth + 1));
        }
    }
    Ok((descendants, implementors))
}

/// Resolve name/kind/file_path for a symbol UID, and optionally collect its methods.
fn resolve_node_info(
    db: &IndexDb,
    uid: &str,
    fallback_name: &str,
    include_methods: bool,
    type_methods: &mut HashMap<String, Vec<(String, Option<String>)>>,
) -> CcResult<(String, String, String, Vec<String>)> {
    let map = db.symbol_rows_by_uids(&[uid.to_string()])?;
    let (name, kind, file_path) = if let Some(row) = map.get(uid) {
        (row.name.clone(), row.kind.clone(), row.file_path.clone())
    } else {
        (
            fallback_name.to_string(),
            "unknown".to_string(),
            String::new(),
        )
    };

    let mut methods = Vec::new();
    if include_methods {
        let method_edges = db.query_semantic_edges(Some(uid), None, Some("defines_method"))?;
        let mut method_list = Vec::new();
        for me in &method_edges {
            methods.push(me.target_symbol.clone());
            method_list.push((me.target_symbol.clone(), me.target_symbol_uid.clone()));
        }
        type_methods.insert(uid.to_string(), method_list);
    }

    Ok((name, kind, file_path, methods))
}

/// Detect override relationships between ancestors and descendants.
///
/// For each descendant that defines a method with the same name as an ancestor method,
/// it's considered an override. We also compare param_count when available.
fn detect_overrides(
    db: &IndexDb,
    base_types: &[&HierarchyNode],
    descendants: &[HierarchyNode],
    type_methods: &HashMap<String, Vec<(String, Option<String>)>>,
) -> CcResult<Vec<OverrideInfo>> {
    // Collect all base type methods: (method_name, base_type_name, method_uid)
    let mut ancestor_methods: Vec<(String, String, Option<String>)> = Vec::new();
    for anc in base_types {
        if let Some(methods) = type_methods.get(&anc.uid) {
            for (method_name, method_uid) in methods {
                ancestor_methods.push((method_name.clone(), anc.name.clone(), method_uid.clone()));
            }
        }
    }

    if ancestor_methods.is_empty() {
        return Ok(Vec::new());
    }

    // Build a set of ancestor method names for quick lookup
    let ancestor_method_names: HashSet<&str> = ancestor_methods
        .iter()
        .map(|(name, _, _)| name.as_str())
        .collect();

    // Collect all method UIDs we need to look up for param_count
    let mut method_uids_to_lookup: Vec<String> = Vec::new();
    for (_, _, uid) in &ancestor_methods {
        if let Some(u) = uid {
            method_uids_to_lookup.push(u.clone());
        }
    }
    for desc in descendants {
        if let Some(methods) = type_methods.get(&desc.uid) {
            for (name, uid) in methods {
                if ancestor_method_names.contains(name.as_str()) {
                    if let Some(u) = uid {
                        method_uids_to_lookup.push(u.clone());
                    }
                }
            }
        }
    }

    // Batch-lookup param_count via query_json
    let param_counts = lookup_param_counts(db, &method_uids_to_lookup)?;

    let mut overrides = Vec::new();
    for desc in descendants {
        if let Some(desc_methods) = type_methods.get(&desc.uid) {
            for (dm_name, dm_uid) in desc_methods {
                // Check if any ancestor has a method with the same name
                for (am_name, am_type_name, am_uid) in &ancestor_methods {
                    if dm_name != am_name {
                        continue;
                    }
                    // Check param_count compatibility
                    let am_pc = am_uid
                        .as_ref()
                        .and_then(|u| param_counts.get(u.as_str()))
                        .copied()
                        .flatten();
                    let dm_pc = dm_uid
                        .as_ref()
                        .and_then(|u| param_counts.get(u.as_str()))
                        .copied()
                        .flatten();

                    // If both have param_count, they must match; if either is None, accept
                    let compatible = match (am_pc, dm_pc) {
                        (Some(a), Some(d)) => a == d,
                        _ => true,
                    };
                    if compatible {
                        overrides.push(OverrideInfo {
                            method: dm_name.clone(),
                            overriding_type: desc.name.clone(),
                            base_type: am_type_name.clone(),
                            param_count: dm_pc,
                        });
                    }
                }
            }
        }
    }

    Ok(overrides)
}

/// Lookup param_count for a set of symbol UIDs. Returns uid → Option<u32>.
fn lookup_param_counts(db: &IndexDb, uids: &[String]) -> CcResult<HashMap<String, Option<u32>>> {
    let mut result = HashMap::new();
    if uids.is_empty() {
        return Ok(result);
    }

    // Use query_json to fetch param_count for each UID
    for chunk in uids.chunks(100) {
        let placeholders: Vec<String> = (1..=chunk.len()).map(|i| format!("?{}", i)).collect();
        let sql = format!(
            "SELECT symbol_uid, param_count FROM symbols WHERE symbol_uid IN ({})",
            placeholders.join(",")
        );
        let params: Vec<String> = chunk.to_vec();
        let rows = db.query_json(&sql, &params)?;
        for row in rows {
            if let Some(uid) = row.get("symbol_uid").and_then(|v| v.as_str()) {
                let pc = row
                    .get("param_count")
                    .and_then(|v| v.as_i64())
                    .map(|n| n as u32);
                result.insert(uid.to_string(), pc);
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Helper: create an IndexDb and insert a type hierarchy:
    ///   Interface A (uid_a)
    ///   Class B implements A (uid_b), defines method "doWork" (uid_b_dowork)
    ///   Class C extends B (uid_c), defines method "doWork" (uid_c_dowork)
    fn setup_hierarchy() -> (TempDir, Arc<IndexDb>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("hierarchy.db")).unwrap().0);
        let conn = db.read_conn().unwrap();

        // File record
        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/types.ts','TypeScript','h1',1.0,500,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();

        // Symbols: Interface A, Class B, Class C, methods doWork on B and C
        let symbols = [
            ("s_a", "uid_a", "A", "interface", 1, 10, None),
            ("s_b", "uid_b", "B", "class", 12, 30, None),
            ("s_c", "uid_c", "C", "class", 32, 50, None),
            (
                "s_b_dw",
                "uid_b_dowork",
                "doWork",
                "method",
                15,
                20,
                Some(2i64),
            ),
            (
                "s_c_dw",
                "uid_c_dowork",
                "doWork",
                "method",
                35,
                40,
                Some(2i64),
            ),
        ];
        for (sid, uid, name, kind, start, end, param_count) in &symbols {
            conn.execute(
                "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
                  start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
                  parent_symbol_id, export_name, is_default_export,
                  framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
                 VALUES(?1,?2,?3,?4,'src/types.ts',NULL,?5,?6,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,?7,NULL,NULL)",
                rusqlite::params![sid, uid, name, kind, start, end, param_count],
            ).unwrap();
        }

        // Semantic edges:
        // B implements A: source=B, target=A, relation=implements
        // C inherits B: source=C, target=B, relation=inherits
        // B defines_method doWork: source=B, target=doWork(B)
        // C defines_method doWork: source=C, target=doWork(C)
        let edges = [
            ("se_1", "uid_b", "B", "uid_a", "A", "implements"),
            ("se_2", "uid_c", "C", "uid_b", "B", "inherits"),
            (
                "se_3",
                "uid_b",
                "B",
                "uid_b_dowork",
                "doWork",
                "defines_method",
            ),
            (
                "se_4",
                "uid_c",
                "C",
                "uid_c_dowork",
                "doWork",
                "defines_method",
            ),
        ];
        for (eid, src_uid, src_name, tgt_uid, tgt_name, rel) in &edges {
            conn.execute(
                "INSERT INTO semantic_edges(edge_id, file_path, source_symbol, source_symbol_uid, target_symbol, target_symbol_uid, relation_kind, line, confidence, parser_tier)
                 VALUES(?1, 'src/types.ts', ?2, ?3, ?4, ?5, ?6, 1, 0.95, 'tree_sitter')",
                rusqlite::params![eid, src_name, src_uid, tgt_name, tgt_uid, rel],
            ).unwrap();
        }

        (tmp, db)
    }

    #[test]
    fn hierarchy_both_directions() {
        let (_tmp, db) = setup_hierarchy();

        let result = type_hierarchy(&db, "B", None, None, "both", 5, true).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        // type_info should be B
        assert_eq!(result.type_info.name, "B");
        assert_eq!(result.type_info.kind, "class");
        assert_eq!(result.type_info.uid, "uid_b");

        // ancestors: A (via implements)
        assert_eq!(result.ancestors.len(), 1);
        assert_eq!(result.ancestors[0].name, "A");
        assert_eq!(result.ancestors[0].relation, "implements");
        assert_eq!(result.ancestors[0].depth, 1);

        // descendants: C (via inherits)
        assert_eq!(result.descendants.len(), 1);
        assert_eq!(result.descendants[0].name, "C");
        assert_eq!(result.descendants[0].relation, "inherits");
        assert_eq!(result.descendants[0].depth, 1);

        // overrides: C.doWork overrides B.doWork (B is the root type that defines doWork)
        assert!(!result.overrides.is_empty());
        let ov = &result.overrides[0];
        assert_eq!(ov.method, "doWork");
        assert_eq!(ov.overriding_type, "C");
        assert_eq!(ov.base_type, "B");
        assert_eq!(ov.param_count, Some(2));
    }

    #[test]
    fn hierarchy_ancestors_only() {
        let (_tmp, db) = setup_hierarchy();

        let result = type_hierarchy(&db, "C", None, None, "ancestors", 5, false).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        assert_eq!(result.type_info.name, "C");

        // ancestors of C: B (inherits), then A (implements, via B)
        assert_eq!(result.ancestors.len(), 2);
        let ancestor_names: Vec<&str> = result.ancestors.iter().map(|a| a.name.as_str()).collect();
        assert!(ancestor_names.contains(&"B"));
        assert!(ancestor_names.contains(&"A"));

        // No descendants when direction=ancestors
        assert!(result.descendants.is_empty());
    }

    #[test]
    fn hierarchy_descendants_only() {
        let (_tmp, db) = setup_hierarchy();

        let result = type_hierarchy(&db, "A", None, None, "descendants", 5, true).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        assert_eq!(result.type_info.name, "A");
        assert_eq!(result.type_info.kind, "interface");

        // No ancestors when direction=descendants
        assert!(result.ancestors.is_empty());

        // B implements A (implementor), C inherits B (descendant)
        // B is an implementor because it's connected via "implements"
        assert!(!result.implementors.is_empty() || !result.descendants.is_empty());
        let all_names: Vec<&str> = result
            .implementors
            .iter()
            .chain(result.descendants.iter())
            .map(|n| n.name.as_str())
            .collect();
        assert!(all_names.contains(&"B"));
        assert!(all_names.contains(&"C"));
    }

    #[test]
    fn hierarchy_with_symbol_uid() {
        let (_tmp, db) = setup_hierarchy();

        let result = type_hierarchy(&db, "B", None, Some("uid_b"), "both", 5, false).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        assert_eq!(result.type_info.uid, "uid_b");
        assert_eq!(result.type_info.name, "B");
    }

    #[test]
    fn hierarchy_disambiguation_returns_candidates() {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("disamb.db")).unwrap().0);
        let conn = db.read_conn().unwrap();

        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/a.ts','TypeScript','h1',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO files(file_path, language, content_hash, mtime, size, summary, content_excerpt, parser_tier, parser_confidence, is_test_file, indexed_at)
             VALUES('src/b.ts','TypeScript','h2',1.0,100,'','','tree_sitter',1.0,0,'2024-01-01T00:00:00Z')",
            [],
        ).unwrap();

        // Two classes named "Client" in different files
        conn.execute(
            "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
              start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
              parent_symbol_id, export_name, is_default_export,
              framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
             VALUES('s1','uid_c1','Client','class','src/a.ts',NULL,1,10,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO symbols(symbol_id, symbol_uid, name, kind, file_path, container, start_line, end_line,
              start_col, end_col, signature, doc, parser_tier, parser_confidence, qname,
              parent_symbol_id, export_name, is_default_export,
              framework_role, receiver_type, param_types, return_type, param_count, base_types, implements)
             VALUES('s2','uid_c2','Client','class','src/b.ts',NULL,1,10,0,0,NULL,NULL,'tree_sitter',1.0,NULL,NULL,NULL,0,NULL,NULL,NULL,NULL,NULL,NULL,NULL)",
            [],
        ).unwrap();

        let result = type_hierarchy(&db, "Client", None, None, "both", 5, false).unwrap();

        // Should return candidates, not a hierarchy
        assert!(result.get("candidates").is_some());
        let candidates = result["candidates"].as_array().unwrap();
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn hierarchy_empty_db_returns_error() {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(IndexDb::open(&tmp.path().join("empty.db")).unwrap().0);

        let result = type_hierarchy(&db, "NonExistent", None, None, "both", 5, false);
        assert!(result.is_err());
    }

    #[test]
    fn hierarchy_max_depth_limits_traversal() {
        let (_tmp, db) = setup_hierarchy();

        // With max_depth=1, ancestors of C should only include B (not A)
        let result = type_hierarchy(&db, "C", None, None, "ancestors", 1, false).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        assert_eq!(result.ancestors.len(), 1);
        assert_eq!(result.ancestors[0].name, "B");
    }

    #[test]
    fn hierarchy_include_methods_lists_method_names() {
        let (_tmp, db) = setup_hierarchy();

        let result = type_hierarchy(&db, "B", None, None, "ancestors", 5, true).unwrap();
        let result: TypeHierarchyResult = serde_json::from_value(result).unwrap();

        // A has no defines_method edges, so methods should be empty
        assert!(result.ancestors[0].methods.is_empty());

        // Now check descendants of A — B should have "doWork"
        let result2 = type_hierarchy(&db, "A", None, None, "descendants", 5, true).unwrap();
        let result2: TypeHierarchyResult = serde_json::from_value(result2).unwrap();

        let b_node = result2
            .implementors
            .iter()
            .chain(result2.descendants.iter())
            .find(|n| n.name == "B")
            .expect("B should be in descendants/implementors");
        assert!(b_node.methods.contains(&"doWork".to_string()));
    }
}
