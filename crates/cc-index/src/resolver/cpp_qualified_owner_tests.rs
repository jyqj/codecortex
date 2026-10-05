//! Owned synthetic records exercise downstream eligibility independently of AST proof.
use super::SymbolCatalog;
use cc_db::index_db::FileEdgesForReresolve;
use cc_model::{
    cpp_owner::CppQualifiedOwnerState as State,
    resolution::{ResolutionOutcome, CPP_QUALIFIED_OWNER_UNPROVEN_BINDING as BLOCKED},
    Language, ParseOutcome, ResolutionKind, SymbolKind, SymbolRecord,
};
use cc_parsers::ParserRegistry;

const FILE: &str = "owned.cpp";
const STATES: [State; 4] = [
    State::ProvenNamespace,
    State::ProvenType,
    State::Unproven,
    State::Ambiguous,
];

fn parse(text: &str) -> ParseOutcome {
    ParserRegistry::new()
        .parse(FILE, text, Language::Cpp)
        .unwrap()
}

fn symbol(state: State) -> SymbolRecord {
    let mut s = parse("int leaf() { return 1; }").symbols.remove(0);
    s.cpp_qualified_owner = state;
    s.kind = if state == State::ProvenType {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };
    s.qname = state.is_proven().then(|| "owner::leaf".into());
    s.symbol_uid = state.is_proven().then(|| format!("uid:{}", state.as_str()));
    s.receiver_type = Some("Owner".into());
    s.param_count = Some(2);
    s
}

fn sites() -> ParseOutcome {
    let mut out = parse("int caller() { return leaf(1, 2); }");
    assert_eq!(out.call_edges.len(), 1);
    assert_eq!(out.symbol_refs.len(), 1);
    // Start with no parser terminal state so generic lookup really executes.
    out.call_edges[0].resolution_strategy.clear();
    out.symbol_refs[0].resolution_strategy.clear();
    out
}

fn assert_blocked(out: &ParseOutcome) {
    for edge in &out.call_edges {
        assert_eq!(edge.target_symbol_id, None);
        assert_eq!(edge.target_file_path, None);
        assert_eq!(edge.callee_symbol_uid, None);
        assert_eq!(edge.resolution_kind, ResolutionKind::Unresolved);
        assert_eq!(edge.resolution_confidence, 0.0);
        assert_eq!(edge.resolution_strategy, BLOCKED);
    }
    for reference in &out.symbol_refs {
        assert_eq!(reference.target_symbol_id, None);
        assert_eq!(reference.target_file_path, None);
        assert_eq!(reference.target_symbol_uid, None);
        assert_eq!(reference.resolution_kind, ResolutionKind::Unresolved);
        assert_eq!(reference.resolution_confidence, 0.0);
        assert_eq!(reference.resolution_strategy, BLOCKED);
    }
    // Fresh parser output also carries an explicit public-surface capability
    // record; dirty reload intentionally lacks that transient surface metadata.
    let has_capability = out
        .public_surface
        .extractor_version
        .starts_with("conservative-");
    assert_eq!(
        out.resolution.records.len(),
        2 + usize::from(has_capability)
    );
    for (kind, id) in [
        ("call", &out.call_edges[0].edge_id),
        ("symbol_ref", &out.symbol_refs[0].ref_id),
    ] {
        let records: Vec<_> = out
            .resolution
            .records
            .iter()
            .filter(|record| record.site_kind == kind && record.site_id == *id)
            .collect();
        assert_eq!(records.len(), 1);
        assert_eq!(
            records[0].outcome,
            ResolutionOutcome::Unresolved {
                reason: BLOCKED.into()
            }
        );
    }
    if has_capability {
        let record = out
            .resolution
            .records
            .iter()
            .find(|record| record.site_kind == "capability")
            .unwrap();
        assert_eq!(record.site_id, "public_surface");
        assert_eq!(
            record.outcome,
            ResolutionOutcome::Unsupported {
                capability: out.public_surface.reasons.join(",")
            }
        );
    }
}

