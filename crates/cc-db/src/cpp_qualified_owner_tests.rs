//! Tiny owned fixtures for durable B1 eligibility and source-bound admission.
use crate::{
    index_db::{FileWriteUnit, IndexDb, PrecompressedChunks},
    symbol_identity_store::{self, Projection},
};
use cc_model::{
    cpp_owner::{CppQualifiedOwnerProof, CppQualifiedOwnerState as State},
    id::StableId,
    identity::{DocumentBatch, DocumentRecord},
    source::{
        BoundaryKind, ByteSpan, ChunkSource, SourceSnapshot, SourceStructure, SyntaxBoundary,
    },
    symbol_identity::{ChunkSymbolIdentity, SYMBOL_IDENTITY_FORMAT},
    ChunkRecord, Language, ParseOutcome, ParserTier, SymbolKind, SymbolRecord,
};

const STATES: [State; 5] = [
    State::NonB1,
    State::ProvenNamespace,
    State::ProvenType,
    State::Unproven,
    State::Ambiguous,
];

fn symbol(state: State, index: usize) -> SymbolRecord {
    let name = format!("leaf{index}");
    let kind = if state == State::ProvenType {
        SymbolKind::Method
    } else {
        SymbolKind::Function
    };
    let qname = state.is_proven().then(|| format!("grove::{name}"));
    let uid = qname
        .as_deref()
        .map(|q| StableId::symbol_uid("a.cpp", q, kind.as_str(), None));
    SymbolRecord {
        cpp_qualified_owner: state,
        symbol_id: format!("symbol{index}"),
        file_path: "a.cpp".into(),
        name,
        kind,
        container: None,
        start_line: index as u32 + 1,
        end_line: index as u32 + 1,
        start_col: 0,
        end_col: 1,
        signature: None,
        doc: None,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
        qname,
        parent_symbol_id: None,
        scope_id: None,
        export_name: None,
        is_default_export: false,
        symbol_uid: uid,
        framework_role: None,
        receiver_type: None,
        param_types: None,
        return_type: None,
        param_count: None,
        base_types: None,
        implements: None,
    }
}

fn unit(symbols: Vec<SymbolRecord>) -> FileWriteUnit {
    FileWriteUnit {
        rel_path: "a.cpp".into(),
        language: Language::Cpp,
        content_hash: "synthetic".into(),
        mtime: 1.0,
        size: 1,
        outcome: ParseOutcome {
            symbols,
            ..Default::default()
        },
    }
}

fn batch(
    db: &IndexDb,
    normal: &[FileWriteUnit],
    dirty: &[FileWriteUnit],
) -> cc_model::CcResult<()> {
    db.writes()
        .write_incremental_batch(&[], normal, dirty, &[], &[], &PrecompressedChunks::new())
        .map(|_| ())
}

fn states(rows: &[SymbolRecord]) -> Vec<State> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by_key(|row| &row.symbol_id);
    rows.iter().map(|row| row.cpp_qualified_owner).collect()
}

fn row_states(rows: &[crate::index_db::SymbolRow]) -> Vec<State> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by_key(|row| &row.symbol_id);
    rows.iter().map(|row| row.cpp_qualified_owner).collect()
}

