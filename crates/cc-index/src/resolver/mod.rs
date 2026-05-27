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

pub use cargo_workspace::{resolve_cargo_workspace, resolve_rust_workspace_import};
pub use catalog::SymbolCatalog;
pub use types::{CatalogScope, ImportBinding, InternalResKind, ResolutionContext, ResolveResult};

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
        catalog.add_symbols(&[func_sym.clone()]);

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
        catalog.add_symbols(&[func_sym.clone()]);

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
        catalog.add_symbols(&[func_sym.clone()]);

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
        catalog.add_symbols(&[func_sym.clone()]);

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