fn reload(out: ParseOutcome) -> ParseOutcome {
    crate::dirty_reload_policy::parse_outcome_from_reloaded_edges(FileEdgesForReresolve {
        symbols: out.symbols,
        imports: out.imports,
        call_edges: out.call_edges,
        symbol_refs: out.symbol_refs,
        semantic_edges: out.semantic_edges,
        dispatch_sites: out.dispatch_sites,
        route_edges: out.route_edges,
    })
}

#[test]
fn cpp_qualified_owner_generic_candidates_are_terminal_negative_evidence() {
    for state in STATES {
        for target_file in [FILE, "other.cpp"] {
            let mut target = symbol(state);
            target.file_path = target_file.into();
            let mut catalog = SymbolCatalog::new();
            catalog.add_symbols(&[target.clone()]);
            catalog.build_type_catalog([&target]);
            assert_eq!(catalog.live_len(), 1, "negative rows stay in the catalog");
            assert_eq!(catalog.entries[0].cpp_qualified_owner, state);
            let mut out = sites();
            catalog.resolve_outcome(FILE, &mut out);
            assert_blocked(&out);
            catalog.resolve_outcome(FILE, &mut out);
            assert_blocked(&out);
        }
    }
}

#[test]
fn cpp_qualified_owner_existing_target_id_or_uid_cannot_skip_rejection() {
    for state in STATES {
        for (with_id, with_uid) in [(true, false), (false, true), (true, true)] {
            let mut target = symbol(state);
            // Deliberately forged durable identity on a rejected row tests state precedence.
            target.symbol_uid = Some(format!("forged:{}", state.as_str()));
            let mut catalog = SymbolCatalog::new();
            catalog.add_symbols(&[target.clone()]);
            let mut out = sites();
            let edge = &mut out.call_edges[0];
            edge.target_symbol_id = with_id.then(|| target.symbol_id.clone());
            edge.callee_symbol_uid = with_uid.then(|| target.symbol_uid.clone().unwrap());
            edge.target_file_path = Some(FILE.into());
            edge.resolution_strategy = "parser_exact".into();
            edge.resolution_kind = ResolutionKind::Exact;
            edge.resolution_confidence = 1.0;
            let reference = &mut out.symbol_refs[0];
            reference.target_symbol_id = edge.target_symbol_id.clone();
            reference.target_symbol_uid = edge.callee_symbol_uid.clone();
            reference.target_file_path = edge.target_file_path.clone();
            reference.resolution_strategy = "parser_exact".into();
            reference.resolution_kind = ResolutionKind::Exact;
            reference.resolution_confidence = 1.0;
            catalog.resolve_outcome(FILE, &mut out);
            assert_blocked(&out);
        }
    }
}

#[test]
fn cpp_qualified_owner_type_catalog_fresh_and_delta_exclude_every_b1_state() {
    for state in STATES {
        let mut target = symbol(state);
        target.symbol_uid = Some(format!("forged:{}", state.as_str()));
        let uid = target.symbol_uid.as_deref().unwrap();
        let mut tc = crate::type_catalog::TypeCatalog::build_from_symbols([&target]);
        assert!(!tc.has_methods());
        assert_eq!(tc.method_param_count("leaf", uid), None);
        tc.add_symbol(&target);
        assert!(!tc.has_methods());
        assert_eq!(tc.method_receiver_compat("leaf", uid, "Owner"), None);

        let mut control = target.clone();
        control.cpp_qualified_owner = State::NonB1;
        control.symbol_uid = Some("control-uid".into());
        tc.add_symbol(&control);
        assert!(tc.has_methods());
        assert_eq!(tc.method_param_count("leaf", "control-uid"), Some(2));
        assert_eq!(
            tc.method_receiver_compat("leaf", "control-uid", "Owner"),
            Some(true)
        );
    }
}