fn assert_projections(db: &IndexDb) {
    let conn = db.read_conn().unwrap();
    assert_eq!(
        db.reads()
            .list_symbol_targets()
            .unwrap()
            .into_iter()
            .map(|row| row.cpp_qualified_owner)
            .collect::<Vec<_>>(),
        STATES
    );
    assert_eq!(
        row_states(&db.reads().file_symbols("a.cpp").unwrap()),
        STATES
    );
    for query in ["leaf", "le"] {
        assert_eq!(
            row_states(&db.reads().find_symbol(query, false, 10).unwrap()),
            STATES
        );
    }
    let kinds = &["function", "method"];
    let names = ["leaf0", "leaf1", "leaf2", "leaf3", "leaf4"];
    for (i, name) in names.iter().enumerate() {
        assert_eq!(
            db.reads().find_symbol(name, true, 10).unwrap()[0].cpp_qualified_owner,
            STATES[i]
        );
        assert_eq!(
            db.reads()
                .find_symbols_by_name_and_kinds(name, kinds)
                .unwrap()[0]
                .cpp_qualified_owner,
            STATES[i]
        );
    }
    let grouped = db
        .reads()
        .find_symbols_by_names_and_kinds(&names, kinds)
        .unwrap();
    assert_eq!(
        names
            .iter()
            .map(|name| grouped[*name][0].cpp_qualified_owner)
            .collect::<Vec<_>>(),
        STATES
    );
    assert_eq!(
        row_states(
            &db.retrieval()
                .scoped_symbol_rows("leaf", false, &crate::ChunkScope::default(), 10)
                .unwrap()
        ),
        STATES
    );
    assert_eq!(
        row_states(&db.reads().symbols_by_file_paths(&["a.cpp"]).unwrap()),
        [State::ProvenNamespace, State::ProvenType]
    );
    let uids = db
        .reads()
        .file_symbols("a.cpp")
        .unwrap()
        .into_iter()
        .filter_map(|row| row.symbol_uid)
        .collect::<Vec<_>>();
    assert_eq!(
        row_states(
            &db.reads()
                .symbol_rows_by_uids(&uids)
                .unwrap()
                .into_values()
                .collect::<Vec<_>>()
        ),
        [State::ProvenNamespace, State::ProvenType]
    );
    assert_eq!(
        states(&IndexDb::load_seed_rows_on(&conn, &[]).unwrap()),
        STATES
    );
    assert_eq!(
        states(&db.reads().resolver_seed_symbols_excluding(&[]).unwrap()),
        STATES
    );
    assert_eq!(
        states(
            &db.reads()
                .load_file_edges_for_reresolve("a.cpp")
                .unwrap()
                .symbols
        ),
        STATES
    );
    assert_eq!(
        states(&db.retrieval().symbol_records_for_infra_binding().unwrap()),
        [State::ProvenNamespace, State::ProvenType]
    );
}

#[test]
fn cpp_owner_all_states_survive_scalar_batch_dirty_and_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("index.db");
    let db = IndexDb::open(&path).unwrap().0;
    let file = unit(
        STATES
            .into_iter()
            .enumerate()
            .map(|(i, s)| symbol(s, i))
            .collect(),
    );
    db.writes()
        .replace_files_batch(std::slice::from_ref(&file))
        .unwrap();
    assert_projections(&db);
    batch(&db, std::slice::from_ref(&file), &[]).unwrap();
    assert_projections(&db);
    // Dirty reload carries durable eligibility without needing transient proofs.
    let mut dirty = file.clone();
    dirty.outcome.symbols = db
        .reads()
        .load_file_edges_for_reresolve("a.cpp")
        .unwrap()
        .symbols;
    db.writes()
        .replace_reresolved_edges_only(std::slice::from_ref(&dirty))
        .unwrap();
    assert_projections(&db);
    batch(&db, &[], &[dirty]).unwrap();
    assert_projections(&db);
    drop(db);
    assert_projections(&IndexDb::open(&path).unwrap().0);
}

#[test]
fn cpp_owner_eligibility_only_changes_seed_token_and_cached_projection() {
    let temp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
    batch(&db, &[], &[]).unwrap();
    let mut file = unit(vec![symbol(State::Unproven, 0)]);
    batch(&db, std::slice::from_ref(&file), &[]).unwrap();
    let (before, rows) = db
        .reads()
        .resolver_seed_symbols_with_token_excluding(&[])
        .unwrap();
    assert!(before.is_some());
    assert_eq!(
        rows.iter().next().unwrap().cpp_qualified_owner,
        State::Unproven
    );
    let old_candidates = db
        .reads()
        .resolution_symbols_in_files(&["a.cpp".into()])
        .unwrap();
    file.outcome.symbols[0].cpp_qualified_owner = State::Ambiguous;
    batch(&db, &[], std::slice::from_ref(&file)).unwrap();
    let (after, rows) = db
        .reads()
        .resolver_seed_symbols_with_token_excluding(&[])
        .unwrap();
    assert_ne!(before, after);
    let new_candidates = db
        .reads()
        .resolution_symbols_in_files(&["a.cpp".into()])
        .unwrap();
    assert_ne!(
        old_candidates, new_candidates,
        "name-bucket evidence changes on eligibility alone"
    );
    assert_eq!(
        new_candidates,
        vec![cc_model::resolution::ResolutionSymbol::from(
            &file.outcome.symbols[0]
        )]
    );
    assert_eq!(
        rows.iter().next().unwrap().cpp_qualified_owner,
        State::Ambiguous
    );
    assert_eq!(
        serde_json::to_value(rows.into_vec()).unwrap(),
        serde_json::to_value(IndexDb::load_seed_rows_on(&db.read_conn().unwrap(), &[]).unwrap())
            .unwrap()
    );
    let conn = db.read_conn().unwrap();
    assert_eq!(
        crate::signature_agg::load_on(&conn).unwrap().unwrap(),
        crate::signature_agg::scan_on(&conn).unwrap()
    );
}

