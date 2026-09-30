//! P4-D V10: authored identity mutations over real parser, SQLite and incremental builds.
use cc_eval::benchmark::mutations::Mutation;
use cc_index::documents::{delta, render};
use cc_model::{
    identity::{DocumentRecord, DocumentRef},
    source::SourceSnapshot,
    Language,
};
use cc_parsers::ParserRegistry;
use cc_server::engine::CodeIndex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Debug,
    path::{Path, PathBuf},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema_version: u32,
    cases: Vec<Case>,
    renderer_spec_case: RendererSpecCase,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    id: String,
    initial_path: String,
    final_path: String,
    initial: String,
    target_marker: String,
    mutations: Vec<Mutation>,
    restore_mutations: Vec<Mutation>,
    expect: Expectation,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RendererSpecCase {
    path: String,
    source: String,
    target_marker: String,
    next_spec_suffix: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Expectation {
    target_count: usize,
    doc_key: Relation,
    doc_version: Relation,
    encoding_key: Relation,
    min_reusable_inputs: usize,
    unique_doc_keys: bool,
    unique_encoding_keys: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Relation {
    Same,
    Different,
    Any,
}

fn plan_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/mutations/p4d-document-identity.json")
}

fn load_plan() -> Plan {
    serde_json::from_slice(&std::fs::read(plan_path()).unwrap()).unwrap()
}

fn write(root: &Path, path: &str, content: &str) {
    let destination = root.join(path);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(destination, content.as_bytes()).unwrap();
}

fn records(root: &Path) -> Vec<DocumentRecord> {
    let connection = Connection::open(root.join(".codecortex/index.sqlite3")).unwrap();
    let mut statement = connection
        .prepare("SELECT record_json FROM document_manifest ORDER BY doc_key")
        .unwrap();
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .map(|row| serde_json::from_str(&row.unwrap()).unwrap())
        .collect()
}

fn settle(index: &mut CodeIndex, full: bool) -> cc_index::IndexReport {
    let mut report = index.build_index(full).unwrap();
    for _ in 0..16 {
        assert!(report.parse_errors.is_empty(), "{:?}", report.parse_errors);
        if report.resolution_freshness.complete {
            return report;
        }
        report = index.build_index(false).unwrap();
    }
    panic!(
        "resolution debt did not settle: {:?}",
        report.resolution_freshness
    );
}

fn target_records<'a>(
    root: &Path,
    all: &'a [DocumentRecord],
    path: &str,
    marker: &str,
) -> Vec<&'a DocumentRecord> {
    let source = std::fs::read_to_string(root.join(path)).unwrap();
    let mut result: Vec<_> = all
        .iter()
        .filter(|record| record.file_path == path)
        .filter(|record| {
            source
                .get(record.source.span.start..record.source.span.end)
                .is_some_and(|slice| slice.contains(marker))
        })
        .collect();
    result.sort_by_key(|record| record.occurrence);
    for record in &result {
        let slice = &source[record.source.span.start..record.source.span.end];
        record.validate(slice).unwrap();
    }
    result
}

fn check_relation<T: Eq + Debug>(relation: Relation, before: &T, after: &T, label: &str) {
    match relation {
        Relation::Same => assert_eq!(before, after, "{label} should be stable"),
        Relation::Different => assert_ne!(before, after, "{label} must change"),
        Relation::Any => {}
    }
}

fn assert_no_encoding_alias(records: impl IntoIterator<Item = DocumentRecord>) {
    let mut observed: BTreeMap<String, (String, String, String)> = BTreeMap::new();
    for record in records {
        let (Some(key), Some(input)) =
            (record.reference.encoding_key.clone(), record.input.as_ref())
        else {
            continue;
        };
        let value = (
            record.encoding_spec.clone(),
            input.input_hash.clone(),
            input.text.clone(),
        );
        if let Some(previous) = observed.insert(key.clone(), value.clone()) {
            assert_eq!(
                previous, value,
                "encoding key {key} aliased different spec/input content"
            );
        }
    }
}

fn relation_evidence(before: &DocumentRef, after: &DocumentRef) -> Value {
    json!({
        "doc_key_same": before.doc_key == after.doc_key,
        "doc_version_same": before.doc_version == after.doc_version,
        "encoding_key_same": before.encoding_key == after.encoding_key,
        "entity_key_same": before.entity_key == after.entity_key,
    })
}