#[test]
fn cpp_qualified_owner_terminal_type_fallback_cannot_bind_valid_neighbor() {
    let mut control = symbol(State::NonB1);
    control.symbol_uid = Some("valid-uid".into());
    control.qname = Some("Other.leaf".into());
    let mut other = control.clone();
    other.symbol_id = "other-id".into();
    other.symbol_uid = Some("other-uid".into());
    other.receiver_type = Some("Different".into());
    other.param_count = Some(1);
    other.qname = Some("Different.leaf".into());
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&[control.clone(), other.clone()]);
    catalog.build_type_catalog([&control, &other]);
    assert_eq!(
        catalog
            .type_catalog
            .as_ref()
            .unwrap()
            .resolve_method_by_arg_count("leaf", 2),
        Some("valid-uid")
    );
    let mut out = sites();
    out.call_edges[0].resolution_strategy = BLOCKED.into();
    out.call_edges[0].receiver_expr = Some("Owner".into());
    out.symbol_refs[0].resolution_strategy = BLOCKED.into();
    catalog.resolve_outcome(FILE, &mut out);
    assert_blocked(&out);
}

#[test]
fn cpp_qualified_owner_rejected_caller_id_never_refills_from_same_name_neighbor() {
    for state in [State::Unproven, State::Ambiguous] {
        for reverse in [false, true] {
            let mut rejected = symbol(state);
            rejected.name = "caller".into();
            let mut neighbor = rejected.clone();
            neighbor.cpp_qualified_owner = State::NonB1;
            neighbor.symbol_id = "neighbor-id".into();
            neighbor.symbol_uid = Some("neighbor-uid".into());
            neighbor.qname = Some("caller".into());
            let mut symbols = vec![neighbor, rejected.clone()];
            if reverse {
                symbols.reverse();
            }
            let mut catalog = SymbolCatalog::new();
            catalog.add_symbols(&symbols);
            let mut out = sites();
            out.call_edges[0].caller_symbol_id = Some(rejected.symbol_id.clone());
            out.call_edges[0].caller_symbol_uid = None;
            catalog.resolve_outcome(FILE, &mut out);
            assert_eq!(
                out.call_edges[0].caller_symbol_id,
                Some(rejected.symbol_id.clone())
            );
            assert_eq!(out.call_edges[0].caller_symbol_uid, None);
            // Even an accidentally forged neighbor UID cannot override the exact rejected ID.
            out.call_edges[0].caller_symbol_uid = Some("neighbor-uid".into());
            catalog.resolve_outcome(FILE, &mut out);
            assert_eq!(out.call_edges[0].caller_symbol_id, Some(rejected.symbol_id));
            assert_eq!(out.call_edges[0].caller_symbol_uid, None);
        }
    }
}

#[test]
fn cpp_qualified_owner_proven_caller_keeps_only_existing_exact_uid() {
    for state in [State::ProvenNamespace, State::ProvenType] {
        let mut caller = symbol(state);
        caller.name = "caller".into();
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&[caller.clone()]);
        for existing in [caller.symbol_uid.clone(), None, Some("wrong-uid".into())] {
            let mut out = sites();
            out.call_edges[0].caller_symbol_id = Some(caller.symbol_id.clone());
            out.call_edges[0].caller_symbol_uid = existing.clone();
            catalog.resolve_outcome(FILE, &mut out);
            let expected = if existing == caller.symbol_uid {
                existing
            } else {
                None
            };
            assert_eq!(out.call_edges[0].caller_symbol_uid, expected);
            assert_eq!(
                out.call_edges[0].caller_symbol_id,
                Some(caller.symbol_id.clone())
            );
        }
    }
}