#[test]
fn cpp_owner_sql_default_check_and_malformed_reads_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
    db.writes().replace_files_batch(&[unit(vec![])]).unwrap();
    let conn = db.write_conn.lock().unwrap();
    conn.execute("INSERT INTO symbols(symbol_id,file_path,name,kind,start_line,end_line,symbol_uid) VALUES('legacy','a.cpp','leaf','function',1,1,'legacy-uid')", []).unwrap();
    assert_eq!(
        IndexDb::load_seed_rows_on(&conn, &[]).unwrap()[0].cpp_qualified_owner,
        State::NonB1
    );
    let row = symbol(State::NonB1, 0);
    let json = serde_json::to_value(&row).unwrap();
    assert!(json.get("cpp_qualified_owner").is_none());
    assert_eq!(
        serde_json::from_value::<SymbolRecord>(json)
            .unwrap()
            .cpp_qualified_owner,
        State::NonB1
    );
    for raw in ["future_positive", "", "PROVEN_NAMESPACE"] {
        assert!(conn
            .execute("UPDATE symbols SET cpp_qualified_owner=?1", [raw])
            .is_err());
    }
    assert!(conn
        .execute("UPDATE symbols SET cpp_qualified_owner=NULL", [])
        .is_err());
    conn.pragma_update(None, "ignore_check_constraints", true)
        .unwrap();
    conn.execute(
        "UPDATE symbols SET cpp_qualified_owner='future_positive'",
        [],
    )
    .unwrap();
    assert!(IndexDb::load_seed_rows_on(&conn, &[]).is_err());
    assert!(crate::signature_agg::scan_on(&conn).is_err());
    drop(conn);
    assert!(db.reads().load_file_edges_for_reresolve("a.cpp").is_err());
    assert!(db.retrieval().symbol_records_for_infra_binding().is_err());
    assert!(db.reads().file_symbols("a.cpp").is_err());
    assert!(db.reads().list_symbol_targets().is_err());
    assert!(db.reads().find_symbol("leaf", true, 10).is_err());
    assert!(db.symbol_graph_reads().symbol_dispatch_rows().is_err());
    assert!(db
        .reads()
        .find_method_in_same_class("legacy-uid", "render")
        .is_err());
    assert!(db
        .reads()
        .resolution_symbols_in_files(&["a.cpp".into()])
        .is_err());
}