#[test]
fn p4d_document_identity_mutations_are_reversible_and_input_safe() {
    let plan = load_plan();
    assert_eq!(plan.schema_version, 1);
    assert!(!plan.cases.is_empty() && plan.cases.len() <= 16);
    let mut ids = BTreeSet::new();
    let mut evidence = Vec::new();

    for case in &plan.cases {
        assert!(ids.insert(case.id.as_str()), "duplicate case id");
        assert!(cc_model::repo_path::is_canonical_file(&case.initial_path));
        assert!(cc_model::repo_path::is_canonical_file(&case.final_path));
        assert!(!case.target_marker.is_empty());
        assert!(!case.mutations.is_empty() && !case.restore_mutations.is_empty());
        for mutation in case.mutations.iter().chain(&case.restore_mutations) {
            mutation.validate().unwrap();
        }

        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        write(
            root,
            ".codecortex.json",
            r#"{"auto_index":{"enabled":false}}"#,
        );
        write(root, &case.initial_path, &case.initial);
        let mut index = CodeIndex::new(Some(root)).unwrap();
        settle(&mut index, true);
        let before = records(root);
        let before_targets = target_records(root, &before, &case.initial_path, &case.target_marker);
        assert_eq!(before_targets.len(), 1, "{} baseline target", case.id);
        let baseline = before_targets[0].reference.clone();

        for mutation in &case.mutations {
            mutation.apply(root).unwrap();
        }
        let mutation_report = settle(&mut index, false);
        let after = records(root);
        let after_targets = target_records(root, &after, &case.final_path, &case.target_marker);
        assert_eq!(
            after_targets.len(),
            case.expect.target_count,
            "{} target count",
            case.id
        );
        let primary = after_targets
            .iter()
            .min_by_key(|record| record.occurrence)
            .unwrap();
        check_relation(
            case.expect.doc_key,
            &baseline.doc_key,
            &primary.reference.doc_key,
            &format!("{} doc_key", case.id),
        );
        check_relation(
            case.expect.doc_version,
            &baseline.doc_version,
            &primary.reference.doc_version,
            &format!("{} doc_version", case.id),
        );
        check_relation(
            case.expect.encoding_key,
            &baseline.encoding_key,
            &primary.reference.encoding_key,
            &format!("{} encoding_key", case.id),
        );
        assert!(
            mutation_report.document_changes.reusable_inputs >= case.expect.min_reusable_inputs,
            "{} reusable input count: {:?}",
            case.id,
            mutation_report.document_changes
        );
        if case.expect.unique_doc_keys {
            let keys: BTreeSet<_> = after_targets
                .iter()
                .map(|record| &record.reference.doc_key)
                .collect();
            assert_eq!(keys.len(), after_targets.len(), "{} doc aliases", case.id);
        }
        if case.expect.unique_encoding_keys {
            let keys: BTreeSet<_> = after_targets
                .iter()
                .filter_map(|record| record.reference.encoding_key.as_ref())
                .collect();
            assert_eq!(
                keys.len(),
                after_targets.len(),
                "{} encoding aliases",
                case.id
            );
        }
        assert!(
            !cc_db::document_store::is_current(index.index_db().unwrap(), &baseline).unwrap(),
            "{} stale baseline stayed current",
            case.id
        );
        assert_no_encoding_alias(before.clone().into_iter().chain(after.clone()));

        for mutation in &case.restore_mutations {
            mutation.apply(root).unwrap();
        }
        let restore_report = settle(&mut index, false);
        let restored = records(root);
        let restored_targets =
            target_records(root, &restored, &case.initial_path, &case.target_marker);
        assert_eq!(restored_targets.len(), 1, "{} restored target", case.id);
        assert_eq!(
            restored_targets[0].reference, baseline,
            "{} did not restore deterministic identity",
            case.id
        );
        assert!(
            cc_db::document_store::is_current(index.index_db().unwrap(), &baseline).unwrap(),
            "{} restored baseline not current",
            case.id
        );
        assert_no_encoding_alias(restored.clone());
        evidence.push(json!({
            "id": case.id,
            "relation": relation_evidence(&baseline, &primary.reference),
            "target_count": after_targets.len(),
            "mutation_changes": mutation_report.document_changes,
            "restore_changes": restore_report.document_changes,
            "restored_exact_identity": true,
        }));
    }

    let spec = &plan.renderer_spec_case;
    assert!(cc_model::repo_path::is_canonical_file(&spec.path));
    assert!(spec.source.contains(&spec.target_marker));
    let outcome = ParserRegistry::new()
        .parse(&spec.path, &spec.source, Language::Python)
        .unwrap();
    let snapshot = SourceSnapshot::new(spec.source.as_bytes());
    let production = delta::prepare(&snapshot, &outcome, &[]).unwrap();
    let original = production
        .records
        .iter()
        .find(|record| {
            spec.source[record.source.span.start..record.source.span.end]
                .contains(&spec.target_marker)
        })
        .unwrap();
    let chunk = outcome
        .chunks
        .iter()
        .find(|chunk| chunk.chunk_id == original.chunk_id)
        .unwrap();
    let input = render::render(&snapshot, chunk, render::RenderOptions::default()).unwrap();
    let next_spec = format!("{}{}", original.encoding_spec, spec.next_spec_suffix);
    let changed = DocumentRecord::new(
        chunk,
        outcome.chunk_policy.as_deref().unwrap(),
        original.occurrence,
        &next_spec,
        Ok(input),
    )
    .unwrap();
    assert_eq!(original.reference.doc_key, changed.reference.doc_key);
    assert_ne!(
        original.reference.doc_version,
        changed.reference.doc_version
    );
    assert_ne!(
        original.reference.encoding_key,
        changed.reference.encoding_key
    );
    assert_eq!(
        original.input.as_ref().unwrap().input_hash,
        changed.input.as_ref().unwrap().input_hash,
        "template/spec identity must change even when rendered bytes are held fixed"
    );
    let spec_delta = delta::compare(
        std::slice::from_ref(&original.reference),
        std::slice::from_ref(&changed),
    );
    assert_eq!(spec_delta.upsert.len(), 1);
    assert!(spec_delta.removed.is_empty());
    assert_eq!(spec_delta.unchanged, 0);
    assert_eq!(spec_delta.reusable_inputs, 0);
    assert_no_encoding_alias(vec![original.clone(), changed]);

    let result = json!({
        "schema_version": 1,
        "status": "passed",
        "cases": evidence,
        "renderer_spec_change": {
            "doc_key_same": true,
            "doc_version_changed": true,
            "encoding_key_changed": true,
            "rendered_input_hash_same": true,
            "reusable_inputs": spec_delta.reusable_inputs,
        },
        "scope": "real parser + incremental/full SQLite document manifests; identity and reusable-input safety only, no vector provider or publication claim"
    });
    if let Ok(directory) = std::env::var("CODECORTEX_BENCH_OBSERVATIONS") {
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            Path::new(&directory).join("p4d-document-identity.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
}