#[test]
fn cpp_qualified_owner_dirty_parser_exact_retention_rejects_tagged_targets() {
    for state in STATES {
        let mut out = parse("int leaf() { return 1; } int caller() { return leaf(); }");
        let target = out.symbols.iter_mut().find(|s| s.name == "leaf").unwrap();
        target.cpp_qualified_owner = state;
        let mut catalog = SymbolCatalog::new();
        catalog.add_symbols(&out.symbols);
        assert_eq!(out.call_edges[0].resolution_strategy, "parser_exact");
        assert!(out.call_edges[0].callee_symbol_uid.is_some());
        let mut loaded = reload(out);
        assert_eq!(loaded.symbols[0].cpp_qualified_owner, state);
        assert!(
            loaded.cpp_qualified_owner_proofs.is_empty(),
            "durable rejection does not need transient AST proof"
        );
        assert_eq!(loaded.call_edges[0].resolution_strategy, BLOCKED);
        assert_eq!(loaded.symbol_refs[0].resolution_strategy, BLOCKED);
        catalog.resolve_outcome(FILE, &mut loaded);
        assert_blocked(&loaded);
        let mut loaded_again = reload(loaded);
        catalog.resolve_outcome(FILE, &mut loaded_again);
        assert_blocked(&loaded_again);
    }
}

#[test]
fn cpp_qualified_owner_dirty_retains_ordinary_parser_exact_and_namespace_terminal() {
    let out = parse("int leaf() { return 1; } int caller() { return leaf(); }");
    let uid = out.call_edges[0].callee_symbol_uid.clone();
    let loaded = reload(out);
    assert_eq!(loaded.call_edges[0].callee_symbol_uid, uid);
    assert_eq!(loaded.call_edges[0].resolution_strategy, "parser_exact");
    let mut out = sites();
    let old = cc_model::resolution::CPP_NAMESPACE_UNPROVEN_BINDING;
    out.call_edges[0].resolution_strategy = old.into();
    out.symbol_refs[0].resolution_strategy = old.into();
    let mut loaded = reload(out);
    SymbolCatalog::new().resolve_outcome(FILE, &mut loaded);
    assert_eq!(loaded.call_edges[0].resolution_strategy, old);
    assert_eq!(loaded.symbol_refs[0].resolution_strategy, old);
}

#[test]
fn cpp_qualified_owner_hierarchy_abstains_even_with_forged_short_container() {
    for state in STATES {
        let mut target = symbol(state);
        let mut owner = parse("struct Owner { int inline_member() { return 0; } };").symbols;
        for container in [None, Some("Owner".into())] {
            target.container = container;
            let mut symbols = owner.clone();
            symbols.push(target.clone());
            let edges = crate::hierarchy::generate_hierarchy_edges(&symbols, &[]);
            assert!(!edges.iter().any(|edge| edge.target_symbol == "leaf"));
            assert!(edges
                .iter()
                .any(|edge| edge.target_symbol == "inline_member"));
            assert!(edges.iter().any(|edge| edge.target_symbol == "Owner"));
        }
        owner.clear();
    }
}

#[test]
fn cpp_qualified_owner_catalog_remove_readd_does_not_leave_stale_id_guard() {
    let target = symbol(State::ProvenType);
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&[target.clone()]);
    catalog.build_type_catalog([&target]);
    assert!(catalog.cpp_qualified_by_id.contains_key(&target.symbol_id));
    catalog.remove_files(&[FILE.into()].into());
    assert!(!catalog.cpp_qualified_by_id.contains_key(&target.symbol_id));
    let mut ordinary = target;
    ordinary.cpp_qualified_owner = State::NonB1;
    ordinary.qname = Some("leaf".into());
    catalog.add_symbols(&[ordinary.clone()]);
    catalog.type_catalog_add_symbols([&ordinary]);
    let mut out = sites();
    catalog.resolve_outcome(FILE, &mut out);
    assert_eq!(out.call_edges[0].target_symbol_id, Some(ordinary.symbol_id));
    assert_eq!(out.call_edges[0].callee_symbol_uid, ordinary.symbol_uid);
}