fn identity_unit(state: State) -> FileWriteUnit {
    let (prefix, qualified, owner_path, declaration) = match state {
        State::ProvenNamespace => (
            "namespace grove {}\n",
            "grove::leaf",
            vec!["grove".into()],
            "namespace grove {}",
        ),
        State::ProvenType => (
            "namespace grove { struct Box {}; }\n",
            "grove::Box::leaf",
            vec!["grove".into(), "Box".into()],
            "struct Box {}",
        ),
        State::NonB1 => ("", "leaf", vec![], ""),
        _ => panic!("identity fixture requires an admissible state"),
    };
    let text = format!("{prefix}int {qualified}() {{ return 1; }}\n");
    let source = SourceSnapshot::new(text.as_bytes());
    let definition = ByteSpan::new(prefix.len(), text.len() - 1).unwrap();
    let owner_start = text.find(declaration).unwrap();
    let signature = ByteSpan::new(definition.start, text.find("() {").unwrap() + 2).unwrap();
    let mut sym = symbol(state, 0);
    sym.name = "leaf".into();
    sym.qname = Some(qualified.into());
    sym.signature = Some(source.slice(signature).unwrap().into());
    sym.symbol_uid = Some(StableId::symbol_uid(
        "a.cpp",
        qualified,
        sym.kind.as_str(),
        sym.signature.as_deref(),
    ));
    let start = source.point(definition.start).unwrap();
    let end = source.point(definition.end).unwrap();
    sym.start_line = start.0 as u32;
    sym.start_col = start.1 as u32;
    sym.end_line = end.0 as u32;
    sym.end_col = end.1 as u32;
    let chunk_source = ChunkSource {
        source: source.identity().clone(),
        span: definition,
        slice_digest: source.slice_digest(definition).unwrap(),
        boundary: "symbol".into(),
        owner: Some(definition),
        signature: Some(signature),
    };
    let chunk = ChunkRecord {
        source: Some(chunk_source.clone()),
        chunk_id: "chunk0".into(),
        file_path: "a.cpp".into(),
        language: Language::Cpp,
        chunk_index: 0,
        start_line: sym.start_line,
        end_line: sym.end_line,
        breadcrumb: "leaf".into(),
        text: source.slice(definition).unwrap().into(),
        symbol_name: Some(sym.name.clone()),
        symbol_kind: Some(sym.kind),
        token_estimate: 8,
        parser_tier: ParserTier::TreeSitter,
        parser_confidence: 1.0,
    };
    let doc = DocumentRecord::new(
        &chunk,
        &"a".repeat(64),
        0,
        "test-encoding",
        Err("synthetic fixture".into()),
    )
    .unwrap();
    let identity = ChunkSymbolIdentity {
        format_version: SYMBOL_IDENTITY_FORMAT,
        chunk_id: chunk.chunk_id.clone(),
        file_path: "a.cpp".into(),
        document: doc.reference.clone(),
        source: source.identity().clone(),
        owner: definition,
        start_line: sym.start_line,
        start_col: sym.start_col,
        end_line: sym.end_line,
        end_col: sym.end_col,
        symbol_id: sym.symbol_id.clone(),
        symbol_uid: sym.symbol_uid.clone().unwrap(),
        name: sym.name.clone(),
        kind: sym.kind,
        qname: qualified.into(),
    };
    let proofs = if state.is_b1() {
        vec![CppQualifiedOwnerProof {
            source: source.identity().clone(),
            definition,
            owner_declaration: ByteSpan::new(owner_start, owner_start + declaration.len()).unwrap(),
            owner_path,
            state,
            symbol_id: sym.symbol_id.clone(),
            qname: qualified.into(),
            symbol_uid: sym.symbol_uid.clone().unwrap(),
        }]
    } else {
        vec![]
    };
    FileWriteUnit {
        rel_path: "a.cpp".into(),
        language: Language::Cpp,
        content_hash: source.identity().content_digest.clone(),
        mtime: 1.0,
        size: text.len() as u64,
        outcome: ParseOutcome {
            symbols: vec![sym.clone()],
            chunks: vec![chunk],
            documents: Some(DocumentBatch {
                records: vec![doc],
                ..Default::default()
            }),
            chunk_policy: Some("a".repeat(64)),
            document_spec: Some(cc_model::identity::hash(&"test-encoding").unwrap()),
            symbol_identities: vec![identity],
            cpp_qualified_owner_proofs: proofs,
            source_structure: Some(SourceStructure {
                source: source.identity().clone(),
                capability: "existing_tree_sitter_ast".into(),
                complete: true,
                reasons: vec![],
                visited_nodes: 1,
                boundaries: vec![SyntaxBoundary {
                    span: definition,
                    parent: None,
                    kind: BoundaryKind::Symbol,
                    name: Some(sym.name),
                    symbol_kind: Some(sym.kind),
                    signature: Some(signature),
                    leading_comment: None,
                    documentation: None,
                }],
            }),
            ..Default::default()
        },
    }
}

fn load_identity(db: &IndexDb, file: &FileWriteUnit) -> cc_model::CcResult<Option<String>> {
    let chunk = &file.outcome.chunks[0];
    symbol_identity_store::load_on(
        &db.read_conn().unwrap(),
        Projection {
            chunk_id: &chunk.chunk_id,
            path: &file.rel_path,
            document: Some(&file.outcome.symbol_identities[0].document),
            proof: chunk.source.as_ref(),
            name: chunk.symbol_name.as_deref(),
            kind: chunk.symbol_kind.map(|kind| kind.as_str()),
        },
        None,
    )
}

