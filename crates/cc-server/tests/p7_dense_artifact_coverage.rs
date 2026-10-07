//! P7-012 read-time coverage regression, frozen against the existing Vec API.
//! A real worker publishes three synthetic embeddings. Only cache payloads
//! are then damaged; no publication or receipt is seeded by the test.
//! This is offline behavior evidence, not live-provider or corpus-quality evidence.
#![cfg(feature = "semantic")]

use cc_db::{
    index_db::IndexDb,
    semantic_manifest_reads::{SemanticManifestReads, SemanticManifestRow},
};
use cc_model::{
    config::ProjectConfig,
    query::{QueryControl, RetrievalStrategy},
    retrieval::{HardScope, LaneOutcome, LaneStatus},
    search::SearchRequest,
    semantic::{SemanticRecall, SemanticRequest},
    ContextEnvelope, Language,
};
use cc_semantic::{
    cache::CacheRead,
    ports::{DocumentInput, EmbeddingProvider, ProviderError, QueryInput},
    types::{DocSpecDigest, InputDigest, VectorSpace},
};
use cc_server::{
    engine::CodeIndex,
    handlers::SharedCodeIndex,
    query_handle::QueryHandle,
    semantic_query_encoding::{
        NetworkLimits, QueryEncodingInputs, QueryEncodingService, QueryNetworkCapacity,
    },
    semantic_wiring::SemanticSubsystem,
    service_factory::QueryServices,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{Arc, Mutex, RwLock},
    time::Duration,
};

const QUERY: &str = "conceptual amber fox";
const DOCS: [(&str, &str); 3] = [
    ("scope/keep.rs", "pub fn retained() -> u32 { 731 }\n"),
    ("scope/lost.py", "def missing():\n    return 732\n"),
    ("outside/foreign.rs", "pub fn external() -> u32 { 999 }\n"),
];

struct UnitProvider(VectorSpace, Mutex<Vec<InputDigest>>);
impl EmbeddingProvider for UnitProvider {
    fn space(&self) -> &VectorSpace {
        &self.0
    }
    fn embed_documents(&self, input: &[DocumentInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        self.1
            .lock()
            .unwrap()
            .extend(input.iter().map(|item| item.input_digest.clone()));
        Ok(input.iter().map(|_| vec![1.0, 0.0]).collect())
    }
    fn embed_queries(&self, input: &[QueryInput]) -> Result<Vec<Vec<f32>>, ProviderError> {
        Ok(input.iter().map(|_| vec![1.0, 0.0]).collect())
    }
}

struct World {
    _root: tempfile::TempDir,
    db: Arc<IndexDb>,
    runtime: SharedCodeIndex,
    subsystem: Arc<SemanticSubsystem>,
    published: Vec<SemanticManifestRow>,
    document_inputs: Vec<InputDigest>,
}

impl World {
    async fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        for (path, text) in DOCS {
            let file = root.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        std::fs::write(
            root.path().join(".codecortex.json"),
            r#"{"auto_index":{"enabled":false},"indexing":{"db_read_pool_size":1},"query":{"deadline_ms":5000,"semantic_timeout_ms":3000}}"#,
        )
        .unwrap();
        let mut index = CodeIndex::new(Some(root.path())).unwrap();
        assert!(index.build_index(false).unwrap().parse_errors.is_empty());
        let db = index.index_db().unwrap().clone();
        let mut config = ProjectConfig::default();
        config.indexing.db_read_pool_size = Some(1);
        config.semantic.enabled = true;
        config.semantic.model_id = "synthetic/p7-artifact-coverage".into();
        config.semantic.dimensions = Some(2);
        config.semantic.max_input_tokens = Some(8192);
        config.semantic.max_batch_items = Some(16);
        config.semantic.endpoint = "https://semantic.invalid/v1".into();
        let subsystem = Arc::new(
            cc_server::semantic_wiring::assemble_with(
                &root.path().to_string_lossy(),
                &config,
                db.clone(),
                |key| {
                    (key == cc_semantic::cache::CACHE_ROOT_ENV)
                        .then(|| root.path().join("cache").to_string_lossy().into_owned())
                },
                false,
            )
            .unwrap()
            .unwrap(),
        );
        let services = Arc::new(QueryServices::default());
        let provider = Arc::new(UnitProvider(subsystem.space.clone(), Mutex::new(vec![])));
        let worker = cc_server::semantic_runtime::SemanticRuntime::new(
            db.clone(),
            subsystem.clone(),
            services.clone(),
            provider.clone(),
        )
        .unwrap();
        assert!(worker.schedule());
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let coverage = db.reads().semantic_coverage().unwrap().coverage;
                if coverage.eligible == 3 && coverage.uncovered == 0 && services.query_pins() == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .unwrap();
        // No background repair races with payload damage or restoration.
        worker.close();
        assert_eq!(services.query_pins(), 0);
        let document_inputs = provider.1.lock().unwrap().clone();
        let encoder = QueryEncodingService::new(
            QueryEncodingInputs {
                cache: subsystem.query_cache.clone(),
                namespace: subsystem.namespace.clone(),
                spec: subsystem.query_spec.clone(),
                budget: cc_semantic::admission::InputBudget::validated(1, 1024, 8192).unwrap(),
                lifecycle: Arc::new(cc_db::semantic_publish::LifecycleFence::default()),
            },
            QueryNetworkCapacity::new(NetworkLimits {
                query_running: 1,
                query_queued: 0,
                background_running: 1,
                background_queued: 0,
            })
            .unwrap(),
            move |_| Ok(provider.clone()),
        )
        .unwrap();
        encoder
            .encode(QUERY.as_bytes().to_vec(), control())
            .await
            .unwrap();
        let published = {
            let conn = db.read_conn().unwrap();
            SemanticManifestReads::on(&conn)
                .scan_space(subsystem.space.digest().unwrap().as_str(), "", 10)
                .unwrap()
        };
        assert_eq!(
            published.len(),
            3,
            "three actual worker publications required"
        );
        assert_eq!(
            published
                .iter()
                .map(|row| row.file_path.as_str())
                .collect::<BTreeSet<_>>(),
            DOCS.iter().map(|(path, _)| *path).collect()
        );
        index.set_semantic_recall(Some(subsystem.recall.clone()));
        Self {
            _root: root,
            db,
            runtime: Arc::new(RwLock::new(index)),
            subsystem,
            published,
            document_inputs,
        }
    }