#[test]
fn cpp_qualified_owner_project_model_preserves_terminal_reason_before_dirty_reload() {
    // Deliberately force a competing module-blocked reason on owned synthetic
    // input. The terminal B1 rejection must win across this shared stage.
    let root = tempfile::tempdir().unwrap();
    let text = "import { leaf } from './missing'; function caller() { leaf(); }";
    std::fs::write(root.path().join("caller.ts"), text).unwrap();
    let project = crate::project_model::discover(
        root.path(),
        ["caller.ts".into()].into(),
        None,
        None,
        &Default::default(),
    )
    .unwrap();
    let mut out = ParserRegistry::new()
        .parse("caller.ts", text, Language::TypeScript)
        .unwrap();
    assert!(!out.call_edges.is_empty());
    assert!(!out.symbol_refs.is_empty());
    for edge in &mut out.call_edges {
        edge.resolution_strategy = BLOCKED.into();
    }
    for reference in &mut out.symbol_refs {
        reference.resolution_strategy = BLOCKED.into();
    }
    project.apply_imports("caller.ts", &mut out);
    let loaded = reload(out);
    assert!(loaded
        .call_edges
        .iter()
        .all(|edge| edge.resolution_strategy == BLOCKED));
    assert!(loaded
        .symbol_refs
        .iter()
        .all(|reference| reference.resolution_strategy == BLOCKED));
}

#[test]
fn cpp_qualified_owner_mixed_generic_bucket_stays_terminal_after_b1_target_removal() {
    for state in STATES {
        for same_file in [false, true] {
            let mut tagged = symbol(state);
            tagged.file_path = if same_file { FILE } else { "tagged.cpp" }.into();
            tagged.symbol_id = "tagged-id".into();
            let mut ordinary = symbol(State::NonB1);
            ordinary.file_path = if same_file { FILE } else { "ordinary.cpp" }.into();
            ordinary.symbol_id = "ordinary-id".into();
            ordinary.symbol_uid = Some("ordinary-uid".into());
            ordinary.qname = Some("leaf".into());
            let mut catalog = SymbolCatalog::new();
            catalog.add_symbols(&[ordinary.clone(), tagged.clone()]);
            let mut out = sites();
            catalog.resolve_outcome(FILE, &mut out);
            assert_blocked(&out);
            catalog.remove_files(&[tagged.file_path.clone()].into());
            if same_file {
                catalog.add_symbols(&[ordinary.clone()]);
            }
            let mut loaded = reload(out);
            catalog.resolve_outcome(FILE, &mut loaded);
            assert_blocked(&loaded);
            // Prove a fresh unblocked site would bind the ordinary neighbor:
            // the persisted terminal reason, rather than mere lack of targets,
            // is what stops an unchanged dependent file rebinding after edits.
            let mut fresh = sites();
            catalog.resolve_outcome(FILE, &mut fresh);
            assert_eq!(fresh.call_edges[0].callee_symbol_uid, ordinary.symbol_uid);
        }
    }
}

#[test]
fn cpp_qualified_owner_mixed_buckets_do_not_demote_existing_non_b1_parser_exact() {
    let mut out = parse("int leaf() { return 1; } int caller() { return leaf(); }");
    let uid = out.call_edges[0].callee_symbol_uid.clone();
    let mut tagged = symbol(State::ProvenNamespace);
    tagged.symbol_id = "tagged-id".into();
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&out.symbols);
    catalog.add_symbols(&[tagged]);
    catalog.resolve_outcome(FILE, &mut out);
    assert_eq!(out.call_edges[0].callee_symbol_uid, uid);
    assert_eq!(out.symbol_refs[0].target_symbol_uid, uid);
    assert_eq!(out.call_edges[0].resolution_strategy, "parser_exact");
    assert_eq!(out.symbol_refs[0].resolution_strategy, "parser_exact");
}

