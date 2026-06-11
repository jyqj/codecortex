//! Symbol catalog and recursive cross-file resolution.
//!
//! SymbolCatalog builds a lookup directory from parsed symbols, then resolves
//! call-edges and symbol-refs to their target symbol IDs / UIDs using a
//! multi-layer strategy: scope bindings → same-file candidates → imports →
//! global unique name fallback.

pub mod cargo_workspace;
pub(crate) mod catalog;
pub(crate) mod helpers;
pub(crate) mod resolve_core;
pub(crate) mod resolve_outcome;
pub(crate) mod route_resolve;
pub(crate) mod type_edges;
pub(crate) mod types;

pub(crate) use cargo_workspace::{resolve_cargo_workspace, resolve_rust_workspace_import};
pub(crate) use catalog::SymbolCatalog;
pub(crate) use types::ResolutionContext;
#[cfg(test)]
pub(crate) use types::{
    CallSiteSignals, CatalogScope, ImportBinding, InternalResKind, ResolveStep, RESOLVE_LADDER,
};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::helpers::*;
    use super::*;
    use cc_model::edge::SemanticRelation;
    use cc_model::scope::ScopeBinding;
    use cc_model::symbol::SymbolKind;
    use cc_model::ParserTier;
    use std::collections::HashMap;

    #[allow(clippy::too_many_arguments)]
    fn make_symbol(
        name: &str,
        file: &str,
        uid: Option<&str>,
        kind: SymbolKind,
        container: Option<&str>,
        qname: Option<&str>,
        start_line: u32,
        end_line: u32,
    ) -> cc_model::symbol::SymbolRecord {
        cc_model::symbol::SymbolRecord {
            symbol_id: format!("sym_{}", name),
            file_path: file.to_string(),
            name: name.to_string(),
            kind,
            container: container.map(String::from),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            signature: None,
            doc: None,
            parser_tier: ParserTier::TreeSitter,
            parser_confidence: 0.8,
            qname: qname.map(String::from),
            parent_symbol_id: None,
            scope_id: None,
            export_name: None,
            is_default_export: false,
            symbol_uid: uid.map(String::from),
            framework_role: None,
            receiver_type: None,
            param_types: None,
            return_type: None,
            param_count: None,
            base_types: None,
            implements: None,
        }
    }

    fn make_simple_symbol(
        name: &str,
        file: &str,
        uid: Option<&str>,
    ) -> cc_model::symbol::SymbolRecord {
        make_symbol(
            name,
            file,
            uid,
            SymbolKind::Function,
            None,
            Some(&format!(
                "{}.{}",
                file.replace('/', ".").trim_end_matches(".py"),
                name
            )),
            1,
            10,
        )
    }

    fn make_scope(
        id: &str,
        parent: Option<&str>,
        name: &str,
        file: &str,
        start: u32,
        end: u32,
        bindings: Vec<ScopeBinding>,
    ) -> CatalogScope {
        CatalogScope {
            scope_id: id.to_string(),
            parent_id: parent.map(String::from),
            name: name.to_string(),
            file_path: file.to_string(),
            start_line: start,
            end_line: end,
            bindings,
        }
    }

    // ------------------------------------------------------------------
    // Test 1: scope_chain walks parents correctly
    // ------------------------------------------------------------------
    #[test]
    fn test_scope_chain_walks_parents() {
        let catalog = SymbolCatalog::new();
        let mut scopes = HashMap::new();
        scopes.insert(
            "s1".to_string(),
            make_scope("s1", None, "module", "a.py", 1, 100, vec![]),
        );
        scopes.insert(
            "s2".to_string(),
            make_scope("s2", Some("s1"), "func_a", "a.py", 5, 50, vec![]),
        );
        scopes.insert(
            "s3".to_string(),
            make_scope("s3", Some("s2"), "if_block", "a.py", 10, 30, vec![]),
        );

        let chain = catalog.scope_chain(&scopes, "s3");
        let ids: Vec<&str> = chain.iter().map(|s| s.scope_id.as_str()).collect();
        assert_eq!(ids, vec!["s3", "s2", "s1"]);
    }

    // ------------------------------------------------------------------
    // Test 2: resolve_member_chain resolves "a.b.c"
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_member_chain() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_symbol(
                "MyClass",
                "app.py",
                Some("uid:class"),
                SymbolKind::Class,
                None,
                Some("MyClass"),
                1,
                50,
            ),
            make_symbol(
                "service",
                "app.py",
                Some("uid:svc"),
                SymbolKind::Variable,
                None,
                Some("service"),
                1,
                50,
            ),
            make_symbol(
                "handler",
                "app.py",
                Some("uid:handler"),
                SymbolKind::Method,
                Some("MyClass"),
                Some("MyClass.handler"),
                10,
                20,
            ),
            make_symbol(
                "inner",
                "app.py",
                Some("uid:inner"),
                SymbolKind::Method,
                Some("MyClass.handler"),
                Some("MyClass.handler.inner"),
                12,
                18,
            ),
        ]);

        // Resolve "MyClass.handler.inner"
        let parts = vec!["MyClass", "handler", "inner"];
        let result = catalog.resolve_member_chain(&parts, "app.py");
        assert!(result.is_some());
        assert_eq!(catalog.entry(result.unwrap()).symbol_id, "sym_inner");
    }

    // ------------------------------------------------------------------
    // Test 3: resolve_via_imports follows import binding
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_via_imports() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_symbol(
            "helper",
            "utils.py",
            Some("uid:helper"),
            SymbolKind::Function,
            None,
            Some("helper"),
            1,
            10,
        )]);

        let imports = vec![ImportBinding {
            local_name: "helper".to_string(),
            source_module: "utils.py".to_string(),
            imported_name: Some("helper".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_via_imports(&imports, "helper");
        assert!(result.is_some());
        assert_eq!(catalog.entry(result.unwrap()).symbol_id, "sym_helper");
    }

    // ------------------------------------------------------------------
    // Test 4: resolve_name full pipeline
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_name_full_pipeline() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_symbol(
                "local_func",
                "main.py",
                Some("uid:local"),
                SymbolKind::Function,
                None,
                Some("local_func"),
                5,
                15,
            ),
            make_symbol(
                "imported_func",
                "lib.py",
                Some("uid:imported"),
                SymbolKind::Function,
                None,
                Some("imported_func"),
                1,
                10,
            ),
            make_symbol(
                "unique_global",
                "other.py",
                Some("uid:global"),
                SymbolKind::Function,
                None,
                Some("unique_global"),
                1,
                10,
            ),
        ]);

        let mut scopes = HashMap::new();
        scopes.insert(
            "mod".to_string(),
            make_scope(
                "mod",
                None,
                "module",
                "main.py",
                1,
                100,
                vec![ScopeBinding {
                    name: "local_func".to_string(),
                    kind: "function".to_string(),
                    symbol_uid: Some("uid:local".to_string()),
                }],
            ),
        );

        let imports = vec![ImportBinding {
            local_name: "imported_func".to_string(),
            source_module: "lib.py".to_string(),
            imported_name: Some("imported_func".to_string()),
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        // 1. Scope binding resolution
        let result = catalog.resolve_name("local_func", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(catalog.entry(r.catalog_index).symbol_id, "sym_local_func");
        assert_eq!(r.resolution_kind, InternalResKind::ScopeResolved);

        // 2. Import resolution
        let result = catalog.resolve_name("imported_func", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            catalog.entry(r.catalog_index).symbol_id,
            "sym_imported_func"
        );
        // imported_func is resolved either via same-file or import depending on catalog state
        // In this case it should resolve via imports since it's in lib.py
        assert!(
            r.resolution_kind == InternalResKind::ImportResolved
                || r.resolution_kind == InternalResKind::Heuristic
        );

        // 3. Global unique fallback
        let result = catalog.resolve_name("unique_global", "main.py", 10, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(
            catalog.entry(r.catalog_index).symbol_id,
            "sym_unique_global"
        );
        assert_eq!(r.resolution_kind, InternalResKind::GlobalUnique);
    }

    // ------------------------------------------------------------------
    // Test 5: original test — resolve_prefers_same_file
    // ------------------------------------------------------------------
    #[test]
    fn resolve_prefers_same_file() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_simple_symbol("greet", "a.py", Some("uid:a")),
            make_simple_symbol("greet", "b.py", Some("uid:b")),
        ]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.call_edges.push(cc_model::edge::CallEdgeRecord {
            edge_id: "e1".into(),
            file_path: "a.py".into(),
            callee_symbol: "greet".into(),
            line: 5,
            ..Default::default()
        });

        catalog.resolve_outcome("a.py", &mut outcome);
        assert_eq!(
            outcome.call_edges[0].callee_symbol_uid.as_deref(),
            Some("uid:a")
        );
    }

    // ------------------------------------------------------------------
    // Test 6: scope_distance returns correct hop count
    // ------------------------------------------------------------------
    #[test]
    fn test_scope_distance() {
        let catalog = SymbolCatalog::new();
        let mut scopes = HashMap::new();
        scopes.insert(
            "s1".to_string(),
            make_scope("s1", None, "module", "a.py", 1, 100, vec![]),
        );
        scopes.insert(
            "s2".to_string(),
            make_scope("s2", Some("s1"), "func", "a.py", 5, 50, vec![]),
        );
        scopes.insert(
            "s3".to_string(),
            make_scope("s3", Some("s2"), "block", "a.py", 10, 30, vec![]),
        );

        assert_eq!(catalog.scope_distance(&scopes, "s3", "s3"), Some(0));
        assert_eq!(catalog.scope_distance(&scopes, "s3", "s2"), Some(1));
        assert_eq!(catalog.scope_distance(&scopes, "s3", "s1"), Some(2));
        assert_eq!(catalog.scope_distance(&scopes, "s1", "s3"), None);
    }

    // ------------------------------------------------------------------
    // Test 7: build_alias_map
    // ------------------------------------------------------------------
    #[test]
    fn test_build_alias_map() {
        let imports = vec![
            ImportBinding {
                local_name: "helper".to_string(),
                source_module: "utils.py".to_string(),
                imported_name: Some("do_help".to_string()),
                file_path: "app.py".to_string(),
                is_namespace: false,
                is_default: false,
            },
            ImportBinding {
                local_name: "ns".to_string(),
                source_module: "lib.py".to_string(),
                imported_name: None,
                file_path: "app.py".to_string(),
                is_namespace: true,
                is_default: false,
            },
        ];

        let map = SymbolCatalog::build_alias_map(&imports);
        assert_eq!(map.get("helper").unwrap(), "utils.py:do_help");
        assert_eq!(map.get("ns").unwrap(), "lib.py:ns");
    }

    // ------------------------------------------------------------------
    // Test 8: classify_call_kind
    // ------------------------------------------------------------------
    #[test]
    fn test_classify_call_kind() {
        let imports = vec![ImportBinding {
            local_name: "ext".to_string(),
            source_module: "lib.py".to_string(),
            imported_name: Some("ext".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        assert_eq!(
            SymbolCatalog::classify_call_kind("obj.method", &imports),
            "method"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("obj.ClassName", &imports),
            "constructor"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("ext", &imports),
            "imported"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("local_fn", &imports),
            "local"
        );
        assert_eq!(
            SymbolCatalog::classify_call_kind("MyClass", &imports),
            "constructor"
        );
    }

    // ------------------------------------------------------------------
    // Test 9: candidate_count_penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_candidate_count_penalty() {
        // 1-3 candidates: no penalty
        assert_eq!(candidate_count_penalty(0.5, 1), 0.5);
        assert_eq!(candidate_count_penalty(0.5, 3), 0.5);
        // 4+: linear decay
        let p4 = candidate_count_penalty(0.5, 4);
        assert!((p4 - 0.375).abs() < 1e-6); // 0.5 * 3/4
        let p6 = candidate_count_penalty(0.5, 6);
        assert!((p6 - 0.25).abs() < 1e-6); // 0.5 * 3/6
        let p10 = candidate_count_penalty(0.5, 10);
        assert!((p10 - 0.15).abs() < 1e-6); // 0.5 * 3/10
    }

    // ------------------------------------------------------------------
    // Test 10: import reachability
    // ------------------------------------------------------------------
    #[test]
    fn test_import_reachable() {
        let imports = vec![ImportBinding {
            local_name: "helper".to_string(),
            source_module: "src/utils/helpers.py".to_string(),
            imported_name: Some("helper".to_string()),
            file_path: "app.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        // Reachable: candidate is in the imported module tree
        assert!(is_import_reachable("src/utils/helpers.py", &imports));
        // Reachable: candidate is a parent/sibling path
        assert!(is_import_reachable("src/utils.py", &imports));
        // Unreachable: completely different module
        assert!(!is_import_reachable("tests/conftest.py", &imports));
        // Empty imports → always unreachable
        assert!(!is_import_reachable("anything.py", &[]));
    }

    // ------------------------------------------------------------------
    // Test 11: common_path_prefix_len
    // ------------------------------------------------------------------
    #[test]
    fn test_common_path_prefix_len() {
        // "src/pkg/module" vs "src/pkg/other" → 2 common segments ("src", "pkg")
        assert_eq!(
            common_path_prefix_len("src/pkg/module.py", "src/pkg/other.py"),
            2
        );
        assert_eq!(common_path_prefix_len("src/a.py", "tests/b.py"), 0);
        // Same file → all segments match ("src", "pkg", "mod")
        assert_eq!(
            common_path_prefix_len("src/pkg/mod.py", "src/pkg/mod.py"),
            3
        );
        // Cross-language same dir still matches directories
        assert_eq!(
            common_path_prefix_len("src/pkg/mod.py", "src/pkg/mod.ts"),
            3
        );
    }

    // ------------------------------------------------------------------
    // Test 12: best_by_import_distance picks closest path
    // ------------------------------------------------------------------
    #[test]
    fn test_best_by_import_distance() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_simple_symbol("helper", "src/utils/helpers.py", Some("uid:near")),
            make_simple_symbol("helper", "vendor/ext/helpers.py", Some("uid:far")),
        ]);

        let candidates = vec![0, 1];
        let result = best_by_import_distance(&catalog.entries, &candidates, "src/utils/main.py");
        assert_eq!(result, Some(0)); // src/utils is closer
    }

    // ------------------------------------------------------------------
    // Test 13: fuzzy single resolution with import-unreachable penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_fuzzy_single_unreachable_penalty() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_symbol(
            "rare_func",
            "vendor/deep/module.py",
            Some("uid:rare"),
            SymbolKind::Function,
            None,
            Some("vendor.deep.module.rare_func"),
            1,
            10,
        )]);

        let scopes = HashMap::new();
        let imports = vec![ImportBinding {
            local_name: "utils".to_string(),
            source_module: "src/utils.py".to_string(),
            imported_name: Some("utils".to_string()),
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_name("rare_func", "main.py", 5, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.resolution_kind, InternalResKind::GlobalUnique);
        // 0.75 * 0.6 = 0.45 (import-unreachable penalty)
        assert!((r.confidence - 0.45).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // Test 14: fuzzy multi resolution with candidate penalty
    // ------------------------------------------------------------------
    #[test]
    fn test_fuzzy_multi_candidate_penalty() {
        let mut catalog = SymbolCatalog::new();
        // 4 candidates with the same name in different files — use distinct symbol_ids
        let mut syms: Vec<cc_model::symbol::SymbolRecord> = vec![
            make_symbol(
                "process",
                "src/a.py",
                Some("uid:a"),
                SymbolKind::Function,
                None,
                Some("src.a.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "src/b.py",
                Some("uid:b"),
                SymbolKind::Function,
                None,
                Some("src.b.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "lib/c.py",
                Some("uid:c"),
                SymbolKind::Function,
                None,
                Some("lib.c.process"),
                1,
                10,
            ),
            make_symbol(
                "process",
                "vendor/d.py",
                Some("uid:d"),
                SymbolKind::Function,
                None,
                Some("vendor.d.process"),
                1,
                10,
            ),
        ];
        // Override symbol_ids to be unique (make_symbol uses name only)
        for (i, s) in syms.iter_mut().enumerate() {
            s.symbol_id = format!("sym_process_{}", i);
        }
        catalog.add_symbols(&syms);

        let scopes = HashMap::new();
        let imports = vec![ImportBinding {
            local_name: "a_mod".to_string(),
            source_module: "src/a.py".to_string(),
            imported_name: None,
            file_path: "main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_name("process", "src/main.py", 5, &scopes, &imports, None);
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.resolution_kind, InternalResKind::FuzzyMulti);
        // Should pick src/a.py (the only reachable candidate)
        assert_eq!(catalog.entries[r.catalog_index].file_path, "src/a.py");
        // Confidence: FuzzySingle base(0.40) * penalty(3/4) = 0.30
        assert!((r.confidence - 0.30).abs() < 1e-6);
    }

    // ------------------------------------------------------------------
    // type_atoms unit tests
    // ------------------------------------------------------------------

    #[test]
    fn test_type_atoms_simple() {
        assert_eq!(type_atoms("User"), vec!["User"]);
        assert_eq!(type_atoms("MyStruct"), vec!["MyStruct"]);
    }

    #[test]
    fn test_type_atoms_generics() {
        assert_eq!(type_atoms("Promise<User>"), vec!["Promise", "User"]);
        assert_eq!(
            type_atoms("Vec<Result<Foo, Bar>>"),
            vec!["Vec", "Result", "Foo", "Bar"]
        );
        // Single-char type param T is filtered
        assert_eq!(type_atoms("Option<T>"), vec!["Option"]);
    }

    #[test]
    fn test_type_atoms_union() {
        // Single-char uppercase letters are filtered as type params
        assert!(type_atoms("A | B").is_empty());
        // Real type names survive
        assert_eq!(type_atoms("User | Admin"), vec!["User", "Admin"]);
    }

    #[test]
    fn test_type_atoms_primitives_filtered() {
        assert!(type_atoms("int").is_empty());
        assert!(type_atoms("str").is_empty());
        assert!(type_atoms("bool").is_empty());
        assert!(type_atoms("void").is_empty());
        assert!(type_atoms("f64").is_empty());
        assert!(type_atoms("usize").is_empty());
    }

    #[test]
    fn test_type_atoms_single_char_type_params() {
        assert!(type_atoms("T").is_empty());
        assert!(type_atoms("K").is_empty());
        assert!(type_atoms("V").is_empty());
        assert!(type_atoms("E").is_empty());
    }

    #[test]
    fn test_type_atoms_pointer_reference() {
        assert_eq!(type_atoms("*http.Client"), vec!["http.Client"]);
        assert!(type_atoms("&str").is_empty()); // str is primitive
        assert_eq!(type_atoms("&MyType"), vec!["MyType"]);
    }

    #[test]
    fn test_type_atoms_python_brackets() {
        assert_eq!(type_atoms("Dict[str, Any]"), vec!["Dict"]);
        assert_eq!(
            type_atoms("List[Tuple[Foo, Bar]]"),
            vec!["List", "Tuple", "Foo", "Bar"]
        );
    }

    #[test]
    fn test_type_atoms_mixed_delimiters() {
        assert_eq!(
            type_atoms("Map<String, List<User>>"),
            vec!["Map", "List", "User"] // String is primitive
        );
    }

    // ------------------------------------------------------------------
    // is_type_like tests
    // ------------------------------------------------------------------

    #[test]
    fn test_is_type_like() {
        assert!(is_type_like(SymbolKind::Class));
        assert!(is_type_like(SymbolKind::Interface));
        assert!(is_type_like(SymbolKind::Enum));
        assert!(is_type_like(SymbolKind::TypeAlias));
        assert!(!is_type_like(SymbolKind::Function));
        assert!(!is_type_like(SymbolKind::Method));
        assert!(!is_type_like(SymbolKind::Variable));
    }

    // ------------------------------------------------------------------
    // derive_uses_type_edges tests
    // ------------------------------------------------------------------

    fn make_typed_symbol(
        name: &str,
        file: &str,
        uid: &str,
        kind: SymbolKind,
        receiver_type: Option<&str>,
        param_types: Option<&str>,
        return_type: Option<&str>,
    ) -> cc_model::symbol::SymbolRecord {
        let mut sym = make_symbol(name, file, Some(uid), kind, None, None, 10, 20);
        sym.receiver_type = receiver_type.map(String::from);
        sym.param_types = param_types.map(String::from);
        sym.return_type = return_type.map(String::from);
        sym
    }

    #[test]
    fn test_derive_uses_type_receiver() {
        let mut catalog = SymbolCatalog::new();

        let class_sym = make_symbol(
            "MyStruct",
            "src/main.rs",
            Some("uid:mystruct"),
            SymbolKind::Class,
            None,
            None,
            1,
            5,
        );
        let method_sym = make_typed_symbol(
            "do_work",
            "src/main.rs",
            "uid:do_work",
            SymbolKind::Method,
            Some("MyStruct"),
            None,
            None,
        );

        catalog.add_symbols(&[class_sym, method_sym.clone()]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(method_sym);

        catalog.derive_uses_type_edges("src/main.rs", &mut outcome);

        assert_eq!(outcome.semantic_edges.len(), 1);
        let edge = &outcome.semantic_edges[0];
        assert_eq!(edge.relation_kind, SemanticRelation::UsesType);
        assert_eq!(edge.source_symbol, "do_work");
        assert_eq!(edge.target_symbol, "MyStruct");
        assert_eq!(edge.target_symbol_uid, Some("uid:mystruct".to_string()));
        assert!((edge.confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_derive_uses_type_params() {
        let mut catalog = SymbolCatalog::new();

        let user_sym = make_symbol(
            "User",
            "src/models.rs",
            Some("uid:user"),
            SymbolKind::Class,
            None,
            None,
            1,
            10,
        );
        let config_sym = make_symbol(
            "Config",
            "src/config.rs",
            Some("uid:config"),
            SymbolKind::Class,
            None,
            None,
            1,
            10,
        );
        let func_sym = make_typed_symbol(
            "process",
            "src/handler.rs",
            "uid:process",
            SymbolKind::Function,
            None,
            Some("User, int, Config"),
            None,
        );

        catalog.add_symbols(&[user_sym, config_sym, func_sym.clone()]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/handler.rs", &mut outcome);

        // Should have edges for User and Config, but not int (primitive)
        assert_eq!(outcome.semantic_edges.len(), 2);
        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"User"));
        assert!(targets.contains(&"Config"));
    }

    #[test]
    fn test_derive_uses_type_return() {
        let mut catalog = SymbolCatalog::new();

        let result_sym = make_symbol(
            "Result",
            "src/types.rs",
            Some("uid:result"),
            SymbolKind::Enum,
            None,
            None,
            1,
            10,
        );
        let func_sym = make_typed_symbol(
            "fetch",
            "src/api.rs",
            "uid:fetch",
            SymbolKind::Function,
            None,
            None,
            Some("Result<User>"),
        );

        catalog.add_symbols(&[result_sym, func_sym.clone()]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/api.rs", &mut outcome);

        // Should have edges for Result and User
        assert_eq!(outcome.semantic_edges.len(), 2);
        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"Result"));
        assert!(targets.contains(&"User"));
    }

    #[test]
    fn test_derive_uses_type_generic_split() {
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "transform",
            "src/lib.rs",
            "uid:transform",
            SymbolKind::Function,
            None,
            Some("Vec<Foo>"),
            Some("HashMap<Bar, Baz>"),
        );
        catalog.add_symbols(std::slice::from_ref(&func_sym));

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        let targets: Vec<&str> = outcome
            .semantic_edges
            .iter()
            .map(|e| e.target_symbol.as_str())
            .collect();
        assert!(targets.contains(&"Vec"));
        assert!(targets.contains(&"Foo"));
        assert!(targets.contains(&"HashMap"));
        assert!(targets.contains(&"Bar"));
        assert!(targets.contains(&"Baz"));
        assert_eq!(outcome.semantic_edges.len(), 5);
    }

    #[test]
    fn test_derive_uses_type_dedup() {
        // Same type appears in both param_types and return_type — should only get one edge
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "roundtrip",
            "src/lib.rs",
            "uid:roundtrip",
            SymbolKind::Function,
            None,
            Some("User"),
            Some("User"),
        );
        catalog.add_symbols(std::slice::from_ref(&func_sym));

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        assert_eq!(outcome.semantic_edges.len(), 1);
        assert_eq!(outcome.semantic_edges[0].target_symbol, "User");
    }

    #[test]
    fn test_derive_uses_type_no_uid_symbol_skipped() {
        // Symbols without symbol_uid should be skipped
        let mut catalog = SymbolCatalog::new();

        let mut func_sym = make_typed_symbol(
            "nouid",
            "src/lib.rs",
            "uid:nouid",
            SymbolKind::Function,
            None,
            Some("Foo"),
            None,
        );
        func_sym.symbol_uid = None; // remove UID
        catalog.add_symbols(std::slice::from_ref(&func_sym));

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/lib.rs", &mut outcome);

        assert!(outcome.semantic_edges.is_empty());
    }

    #[test]
    fn test_derive_uses_type_primitives_only() {
        // All types are primitives — no edges should be generated
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_typed_symbol(
            "add",
            "src/math.rs",
            "uid:add",
            SymbolKind::Function,
            None,
            Some("int, float, bool"),
            Some("f64"),
        );
        catalog.add_symbols(std::slice::from_ref(&func_sym));

        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.symbols.push(func_sym);

        catalog.derive_uses_type_edges("src/math.rs", &mut outcome);

        assert!(outcome.semantic_edges.is_empty());
    }

    // ------------------------------------------------------------------
    // Route handler resolution tests
    // ------------------------------------------------------------------

    fn make_route_edge(
        file: &str,
        route: &str,
        handler: Option<&str>,
        line: u32,
    ) -> cc_model::edge::RouteEdgeRecord {
        cc_model::edge::RouteEdgeRecord {
            edge_id: format!("route_{}", route),
            file_path: file.to_string(),
            route_path: route.to_string(),
            handler_name: handler.map(String::from),
            method: Some("GET".to_string()),
            line,
            start_col: 0,
            end_line: None,
            end_col: 0,
            handler_symbol_id: None,
            handler_symbol_uid: None,
            handler_expr: None,
            router_symbol_uid: None,
            framework: None,
            route_kind: None,
            confidence: 0.8,
            parser_tier: ParserTier::TreeSitter,
        }
    }

    #[test]
    fn test_route_dotted_handler_via_import() {
        // Scenario: import { userCtrl } from './controllers/user'
        //           app.get('/users', userCtrl.getUsers)
        let mut catalog = SymbolCatalog::new();

        // Controller file has a class with a method
        let class_sym = make_symbol(
            "UserController",
            "src/controllers/user.ts",
            Some("uid:UserController"),
            SymbolKind::Class,
            None,
            Some("UserController"),
            1,
            50,
        );
        let method_sym = make_symbol(
            "getUsers",
            "src/controllers/user.ts",
            Some("uid:getUsers"),
            SymbolKind::Method,
            Some("UserController"),
            Some("UserController.getUsers"),
            10,
            20,
        );
        catalog.add_symbols(&[class_sym, method_sym]);

        let imports = vec![ImportBinding {
            local_name: "userCtrl".to_string(),
            source_module: "src/controllers/user.ts".to_string(),
            imported_name: Some("UserController".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let scopes = HashMap::new();
        let result =
            catalog.resolve_dotted_handler("userCtrl.getUsers", "src/routes.ts", &scopes, &imports);
        assert!(result.is_some(), "Should resolve dotted handler via import");
        let idx = result.unwrap();
        assert_eq!(catalog.entries[idx].name, "getUsers");
        assert_eq!(
            catalog.entries[idx].symbol_uid.as_deref(),
            Some("uid:getUsers")
        );
    }

    #[test]
    fn test_route_dotted_handler_via_qname() {
        // Scenario: controllers.users.list — resolved via qualified name
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_symbol(
            "list",
            "src/controllers/users.ts",
            Some("uid:list"),
            SymbolKind::Function,
            Some("users"),
            Some("controllers.users.list"),
            5,
            15,
        );
        catalog.add_symbols(&[func_sym]);

        let scopes = HashMap::new();
        let imports = vec![];
        let result = catalog.resolve_dotted_handler(
            "controllers.users.list",
            "src/app.ts",
            &scopes,
            &imports,
        );
        assert!(result.is_some(), "Should resolve dotted handler via qname");
        assert_eq!(catalog.entries[result.unwrap()].name, "list");
    }

    #[test]
    fn test_route_handler_global_prefers_imported() {
        // Scenario: handler "getUsers" exists in two files; one is imported
        let mut catalog = SymbolCatalog::new();

        let sym_a = make_symbol(
            "getUsers",
            "src/controllers/user.ts",
            Some("uid:getUsers_ctrl"),
            SymbolKind::Function,
            None,
            Some("user.getUsers"),
            1,
            10,
        );
        let sym_b = make_symbol(
            "getUsers",
            "src/test/mock.ts",
            Some("uid:getUsers_mock"),
            SymbolKind::Function,
            None,
            Some("mock.getUsers"),
            1,
            10,
        );
        catalog.add_symbols(&[sym_a, sym_b]);

        let imports = vec![ImportBinding {
            local_name: "userCtrl".to_string(),
            source_module: "src/controllers/user.ts".to_string(),
            imported_name: Some("UserController".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog.resolve_handler_global("getUsers", "src/routes.ts", &imports);
        assert!(result.is_some(), "Should resolve via global handler lookup");
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:getUsers_ctrl"),
            "Should prefer the handler in the imported file"
        );
    }

    #[test]
    fn test_route_handler_global_unique_function() {
        // Scenario: handler name is globally unique — resolves directly
        let mut catalog = SymbolCatalog::new();

        let func_sym = make_symbol(
            "handleLogin",
            "src/auth/handler.ts",
            Some("uid:handleLogin"),
            SymbolKind::Function,
            None,
            Some("handler.handleLogin"),
            1,
            10,
        );
        catalog.add_symbols(&[func_sym]);

        let imports = vec![];
        let result = catalog.resolve_handler_global("handleLogin", "src/routes.ts", &imports);
        assert!(result.is_some());
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:handleLogin")
        );
    }

    #[test]
    fn test_route_handler_global_skips_non_function() {
        // Scenario: "User" exists as a class — should not resolve as handler
        let mut catalog = SymbolCatalog::new();

        let class_sym = make_symbol(
            "User",
            "src/models/user.ts",
            Some("uid:User"),
            SymbolKind::Class,
            None,
            Some("User"),
            1,
            50,
        );
        catalog.add_symbols(&[class_sym]);

        let imports = vec![];
        let result = catalog.resolve_handler_global("User", "src/routes.ts", &imports);
        assert!(result.is_none(), "Should not resolve class as handler");
    }

    #[test]
    fn test_route_resolve_outcome_three_tier() {
        // Integration test: verify the full three-tier pipeline in resolve_outcome
        let mut catalog = SymbolCatalog::new();

        // Cross-file handler (dotted)
        let ctrl_class = make_symbol(
            "AuthController",
            "src/controllers/auth.ts",
            Some("uid:AuthController"),
            SymbolKind::Class,
            None,
            Some("AuthController"),
            1,
            100,
        );
        let ctrl_method = make_symbol(
            "login",
            "src/controllers/auth.ts",
            Some("uid:login"),
            SymbolKind::Method,
            Some("AuthController"),
            Some("AuthController.login"),
            10,
            30,
        );
        // Simple name handler (tier 2/3)
        let handler_func = make_symbol(
            "healthCheck",
            "src/handlers/health.ts",
            Some("uid:healthCheck"),
            SymbolKind::Function,
            None,
            Some("health.healthCheck"),
            1,
            10,
        );

        catalog.add_symbols(&[ctrl_class, ctrl_method, handler_func]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        // Import in the route file
        outcome.imports.push(cc_model::edge::ImportRecord {
            import_string: "./controllers/auth".to_string(),
            imported_name: Some("AuthController".to_string()),
            alias: Some("authCtrl".to_string()),
            resolved_path: Some("src/controllers/auth.ts".to_string()),
            file_path: "src/routes.ts".to_string(),
            is_namespace: false,
            is_default: false,
            is_reexport: false,
        });

        // Route 1: dotted handler (tier 1)
        outcome.route_edges.push(make_route_edge(
            "src/routes.ts",
            "/login",
            Some("authCtrl.login"),
            5,
        ));

        // Route 2: simple cross-file handler (tier 3)
        outcome.route_edges.push(make_route_edge(
            "src/routes.ts",
            "/health",
            Some("healthCheck"),
            10,
        ));

        catalog.resolve_outcome("src/routes.ts", &mut outcome);

        // Verify route 1 resolved to the method
        assert_eq!(
            outcome.route_edges[0].handler_symbol_uid.as_deref(),
            Some("uid:login"),
            "Dotted handler should resolve via tier 1"
        );

        // Verify route 2 resolved to the function
        assert_eq!(
            outcome.route_edges[1].handler_symbol_uid.as_deref(),
            Some("uid:healthCheck"),
            "Simple handler should resolve via tier 2 or 3"
        );
    }

    #[test]
    fn test_route_already_resolved_skipped() {
        // Routes with handler_symbol_id already set should be skipped
        let mut catalog = SymbolCatalog::new();
        let func = make_simple_symbol("handler", "src/app.ts", Some("uid:handler"));
        catalog.add_symbols(&[func]);

        let mut outcome = cc_model::parse::ParseOutcome::default();
        let mut route = make_route_edge("src/app.ts", "/test", Some("handler"), 5);
        route.handler_symbol_id = Some("existing_id".to_string());
        route.handler_symbol_uid = Some("existing_uid".to_string());
        outcome.route_edges.push(route);

        catalog.resolve_outcome("src/app.ts", &mut outcome);

        // Should remain unchanged
        assert_eq!(
            outcome.route_edges[0].handler_symbol_uid.as_deref(),
            Some("existing_uid")
        );
    }

    #[test]
    fn test_route_namespace_import_dotted_handler() {
        // Scenario: import * as controllers from './controllers'
        //           app.get('/users', controllers.getUsers)
        let mut catalog = SymbolCatalog::new();

        // Create the symbol with an export_name so the namespace import can find it
        let mut func_sym = make_symbol(
            "getUsers",
            "src/controllers/index.ts",
            Some("uid:getUsers"),
            SymbolKind::Function,
            None,
            Some("controllers.getUsers"),
            5,
            15,
        );
        func_sym.export_name = Some("getUsers".to_string());
        catalog.add_symbols(&[func_sym]);

        let imports = vec![ImportBinding {
            local_name: "controllers".to_string(),
            source_module: "src/controllers/index.ts".to_string(),
            imported_name: None,
            file_path: "src/routes.ts".to_string(),
            is_namespace: true,
            is_default: false,
        }];

        let scopes = HashMap::new();
        let result = catalog.resolve_dotted_handler(
            "controllers.getUsers",
            "src/routes.ts",
            &scopes,
            &imports,
        );
        assert!(
            result.is_some(),
            "Should resolve namespace import dotted handler"
        );
        assert_eq!(
            catalog.entries[result.unwrap()].symbol_uid.as_deref(),
            Some("uid:getUsers")
        );
    }

    // ------------------------------------------------------------------
    // Test: resolve_name LRU cache returns consistent results
    // ------------------------------------------------------------------
    #[test]
    fn test_resolve_name_cache_returns_consistent_results() {
        let mut catalog = SymbolCatalog::new();
        let sym = make_simple_symbol("helper", "src/utils.py", Some("uid:helper"));
        catalog.add_symbols(&[sym]);

        let scopes = HashMap::new();
        let imports = vec![];

        // First call — uncached
        let result1 = catalog.resolve_name("helper", "src/utils.py", 5, &scopes, &imports, None);
        assert!(result1.is_some(), "Should resolve helper symbol");
        let r1 = result1.unwrap();

        // Second call — should be served from cache with identical result
        let result2 = catalog.resolve_name("helper", "src/utils.py", 5, &scopes, &imports, None);
        assert!(result2.is_some(), "Cached resolution should also succeed");
        let r2 = result2.unwrap();

        assert_eq!(r1.catalog_index, r2.catalog_index);
        assert_eq!(r1.resolution_kind, r2.resolution_kind);
        assert!(
            (r1.confidence - r2.confidence).abs() < f64::EPSILON,
            "Confidence should be identical"
        );
    }

    // ------------------------------------------------------------------
    // Call-site signal tests
    // ------------------------------------------------------------------

    /// Method symbol with receiver/param metadata and a unique symbol_id
    /// (make_symbol derives symbol_id from the name alone, which would let
    /// dedup_by_id collapse same-named candidates).
    fn make_method_with_meta(
        name: &str,
        file: &str,
        uid: &str,
        receiver_type: &str,
        param_count: u32,
    ) -> cc_model::symbol::SymbolRecord {
        let mut sym = make_symbol(
            name,
            file,
            Some(uid),
            SymbolKind::Method,
            Some(receiver_type),
            Some(&format!("{}.{}", receiver_type, name)),
            1,
            10,
        );
        sym.symbol_id = format!("sym_{}_{}", receiver_type, name);
        sym.receiver_type = Some(receiver_type.to_string());
        sym.param_count = Some(param_count);
        sym
    }

    fn parse_method_catalog(parser_params: u32, validator_params: u32) -> SymbolCatalog {
        let mut catalog = SymbolCatalog::new();
        let symbols = vec![
            make_method_with_meta(
                "parse",
                "src/parser.py",
                "uid:parser_parse",
                "Parser",
                parser_params,
            ),
            make_method_with_meta(
                "parse",
                "src/validator.py",
                "uid:validator_parse",
                "Validator",
                validator_params,
            ),
        ];
        catalog.add_symbols(&symbols);
        catalog.build_type_catalog(&symbols);
        catalog
    }

    #[test]
    fn test_fuzzy_signal_arg_count_disambiguation() {
        // Parser.parse takes 1 param, Validator.parse takes 2 — a call with
        // one argument must pick Parser.parse.
        let catalog = parse_method_catalog(1, 2);
        let scopes = HashMap::new();
        let imports = vec![];

        let signals = CallSiteSignals {
            arg_count: Some(1),
            receiver: None,
        };
        let result = catalog
            .resolve_name_with_signals("parse", "src/main.py", 5, &scopes, &imports, None, signals)
            .expect("arg-count signal should disambiguate");
        assert_eq!(
            catalog.entry(result.catalog_index).symbol_uid.as_deref(),
            Some("uid:parser_parse")
        );
        assert_eq!(result.resolution_kind, InternalResKind::FuzzySignal);
        assert_eq!(result.winning_step, ResolveStep::FuzzyArgCount);
        assert_eq!(result.candidate_count, 2);
        assert_eq!(result.strategy_name(), "fuzzy_arg_count");
        // Between FuzzySingle and GlobalUnique
        assert!(result.confidence > InternalResKind::FuzzySingle.base_confidence());
        assert!(result.confidence < InternalResKind::GlobalUnique.base_confidence());
    }

    #[test]
    fn test_fuzzy_signal_receiver_disambiguation() {
        // Equal param counts — only the receiver signal can discriminate.
        let catalog = parse_method_catalog(1, 1);
        let scopes = HashMap::new();
        let imports = vec![];

        let signals = CallSiteSignals {
            arg_count: None,
            receiver: Some("Validator"),
        };
        let result = catalog
            .resolve_name_with_signals("parse", "src/main.py", 5, &scopes, &imports, None, signals)
            .expect("receiver signal should disambiguate");
        assert_eq!(
            catalog.entry(result.catalog_index).symbol_uid.as_deref(),
            Some("uid:validator_parse")
        );
        assert_eq!(result.resolution_kind, InternalResKind::FuzzySignal);
        assert_eq!(result.winning_step, ResolveStep::FuzzyReceiver);
        assert_eq!(result.strategy_name(), "fuzzy_receiver");
    }

    #[test]
    fn test_fuzzy_arg_count_keeps_metadata_less_wildcards() {
        // Parser.parse declares 1 param; Untyped.parse has no recorded
        // param count. Arity evidence must not eliminate the wildcard, so
        // the pool stays at 2 and resolution falls through to the
        // import-distance step instead of a FuzzySignal win.
        let mut catalog = SymbolCatalog::new();
        let mut untyped =
            make_method_with_meta("parse", "src/untyped.py", "uid:untyped_parse", "Untyped", 0);
        untyped.param_count = None;
        let symbols = vec![
            make_method_with_meta("parse", "src/parser.py", "uid:parser_parse", "Parser", 1),
            untyped,
        ];
        catalog.add_symbols(&symbols);
        catalog.build_type_catalog(&symbols);

        let signals = CallSiteSignals {
            arg_count: Some(1),
            receiver: None,
        };
        let result = catalog
            .resolve_name_with_signals(
                "parse",
                "src/main.py",
                5,
                &HashMap::new(),
                &[],
                None,
                signals,
            )
            .expect("fuzzy fallback still resolves");
        assert_eq!(result.winning_step, ResolveStep::FuzzyImportDistance);
        assert_eq!(result.resolution_kind, InternalResKind::FuzzyMulti);
    }

    #[test]
    fn test_fuzzy_arg_count_defaulted_params_tier() {
        // No exact arity match: Parser.parse declares 3 params,
        // Validator.parse declares 0. A 1-argument call matches the
        // defaulted-params tier (param_count > arg_count) and must pick
        // Parser.parse rather than eliminating it.
        let catalog = parse_method_catalog(3, 0);
        let signals = CallSiteSignals {
            arg_count: Some(1),
            receiver: None,
        };
        let result = catalog
            .resolve_name_with_signals(
                "parse",
                "src/main.py",
                5,
                &HashMap::new(),
                &[],
                None,
                signals,
            )
            .expect("defaulted-params tier should disambiguate");
        assert_eq!(
            catalog.entry(result.catalog_index).symbol_uid.as_deref(),
            Some("uid:parser_parse")
        );
        assert_eq!(result.winning_step, ResolveStep::FuzzyArgCount);
        assert_eq!(result.resolution_kind, InternalResKind::FuzzySignal);
    }

    #[test]
    fn test_fuzzy_receiver_elimination_only_keeps_pool() {
        // Parser.parse is positively incompatible with the receiver, the
        // other candidate has no receiver metadata. Elimination-only
        // evidence must not narrow (one-level subtype check can be a false
        // negative), so the pool survives to import-distance.
        let mut catalog = SymbolCatalog::new();
        let mut untyped =
            make_method_with_meta("parse", "src/untyped.py", "uid:untyped_parse", "Untyped", 1);
        untyped.receiver_type = None;
        let symbols = vec![
            make_method_with_meta("parse", "src/parser.py", "uid:parser_parse", "Parser", 1),
            untyped,
        ];
        catalog.add_symbols(&symbols);
        catalog.build_type_catalog(&symbols);

        let signals = CallSiteSignals {
            arg_count: None,
            receiver: Some("Validator"),
        };
        let result = catalog
            .resolve_name_with_signals(
                "parse",
                "src/main.py",
                5,
                &HashMap::new(),
                &[],
                None,
                signals,
            )
            .expect("fuzzy fallback still resolves");
        assert_eq!(result.winning_step, ResolveStep::FuzzyImportDistance);
    }

    #[test]
    fn test_fuzzy_signal_unreachable_import_penalty() {
        // Same disambiguation as the arg-count test, but with an import
        // list that cannot reach the winner: FuzzySignal applies the 0.5x
        // unreachable-import penalty.
        let catalog = parse_method_catalog(1, 2);
        let imports = vec![ImportBinding {
            local_name: "other".to_string(),
            source_module: "pkg/elsewhere".to_string(),
            imported_name: None,
            file_path: "src/main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];
        let signals = CallSiteSignals {
            arg_count: Some(1),
            receiver: None,
        };
        let result = catalog
            .resolve_name_with_signals(
                "parse",
                "src/main.py",
                5,
                &HashMap::new(),
                &imports,
                None,
                signals,
            )
            .expect("arg-count signal should disambiguate");
        assert_eq!(result.resolution_kind, InternalResKind::FuzzySignal);
        assert!(
            (result.confidence - InternalResKind::FuzzySignal.base_confidence() * 0.5).abs()
                < f64::EPSILON
        );
    }

    #[test]
    fn test_resolve_ladder_order_locked() {
        assert_eq!(
            RESOLVE_LADDER,
            [
                ResolveStep::SelfMember,
                ResolveStep::ScopeBinding,
                ResolveStep::SameFile,
                ResolveStep::Import,
                ResolveStep::Suffix,
                ResolveStep::GlobalUnique,
                ResolveStep::FuzzyArgCount,
                ResolveStep::FuzzyReceiver,
                ResolveStep::FuzzyImportDistance,
            ]
        );
    }

    #[test]
    fn test_no_signal_resolution_matches_legacy_behavior() {
        // Method metadata is present, but without call-site signals the
        // ladder must skip the signal steps and fall through to the legacy
        // import-distance tie-breaking with unchanged confidence math.
        let catalog = parse_method_catalog(1, 2);
        let scopes = HashMap::new();
        let imports = vec![];

        let result = catalog
            .resolve_name("parse", "src/main.py", 5, &scopes, &imports, None)
            .expect("fuzzy multi should still resolve without signals");
        assert_eq!(result.resolution_kind, InternalResKind::FuzzyMulti);
        assert_eq!(result.winning_step, ResolveStep::FuzzyImportDistance);
        assert_eq!(result.candidate_count, 2);
        assert_eq!(result.strategy_name(), "fuzzy_multi");
        // Legacy confidence: FuzzyMulti base (0.30), no count penalty for 2
        // candidates, halved because no candidate is import-reachable.
        assert!((result.confidence - 0.15).abs() < 1e-6);
    }

    #[test]
    fn test_resolve_cache_key_includes_signals() {
        let catalog = parse_method_catalog(1, 2);
        let scopes = HashMap::new();
        let imports = vec![];

        // Signal-free resolution first populates the cache...
        let plain = catalog
            .resolve_name("parse", "src/main.py", 5, &scopes, &imports, None)
            .unwrap();
        assert_eq!(plain.resolution_kind, InternalResKind::FuzzyMulti);

        // ...and a signal-bearing call at the same site must not be served
        // the signal-free entry.
        let signals = CallSiteSignals {
            arg_count: Some(2),
            receiver: None,
        };
        let signaled = catalog
            .resolve_name_with_signals("parse", "src/main.py", 5, &scopes, &imports, None, signals)
            .unwrap();
        assert_eq!(signaled.resolution_kind, InternalResKind::FuzzySignal);
        assert_eq!(
            catalog.entry(signaled.catalog_index).symbol_uid.as_deref(),
            Some("uid:validator_parse")
        );

        // Repeating the signal call hits its own cache entry consistently.
        let signaled_again = catalog
            .resolve_name_with_signals("parse", "src/main.py", 5, &scopes, &imports, None, signals)
            .unwrap();
        assert_eq!(signaled_again.catalog_index, signaled.catalog_index);
        assert_eq!(signaled_again.winning_step, signaled.winning_step);
    }

    // ------------------------------------------------------------------
    // Ladder confidence-boundary tests: Suffix (0.65), GlobalUnique
    // (0.75), and the FuzzySingle confidence anchor (0.40). Each level
    // gets a minimal hit scenario and a just-miss scenario that falls to
    // the next level.
    // ------------------------------------------------------------------

    /// Symbol with a distinct symbol_id (make_simple_symbol derives the
    /// qname from the file path, e.g. "src/mod.py" → "src.mod.<name>").
    fn make_distinct_symbol(name: &str, file: &str, uid: &str) -> cc_model::symbol::SymbolRecord {
        let mut sym = make_simple_symbol(name, file, Some(uid));
        sym.symbol_id = format!("sym#{}#{}", file, name);
        sym
    }

    /// Resolve `name` from a symbol-free file with no scopes or imports, so
    /// only the global ladder steps (Suffix and later) can fire.
    fn resolve_global(catalog: &SymbolCatalog, name: &str) -> Option<types::ResolveResult> {
        catalog.resolve_name(name, "src/main.py", 5, &HashMap::new(), &[], None)
    }

    #[test]
    fn test_ladder_suffix_hit_boundary() {
        // qname "src.mod.helper" suffix-matches the dotted name "mod.helper".
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_distinct_symbol("helper", "src/mod.py", "uid:h")]);

        let result = resolve_global(&catalog, "mod.helper").expect("suffix match should resolve");
        assert_eq!(result.winning_step, ResolveStep::Suffix);
        assert_eq!(result.resolution_kind, InternalResKind::SuffixMatch);
        assert_eq!(result.candidate_count, 1);
        assert!(
            (result.confidence - InternalResKind::SuffixMatch.base_confidence()).abs() < 1e-9,
            "single suffix match must score the 0.65 base, got {}",
            result.confidence
        );
    }

    #[test]
    fn test_ladder_suffix_miss_falls_to_global_unique() {
        // qname "src.other.helper" does NOT end with ".mod.helper", so the
        // Suffix step misses and the globally unique leaf "helper" wins one
        // step later at the GlobalUnique confidence.
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_distinct_symbol("helper", "src/other.py", "uid:h")]);

        let result = resolve_global(&catalog, "mod.helper").expect("leaf should resolve");
        assert_eq!(result.winning_step, ResolveStep::GlobalUnique);
        assert_eq!(result.resolution_kind, InternalResKind::GlobalUnique);
        assert_eq!(result.candidate_count, 1);
        assert!((result.confidence - InternalResKind::GlobalUnique.base_confidence()).abs() < 1e-9);
    }

    #[test]
    fn test_ladder_global_unique_hit_boundary() {
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[make_distinct_symbol("helper", "src/lib.py", "uid:h")]);

        let result = resolve_global(&catalog, "helper").expect("unique leaf should resolve");
        assert_eq!(result.winning_step, ResolveStep::GlobalUnique);
        assert_eq!(result.candidate_count, 1);
        assert!(
            (result.confidence - InternalResKind::GlobalUnique.base_confidence()).abs() < 1e-9,
            "globally unique leaf must score the 0.75 base, got {}",
            result.confidence
        );
    }

    #[test]
    fn test_ladder_global_unique_miss_falls_to_import_distance() {
        // Two same-name candidates: GlobalUnique misses, and with no
        // call-site signals the signal steps are skipped, so resolution
        // falls directly to FuzzyImportDistance — the next reachable level
        // after GlobalUnique.
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_distinct_symbol("helper", "src/a.py", "uid:a"),
            make_distinct_symbol("helper", "lib/b.py", "uid:b"),
        ]);

        let result = resolve_global(&catalog, "helper").expect("fuzzy multi should resolve");
        assert_eq!(result.winning_step, ResolveStep::FuzzyImportDistance);
        assert_eq!(result.resolution_kind, InternalResKind::FuzzyMulti);
        assert_eq!(result.candidate_count, 2);
        // FuzzyMulti base (0.30), no count penalty for 2 candidates, halved
        // because no imports make any candidate reachable.
        assert!((result.confidence - 0.15).abs() < 1e-9);
    }

    #[test]
    fn test_ladder_fuzzy_single_confidence_via_unique_reachable() {
        // The FuzzySingle base (0.40) is observable when the import filter
        // narrows a multi-candidate pool to exactly one reachable winner:
        // the import-distance step promotes that winner to FuzzySingle
        // confidence while keeping the FuzzyMulti kind.
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[
            make_distinct_symbol("helper", "src/a.py", "uid:a"),
            make_distinct_symbol("helper", "lib/b.py", "uid:b"),
        ]);
        let imports = vec![ImportBinding {
            local_name: "a".to_string(),
            source_module: "src/a".to_string(),
            imported_name: None,
            file_path: "src/main.py".to_string(),
            is_namespace: false,
            is_default: false,
        }];

        let result = catalog
            .resolve_name("helper", "src/main.py", 5, &HashMap::new(), &imports, None)
            .expect("unique reachable candidate should resolve");
        assert_eq!(
            catalog.entry(result.catalog_index).symbol_uid.as_deref(),
            Some("uid:a")
        );
        assert_eq!(result.winning_step, ResolveStep::FuzzyImportDistance);
        assert_eq!(result.resolution_kind, InternalResKind::FuzzyMulti);
        assert_eq!(result.candidate_count, 2);
        assert!(
            (result.confidence - InternalResKind::FuzzySingle.base_confidence()).abs() < 1e-9,
            "unique reachable winner must score the FuzzySingle 0.40 base, got {}",
            result.confidence
        );
    }

    // ------------------------------------------------------------------
    // TypeCatalog upgrade-gate tests
    // ------------------------------------------------------------------

    fn make_preresolved_edge(
        kind: cc_model::edge::ResolutionKind,
        strategy: &str,
        confidence: f64,
        target_uid: &str,
        receiver: &str,
    ) -> cc_model::edge::CallEdgeRecord {
        cc_model::edge::CallEdgeRecord {
            edge_id: "e1".into(),
            file_path: "src/main.py".into(),
            callee_symbol: "v.parse".into(),
            receiver_expr: Some(receiver.to_string()),
            line: 5,
            target_symbol_id: Some("sym_pre".into()),
            target_file_path: Some("src/parser.py".into()),
            callee_symbol_uid: Some(target_uid.to_string()),
            resolution_kind: kind,
            resolution_confidence: confidence,
            resolution_strategy: strategy.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn test_type_catalog_upgrade_replaces_name_evidence_result() {
        // Edge pre-resolved by global_unique (name evidence only) to
        // Parser.parse, but the receiver is a Validator: the type catalog's
        // higher-confidence receiver match must replace it and record the
        // upgrade provenance.
        let catalog = parse_method_catalog(1, 2);
        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.call_edges.push(make_preresolved_edge(
            cc_model::edge::ResolutionKind::Heuristic,
            "global_unique",
            0.75,
            "uid:parser_parse",
            "Validator",
        ));

        catalog.resolve_outcome("src/main.py", &mut outcome);

        let edge = &outcome.call_edges[0];
        assert_eq!(
            edge.callee_symbol_uid.as_deref(),
            Some("uid:validator_parse")
        );
        assert!(edge.resolution_strategy.starts_with("receiver_type"));
        assert!(edge
            .resolution_strategy
            .contains("upgraded_from=global_unique"));
        assert!(edge.resolution_confidence > 0.75);
    }

    #[test]
    fn test_type_catalog_upgrade_skips_import_proven_result() {
        // Scope/import-proven results are never replaced, even when the type
        // catalog disagrees with higher confidence.
        let catalog = parse_method_catalog(1, 2);
        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.call_edges.push(make_preresolved_edge(
            cc_model::edge::ResolutionKind::ScopeResolved,
            "import_map",
            0.85,
            "uid:parser_parse",
            "Validator",
        ));

        catalog.resolve_outcome("src/main.py", &mut outcome);

        let edge = &outcome.call_edges[0];
        assert_eq!(edge.callee_symbol_uid.as_deref(), Some("uid:parser_parse"));
        assert_eq!(edge.resolution_strategy, "import_map");
        assert!((edge.resolution_confidence - 0.85).abs() < 1e-6);
    }

    #[test]
    fn test_type_catalog_upgrade_keeps_same_target() {
        // When the type catalog agrees with the main-chain target, the edge
        // keeps its original strategy and confidence (no pointless rewrite).
        let catalog = parse_method_catalog(1, 2);
        let mut outcome = cc_model::parse::ParseOutcome::default();
        outcome.call_edges.push(make_preresolved_edge(
            cc_model::edge::ResolutionKind::Heuristic,
            "global_unique",
            0.75,
            "uid:validator_parse",
            "Validator",
        ));

        catalog.resolve_outcome("src/main.py", &mut outcome);

        let edge = &outcome.call_edges[0];
        assert_eq!(
            edge.callee_symbol_uid.as_deref(),
            Some("uid:validator_parse")
        );
        assert_eq!(edge.resolution_strategy, "global_unique");
        assert!((edge.resolution_confidence - 0.75).abs() < 1e-6);
    }

    #[test]
    fn test_clear_resolve_cache() {
        let mut catalog = SymbolCatalog::new();
        let sym = make_simple_symbol("foo", "a.py", Some("uid:foo"));
        catalog.add_symbols(&[sym]);

        let scopes = HashMap::new();
        let imports = vec![];

        // Populate cache
        let _ = catalog.resolve_name("foo", "a.py", 1, &scopes, &imports, None);

        // Clear should not panic
        catalog.clear_resolve_cache();

        // Should still resolve correctly after cache clear
        let result = catalog.resolve_name("foo", "a.py", 1, &scopes, &imports, None);
        assert!(result.is_some(), "Should resolve after cache clear");
    }
}