    async fn recall(&self, scope: HardScope, limit: usize) -> LaneOutcome {
        self.subsystem
            .recall
            .recall(
                SemanticRequest {
                    query: QUERY.into(),
                    scope,
                    limit,
                    policy_fingerprint: "p7-artifact-coverage-v1".into(),
                    generation: self.db.reads().read_generation().unwrap(),
                },
                control(),
            )
            .await
            .unwrap()
    }

    async fn envelope(&self, path_prefix: &str) -> ContextEnvelope {
        QueryHandle::capture(&self.runtime)
            .unwrap()
            .search_async(
                QUERY.into(),
                10,
                None,
                SearchRequest {
                    retrieval_strategy: Some(RetrievalStrategy::Semantic),
                    path_prefix: Some(path_prefix.into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap()
    }

    fn assert_receipt(&self, outcome: &LaneOutcome, status: LaneStatus, paths: &[&str]) {
        outcome.validate().unwrap();
        assert_eq!(
            outcome.status, status,
            "read-time artifact loss must affect coverage"
        );
        assert_eq!(outcome.coverage.complete, status == LaneStatus::Complete);
        assert_eq!(outcome.candidate_count, paths.len());
        assert_eq!(outcome.coverage.total_lower_bound, paths.len());
        let actual = outcome
            .candidates
            .iter()
            .map(|candidate| {
                self.published
                    .iter()
                    .find(|row| row.doc_key == candidate.document.doc_key)
                    .unwrap()
                    .file_path
                    .as_str()
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, paths.iter().copied().collect());
        if status == LaneStatus::Partial {
            assert_eq!(
                outcome.truncation_reason.as_deref(),
                Some("semantic_artifact_unavailable")
            );
            assert!(
                !outcome.is_cacheable(),
                "a file repair must be visible without an epoch bump"
            );
        } else {
            assert!(outcome.truncation_reason.is_none());
        }
    }

    fn artifact(&self, path: &str) -> SavedArtifact {
        let row = self
            .published
            .iter()
            .find(|row| row.file_path == path)
            .unwrap();
        let address: Vec<_> = row.artifact_ref.split(':').collect();
        assert_eq!(address.len(), 6);
        assert_eq!(address[0], "cas.v1");
        assert_eq!(address[1], self.subsystem.cache.namespace());
        assert_eq!(address[2], row.space_id);
        assert_eq!(address[3], row.input_digest);
        assert_eq!(address[4], self.subsystem.doc_spec.as_str());
        // Public cache v1 layout; corrupt the exact file published for this row.
        let payload = self
            .subsystem
            .cache
            .root()
            .join(format!("namespace-{}", address[1]))
            .join(address[2])
            .join(address[3])
            .join(address[4])
            .join(format!("{}.bin", address[4]));
        let original = std::fs::read(&payload).unwrap();
        assert_eq!(original.len(), 8);
        SavedArtifact {
            payload,
            original,
            input: self
                .document_inputs
                .iter()
                .find(|input| input.as_str() == row.input_digest)
                .unwrap()
                .clone(),
            spec: self.subsystem.doc_spec.clone(),
        }
    }

    fn assert_publications_unchanged(&self) {
        let conn = self.db.read_conn().unwrap();
        assert_eq!(
            SemanticManifestReads::on(&conn)
                .scan_space(self.subsystem.space.digest().unwrap().as_str(), "", 10)
                .unwrap(),
            self.published
        );
        drop(conn);
        assert_eq!(
            self.db
                .reads()
                .semantic_coverage()
                .unwrap()
                .coverage
                .uncovered,
            0
        );
        assert!(
            !self.subsystem.ledger.snapshot().degraded,
            "query reads must not mutate the repair ledger"
        );
    }
}

struct SavedArtifact {
    payload: PathBuf,
    original: Vec<u8>,
    input: InputDigest,
    spec: DocSpecDigest,
}
#[derive(Clone, Copy)]
enum Fault {
    Missing,
    Corrupt,
}
impl SavedArtifact {
    fn damage(&self, world: &World, fault: Fault) {
        match fault {
            Fault::Missing => std::fs::remove_file(&self.payload).unwrap(),
            Fault::Corrupt => {
                let mut bad = self.original.clone();
                bad[0] ^= 1; // Same-length payload with an invalid sidecar checksum.
                std::fs::write(&self.payload, bad).unwrap();
            }
        }
        let read = world
            .subsystem
            .cache
            .get(&world.subsystem.space, &self.input, &self.spec)
            .unwrap();
        assert!(matches!(
            (fault, read),
            (Fault::Missing, CacheRead::Miss) | (Fault::Corrupt, CacheRead::Corrupt(_))
        ));
    }
    fn restore(&self) {
        std::fs::write(&self.payload, &self.original).unwrap();
    }
}

fn control() -> QueryControl {
    QueryControl::new(Duration::from_secs(5)).unwrap()
}
fn scope(prefix: &str) -> HardScope {
    HardScope {
        path_prefix: Some(prefix.into()),
        ..Default::default()
    }
}
fn semantic_lane(envelope: &ContextEnvelope) -> &Value {
    envelope.evidence_summary["retrieval"]["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|lane| lane["lane_id"] == "semantic")
        .unwrap()
}
fn assert_envelope(envelope: &ContextEnvelope, status: &str, paths: &[&str]) {
    let lane = semantic_lane(envelope);
    assert_eq!(lane["status"], status);
    assert_eq!(lane["coverage"]["complete"], status == "complete");
    assert_eq!(lane["candidate_count"], paths.len());
    if status == "partial" {
        assert_eq!(lane["truncation_reason"], "semantic_artifact_unavailable");
    }
    let hits = envelope.machine_pack["hits"].as_array().unwrap();
    assert_eq!(hits.len(), paths.len());
    assert_eq!(
        hits.iter()
            .map(|hit| hit["file_path"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        paths.iter().copied().collect()
    );
    for hit in hits {
        let text = hit["text"].as_str().unwrap();
        let source = DOCS
            .iter()
            .find(|(path, _)| *path == hit["file_path"].as_str().unwrap())
            .unwrap()
            .1;
        assert!(
            !text.is_empty() && source.contains(text),
            "surviving semantic evidence must hydrate current source"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_artifact_changes_fresh_and_warm_receipts_until_restored() {
    let world = World::new().await;
    let original_generation = world.db.reads().read_generation().unwrap();
    let lost = world.artifact("scope/lost.py");
    world.assert_receipt(
        &world.recall(scope("scope/"), 10).await,
        LaneStatus::Complete,
        &["scope/keep.rs", "scope/lost.py"],
    );
    assert_envelope(
        &world.envelope("scope/").await,
        "complete",
        &["scope/keep.rs", "scope/lost.py"],
    );
    for _ in 0..2 {
        lost.damage(&world, Fault::Missing);
        for _ in 0..2 {
            world.assert_receipt(
                &world.recall(scope("scope/"), 10).await,
                LaneStatus::Partial,
                &["scope/keep.rs"],
            );
            assert_envelope(
                &world.envelope("scope/").await,
                "partial",
                &["scope/keep.rs"],
            );
        }
        world.assert_publications_unchanged();
        lost.restore();
        world.assert_receipt(
            &world.recall(scope("scope/"), 10).await,
            LaneStatus::Complete,
            &["scope/keep.rs", "scope/lost.py"],
        );
        assert_envelope(
            &world.envelope("scope/").await,
            "complete",
            &["scope/keep.rs", "scope/lost.py"],
        );
    }
    assert_eq!(
        world.db.reads().read_generation().unwrap(),
        original_generation
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_artifact_is_partial_even_when_no_candidate_survives() {
    let world = World::new().await;
    let lost = world.artifact("scope/lost.py");
    lost.damage(&world, Fault::Corrupt);
    world.assert_receipt(
        &world.recall(scope("scope/lost.py"), 10).await,
        LaneStatus::Partial,
        &[],
    );
    assert_envelope(&world.envelope("scope/lost.py").await, "partial", &[]);
    world.assert_publications_unchanged();
    // A complete scan with no eligible documents is a different outcome.
    world.assert_receipt(
        &world.recall(scope("absent/"), 10).await,
        LaneStatus::Complete,
        &[],
    );
    assert_envelope(&world.envelope("absent/").await, "complete", &[]);
    lost.restore();
    world.assert_receipt(
        &world.recall(scope("scope/lost.py"), 10).await,
        LaneStatus::Complete,
        &["scope/lost.py"],
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn out_of_scope_damage_and_empty_or_zero_limit_queries_stay_complete() {
    let world = World::new().await;
    let outside = world.artifact("outside/foreign.rs");
    for fault in [Fault::Missing, Fault::Corrupt] {
        outside.damage(&world, fault);
        world.assert_receipt(
            &world.recall(scope("scope/"), 10).await,
            LaneStatus::Complete,
            &["scope/keep.rs", "scope/lost.py"],
        );
        outside.restore();
    }
    outside.damage(&world, Fault::Corrupt);
    world
        .artifact("scope/lost.py")
        .damage(&world, Fault::Missing);
    for filter in [
        HardScope {
            file_paths: Some(vec!["scope/keep.rs".into()]),
            ..Default::default()
        },
        HardScope {
            languages: Some(vec![Language::Rust]),
            ..scope("scope/")
        },
        HardScope {
            file_paths: Some(vec!["scope/keep.rs".into(), "outside/foreign.rs".into()]),
            ..scope("scope/")
        },
    ] {
        world.assert_receipt(
            &world.recall(filter, 10).await,
            LaneStatus::Complete,
            &["scope/keep.rs"],
        );
    }
    for filter in [
        HardScope {
            file_paths: Some(vec![]),
            ..Default::default()
        },
        HardScope {
            languages: Some(vec![]),
            ..Default::default()
        },
        scope("absent/"),
    ] {
        world.assert_receipt(&world.recall(filter, 10).await, LaneStatus::Complete, &[]);
    }
    world.assert_receipt(
        &world.recall(HardScope::default(), 0).await,
        LaneStatus::Complete,
        &[],
    );
    world.assert_publications_unchanged();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_space_keeps_its_unavailable_receipt_despite_artifact_damage() {
    let world = World::new().await;
    world
        .artifact("scope/lost.py")
        .damage(&world, Fault::Missing);
    // Exercise the sanctioned activation fence after the real worker stops.
    world
        .db
        .register_semantic_space("other-space", "{}")
        .unwrap();
    world
        .db
        .switch_semantic_active_space("other-space", "artifact-coverage-control")
        .unwrap();
    let outcome = world.recall(scope("scope/"), 10).await;
    outcome.validate().unwrap();
    assert_eq!(outcome.status, LaneStatus::Unavailable);
    assert_eq!(
        outcome.truncation_reason.as_deref(),
        Some("semantic_space_not_active")
    );
    assert!(!outcome.coverage.complete);
    assert!(outcome.candidates.is_empty());
}