#[test]
fn cpp_owner_proven_and_non_b1_identity_roundtrip() {
    for state in [State::NonB1, State::ProvenNamespace, State::ProvenType] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("index.db");
        let db = IndexDb::open(&path).unwrap().0;
        let file = identity_unit(state);
        db.writes()
            .replace_files_batch(std::slice::from_ref(&file))
            .unwrap();
        assert_eq!(
            load_identity(&db, &file).unwrap(),
            file.outcome.symbols[0].qname
        );
        batch(&db, std::slice::from_ref(&file), &[]).unwrap();
        let mut dirty = unit(
            db.reads()
                .load_file_edges_for_reresolve("a.cpp")
                .unwrap()
                .symbols,
        );
        dirty.content_hash = file.content_hash.clone();
        batch(&db, &[], &[dirty]).unwrap();
        assert_eq!(
            load_identity(&db, &file).unwrap(),
            file.outcome.symbols[0].qname
        );
        drop(db);
        assert_eq!(
            load_identity(&IndexDb::open(&path).unwrap().0, &file).unwrap(),
            file.outcome.symbols[0].qname
        );
    }
}

#[test]
fn cpp_owner_missing_or_false_transient_proof_rolls_back_both_writers() {
    let base = identity_unit(State::ProvenNamespace);
    let mutations: Vec<(&str, fn(&mut FileWriteUnit))> = vec![
        ("missing proof", |f| {
            f.outcome.cpp_qualified_owner_proofs.clear()
        }),
        ("duplicate proof", |f| {
            f.outcome
                .cpp_qualified_owner_proofs
                .push(f.outcome.cpp_qualified_owner_proofs[0].clone())
        }),
        ("stripped state", |f| {
            f.outcome.symbols[0].cpp_qualified_owner = State::NonB1
        }),
        ("rejected state", |f| {
            f.outcome.symbols[0].cpp_qualified_owner = State::Unproven
        }),
        ("proof state", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].state = State::ProvenType
        }),
        ("proof source", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].source.snapshot_id = "0".repeat(64)
        }),
        ("definition span", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].definition.start += 1
        }),
        ("owner span", |f| {
            f.outcome.cpp_qualified_owner_proofs[0]
                .owner_declaration
                .end = usize::MAX
        }),
        ("owner path", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].owner_path[0] = "other".into()
        }),
        ("symbol id", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].symbol_id = "other".into()
        }),
        ("qname", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].qname = "other::leaf".into()
        }),
        ("uid", |f| {
            f.outcome.cpp_qualified_owner_proofs[0].symbol_uid = "wrong".into()
        }),
        ("signature uid", |f| {
            f.outcome.symbols[0].signature = Some("different".into())
        }),
    ];
    for (label, mutate) in mutations {
        for multi in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
            db.writes()
                .replace_files_batch(std::slice::from_ref(&base))
                .unwrap();
            let before = db.reads().generation().unwrap();
            let mut bad = base.clone();
            mutate(&mut bad);
            let result = if multi {
                batch(&db, &[bad], &[])
            } else {
                db.writes().replace_files_batch(&[bad])
            };
            assert!(result.is_err(), "{label}, multi={multi}");
            assert_eq!(db.reads().generation().unwrap(), before, "{label}");
            assert_eq!(
                load_identity(&db, &base).unwrap(),
                base.outcome.symbols[0].qname,
                "{label}"
            );
        }
    }
}

#[test]
fn cpp_owner_persisted_rejected_invalid_and_inconsistent_identity_fail_closed() {
    let file = identity_unit(State::ProvenNamespace);
    let mutations = [
        "UPDATE symbols SET cpp_qualified_owner='unproven'",
        "UPDATE symbols SET cpp_qualified_owner='ambiguous'",
        "UPDATE symbols SET cpp_qualified_owner='proven_type'",
        "UPDATE symbols SET cpp_qualified_owner='future_positive'",
        "UPDATE symbols SET signature='wrong'",
        "UPDATE symbols SET container='grove'",
        "UPDATE symbols SET symbol_uid='wrong'",
        "UPDATE symbols SET qname='other::leaf'",
    ];
    for sql in mutations {
        let temp = tempfile::tempdir().unwrap();
        let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
        db.writes()
            .replace_files_batch(std::slice::from_ref(&file))
            .unwrap();
        {
            let conn = db.write_conn.lock().unwrap();
            conn.pragma_update(None, "ignore_check_constraints", true)
                .unwrap();
            conn.execute(sql, []).unwrap();
        }
        assert!(load_identity(&db, &file).is_err(), "{sql}");
    }
}

