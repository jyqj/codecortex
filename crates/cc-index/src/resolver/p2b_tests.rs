use super::*;
use cc_model::{Language, ParseOutcome};
fn parsed(path: &str, source: &str) -> ParseOutcome {
    cc_parsers::ParserRegistry::new()
        .parse(path, source, Language::Python)
        .unwrap()
}
#[test]
fn p2b_chained_calls_do_not_share_identity_across_different_spans() {
    let source = include_str!("../../../cc-eval/fixtures/sample-project/api_handler.rs");
    let o = cc_parsers::ParserRegistry::new()
        .parse("api_handler.rs", source, Language::Rust)
        .unwrap();
    let mut sites = std::collections::BTreeMap::new();
    for edge in &o.call_edges {
        let identity = (
            &edge.callee_symbol,
            edge.line,
            edge.start_col,
            edge.end_line,
            edge.end_col,
        );
        if let Some(previous) = sites.insert(&edge.edge_id, identity) {
            assert_eq!(
                previous, identity,
                "distinct source calls share {}",
                edge.edge_id
            );
        }
    }
}
#[test]
fn p2b_sealing_never_resurrects_a_revoked_target() {
    let provider = parsed("a.py", "def service(x):\n    return x\n");
    let mut consumer = parsed("use.py", "def entry(x):\n    return service(x)\n");
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&provider.symbols);
    catalog.add_symbols(&consumer.symbols);
    catalog.resolve_outcome("use.py", &mut consumer);
    let edge = consumer
        .call_edges
        .iter_mut()
        .find(|e| e.callee_symbol == "service")
        .unwrap();
    assert!(edge.callee_symbol_uid.is_some());
    let id = edge.edge_id.clone();
    edge.callee_symbol_uid = None;
    edge.target_file_path = None;
    edge.target_symbol_id = None;
    catalog.seal_resolution_manifest("use.py", &mut consumer);
    let record = consumer
        .resolution
        .records
        .iter()
        .find(|r| r.site_kind == "call" && r.site_id == id)
        .unwrap();
    assert!(matches!(
        record.outcome,
        cc_model::resolution::ResolutionOutcome::Unresolved { .. }
    ));
}
#[test]
fn p2b_duplicate_import_aliases_are_not_first_writer_wins() {
    use super::types::{ImportBinding, NameRequest, NameResolution};
    let a = parsed("a.py", "def shared(x):\n    return x\n");
    let b = parsed("b.py", "def shared(x):\n    return x\n");
    let make = |path: &str| ImportBinding {
        local_name: "same".into(),
        source_module: path.into(),
        imported_name: Some("shared".into()),
        file_path: "use.py".into(),
        is_namespace: false,
        is_default: false,
    };
    for imports in [
        vec![make("a.py"), make("b.py")],
        vec![make("b.py"), make("a.py")],
    ] {
        let mut c = SymbolCatalog::new();
        c.add_symbols(&a.symbols);
        c.add_symbols(&b.symbols);
        let r = c.resolve_decision(NameRequest {
            name: "same",
            file: "use.py",
            line: 1,
            scopes: &std::collections::HashMap::new(),
            imports: &imports,
            container: None,
            signals: Default::default(),
        });
        assert!(
            matches!(r,NameResolution::Ambiguous{ref candidates,..} if candidates.len()==2),
            "{r:?}"
        );
    }
}
#[test]
fn p2b_large_candidate_pool_stays_explicit_and_shared_go_views_are_bounded() {
    use super::types::{NameRequest, NameResolution};
    let mut c = SymbolCatalog::new();
    for i in 0..300 {
        c.add_symbols(&parsed(&format!("f{i:03}.py"), "def shared(x):\n    return x\n").symbols);
    }
    let r = c.resolve_decision(NameRequest {
        name: "shared",
        file: "use.py",
        line: 1,
        scopes: &std::collections::HashMap::new(),
        imports: &[],
        container: None,
        signals: Default::default(),
    });
    assert!(
        matches!(r,NameResolution::Ambiguous{ref candidates,truncated:true,..} if candidates.is_empty())
    );
    let key = cc_model::package_surface::PackageKey {
        directory: "pkg".into(),
        name: "pkg".into(),
        test_files: false,
    };
    let files: Vec<_> = (0..500).map(|i| format!("pkg/f{i}.go")).collect();
    let groups = std::collections::BTreeMap::from([(key.clone(), files.clone())]);
    c.install_go_groups(&groups, files.iter().map(|p| (p.clone(), key.clone())));
    let first = c.go_packages.get(&files[0]).unwrap();
    assert!(files
        .iter()
        .all(|p| std::sync::Arc::ptr_eq(first, &c.go_packages[p])));
}
#[test]
fn p2b_javascript_route_sites_preserve_distinct_targets() {
    let registry = cc_parsers::ParserRegistry::new();
    let mut units = Vec::new();
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../cc-eval/fixtures/sample-project");
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file() {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            let language = cc_parsers::detect_language(&name);
            if let Ok(text) = std::fs::read_to_string(&path) {
                if let Ok(outcome) = registry.parse(&name, &text, language) {
                    units.push((name, outcome));
                }
            }
        }
    }
    units.sort_by(|a, b| a.0.cmp(&b.0));
    let mut c = SymbolCatalog::new();
    for (_, o) in &units {
        c.add_symbols(&o.symbols);
    }
    let (_, mut outcome) = units.into_iter().find(|(p, _)| p == "routes.js").unwrap();
    c.resolve_outcome("routes.js", &mut outcome);
    let mut seen = std::collections::BTreeMap::new();
    for record in &outcome.resolution.records {
        if let Some(previous) = seen.insert((&record.site_kind, &record.site_id), record) {
            assert_eq!(previous, record, "conflicting raw resolver records");
        }
    }
    outcome.resolution.validate().unwrap();
}
#[test]
fn p2b_type_contributions_never_use_last_writer_or_first_method() {
    let mut a = parsed("a.py", "def invoke(x):\n    return x\n")
        .symbols
        .remove(0);
    let mut b = parsed("b.py", "def invoke(x):\n    return x\n")
        .symbols
        .remove(0);
    a.receiver_type = Some("Client".into());
    b.receiver_type = Some("Client".into());
    for reversed in [false, true] {
        let symbols = if reversed { [&b, &a] } else { [&a, &b] };
        let tc = crate::type_catalog::TypeCatalog::build_from_symbols(symbols);
        assert!(tc.resolve_method_by_receiver("invoke", "Client").is_none());
    }
    a.kind = cc_model::SymbolKind::TypeAlias;
    b.kind = cc_model::SymbolKind::TypeAlias;
    a.name = "Alias".into();
    b.name = "Alias".into();
    a.base_types = Some("First".into());
    b.base_types = Some("Second".into());
    for reversed in [false, true] {
        let symbols = if reversed { [&b, &a] } else { [&a, &b] };
        let tc = crate::type_catalog::TypeCatalog::build_from_symbols(symbols);
        assert_eq!(tc.resolve_alias("alias"), "alias");
    }
}
#[test]
fn p2b_community_labels_ignore_sql_row_order() {
    let edges = vec![
        ("a".into(), "a".into()),
        ("b".into(), "b".into()),
        ("c".into(), "c".into()),
    ];
    let mut reverse = edges.clone();
    reverse.reverse();
    assert_eq!(
        crate::community::louvain_communities(&edges, 100),
        crate::community::louvain_communities(&reverse, 100)
    );
}

#[test]
fn p2b_equal_distance_lookup_is_independent_of_insertion_history() {
    let a = parsed("a/api.py", "def shared(value):\n    return value\n");
    let b = parsed("b/api.py", "def shared(value):\n    return value\n");
    let consumer = parsed("use.py", "def run(value):\n    return shared(value)\n");
    let mut results = Vec::new();
    for reversed in [false, true] {
        let mut c = SymbolCatalog::new();
        for o in if reversed { [&b, &a] } else { [&a, &b] } {
            c.add_symbols(&o.symbols);
        }
        c.add_symbols(&consumer.symbols);
        let mut o = consumer.clone();
        c.resolve_outcome("use.py", &mut o);
        results.push(serde_json::to_value(&o.call_edges).unwrap());
    }
    assert_eq!(
        results[0], results[1],
        "same candidate facts must not change with bucket order"
    );
}