#[test]
fn cpp_qualified_owner_negative_evidence_precedes_candidate_truncation_and_budgets() {
    let mut catalog = SymbolCatalog::new();
    let mut tagged = symbol(State::Unproven);
    tagged.symbol_id = "zzz-tagged".into();
    tagged.file_path = "tagged.cpp".into();
    for i in 0..=cc_model::resolution::MAX_RESOLUTION_CANDIDATES {
        let mut ordinary = symbol(State::NonB1);
        ordinary.symbol_id = format!("ordinary-{i}");
        ordinary.symbol_uid = Some(format!("ordinary-{i}"));
        ordinary.file_path = format!("ordinary-{i}.cpp");
        catalog.add_symbols(&[ordinary]);
    }
    catalog.add_symbols(&[tagged]);
    let all: Vec<_> = (0..catalog.entries.len()).collect();
    assert!(matches!(
        catalog.ambiguous(&all, "synthetic_tie"),
        super::types::NameResolution::Unresolved(BLOCKED)
    ));
    catalog.max_fuzzy_pool = 2;
    let mut out = sites();
    catalog.resolve_outcome(FILE, &mut out);
    assert_blocked(&out);
}

#[test]
fn cpp_qualified_owner_infra_name_prefix_and_contains_abstain_without_promotion() {
    for state in STATES {
        for name in ["leaf", "leaf_service", "big_leaf_handler"] {
            for reverse in [false, true] {
                let mut tagged = symbol(state);
                tagged.name = name.into();
                let mut ordinary = tagged.clone();
                ordinary.cpp_qualified_owner = State::NonB1;
                ordinary.symbol_uid = Some("ordinary-uid".into());
                let mut symbols = vec![tagged, ordinary.clone()];
                if reverse {
                    symbols.reverse();
                }
                let (mut nodes, _) = crate::infra_pass::parse_docker_compose(
                    "docker-compose.yml",
                    "services:\n  leaf:\n    image: owned:latest\n",
                );
                assert_eq!(nodes.len(), 2);
                assert_eq!(nodes[0].kind, cc_model::infra::InfraKind::ComposeService);
                assert_eq!(nodes[0].name, "leaf");
                assert_eq!(nodes[1].kind, cc_model::infra::InfraKind::DockerImage);
                crate::infra_pass::bind_infra_to_symbols(&mut nodes, &[ordinary]);
                assert_eq!(nodes[0].bound_symbol_uid.as_deref(), Some("ordinary-uid"));
                crate::infra_pass::bind_infra_to_symbols(&mut nodes, &symbols);
                assert_eq!(nodes[0].bound_symbol_uid, None);
                assert_eq!(nodes[0].binding_confidence, None);
                assert_eq!(nodes[1].bound_symbol_uid, None);
                assert_eq!(nodes[1].binding_confidence, None);
            }
        }
    }
}

#[test]
fn cpp_qualified_owner_framework_all_name_lookup_does_not_promote_local_neighbor() {
    let mut tagged = symbol(State::ProvenNamespace);
    tagged.symbol_id = "tagged-id".into();
    let mut ordinary = tagged.clone();
    ordinary.cpp_qualified_owner = State::NonB1;
    ordinary.symbol_id = "ordinary-id".into();
    ordinary.symbol_uid = Some("ordinary-uid".into());
    let mut remote = ordinary.clone();
    remote.file_path = "remote.ts".into();
    remote.symbol_id = "remote-id".into();
    remote.symbol_uid = Some("remote-uid".into());
    let mut catalog = SymbolCatalog::new();
    catalog.add_symbols(&[ordinary, tagged, remote]);
    assert_eq!(
        catalog.lookup_all_by_name("leaf"),
        vec![(
            "remote-uid".into(),
            "remote.ts".into(),
            SymbolKind::Function
        )]
    );
}