#[test]
fn cpp_owner_dispatch_gate_hashes_eligibility_and_excludes_only_b1() {
    use std::collections::HashSet;
    use std::hash::{Hash, Hasher};
    let temp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
    let mut file = unit(vec![symbol(State::NonB1, 0)]);
    // Retain the UID through rejected-state mutations to prove a persisted
    // poisoned target cannot regain generic binding merely by carrying a UID.
    file.outcome.symbols[0].symbol_uid = Some("uid-fixed".into());
    let mut hashes = HashSet::new();
    for state in STATES {
        file.outcome.symbols[0].cpp_qualified_owner = state;
        batch(&db, std::slice::from_ref(&file), &[]).unwrap();
        let agg = db
            .reads()
            .stored_graph_signature_aggregates()
            .unwrap()
            .unwrap();
        hashes.insert(agg.symbols_full.sum);
        assert_eq!(agg.symbols_full.count, 1);
        let dispatch = db.symbol_graph_reads().symbol_dispatch_rows().unwrap();
        assert_eq!(dispatch.len(), usize::from(state.is_non_b1()));
        let public = db.reads().find_symbol("leaf0", true, 10).unwrap();
        assert_eq!(public.len(), 1, "B1 remains publicly discoverable");
        assert_eq!(public[0].cpp_qualified_owner, state);
        if state.is_non_b1() {
            assert!(serde_json::to_value(&public[0])
                .unwrap()
                .get("cpp_qualified_owner")
                .is_none());
            let mut historical = std::collections::hash_map::DefaultHasher::new();
            for text in ["uid-fixed", "leaf0", "function", ""] {
                text.hash(&mut historical);
            }
            assert_eq!(
                agg.symbols_full.sum,
                historical.finish(),
                "NonB1 dispatch hash is unchanged"
            );
        }
    }
    assert_eq!(
        hashes.len(),
        STATES.len(),
        "every eligibility transition invalidates dispatch/interface inputs"
    );
}

#[test]
fn cpp_owner_same_class_dispatch_rejects_forged_b1_containers() {
    let temp = tempfile::tempdir().unwrap();
    let db = IndexDb::open(&temp.path().join("index.db")).unwrap().0;
    let mut member = symbol(State::NonB1, 0);
    member.kind = SymbolKind::Method;
    member.symbol_uid = Some("member".into());
    member.container = Some("Box".into());
    let mut target = symbol(State::NonB1, 1);
    target.kind = SymbolKind::Method;
    target.name = "render".into();
    target.symbol_uid = Some("target".into());
    target.container = Some("Box".into());
    for state in STATES {
        target.cpp_qualified_owner = state;
        db.writes()
            .replace_files_batch(&[unit(vec![member.clone(), target.clone()])])
            .unwrap();
        let result = db
            .reads()
            .find_method_in_same_class("member", "render")
            .unwrap();
        assert_eq!(result, state.is_non_b1().then(|| "target".into()));
        let methods = db.reads().find_methods_by_containers(&["Box"]).unwrap();
        assert_eq!(methods["Box"].len(), 1 + usize::from(state.is_non_b1()));
        let classes = db
            .reads()
            .find_classes_with_method_names(&["render"])
            .unwrap();
        assert_eq!(classes.len(), usize::from(state.is_non_b1()));
        // A B1 caller with an injected display container is also ineligible.
        member.cpp_qualified_owner = state;
        target.cpp_qualified_owner = State::NonB1;
        db.writes()
            .replace_files_batch(&[unit(vec![member.clone(), target.clone()])])
            .unwrap();
        assert_eq!(
            db.reads()
                .find_method_in_same_class("member", "render")
                .unwrap(),
            state.is_non_b1().then(|| "target".into())
        );
        member.cpp_qualified_owner = State::NonB1;
    }
}
