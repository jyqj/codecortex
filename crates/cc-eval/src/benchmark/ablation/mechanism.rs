//! Source-isolated local/dense mechanism counterfactuals. The production public
//! `semantic` policy retains local lanes; only these verified snapshots remove
//! them. Every cell receives the same public request and locked configuration.
use super::{strategy, BuildOptionsProjection, BuildReceipt, Control, Dataset, Hints, Variant};
use crate::benchmark::{
    adapters::mcp_stdio::McpStdio, invalid, manifest, normalizer, report, runner, schema::Suite,
    statistics, Result,
};
use cc_model::query::RetrievalStrategy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};

pub const SPEC: &str = "source-isolated-mechanism-ablation-v1";
const LOCAL: &str = "local_retrieval";
const DENSE: &str = "semantic_dense";
const LOCAL_LANES: [&str; 5] = ["exact_symbol", "path", "lexical", "grep", "graph"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Build {
    pub schema_version: u32,
    pub source_commit: String,
    pub reference_source: PathBuf,
    pub controls: Vec<Control>,
    pub variants: Vec<Variant>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub build: PathBuf,
    pub suite: PathBuf,
    pub readiness: strategy::ReadyPolicy,
}

/// The controlled replacements are checked in and compiled into the verifier.
/// A plan cannot certify arbitrary edited code merely by declaring its edits.
pub fn controls() -> Vec<Control> {
    serde_json::from_str(include_str!("mechanism_controls.json"))
        .expect("checked-in mechanism controls")
}
fn factors(name: &str) -> Result<(bool, bool)> {
    match name {
        "none" => Ok((false, false)),
        "local" => Ok((true, false)),
        "dense_only" => Ok((false, true)),
        "hybrid" => Ok((true, true)),
        _ => Err(invalid("unknown source-isolated mechanism cell")),
    }
}
fn effective(dense: bool) -> RetrievalStrategy {
    if dense {
        RetrievalStrategy::Semantic
    } else {
        RetrievalStrategy::Local
    }
}

pub fn validate_build(build: &Build, base: &Path, suite: &Path) -> Result<BuildOptionsProjection> {
    if build.schema_version != 1
        || build.source_commit.len() != 40
        || !build.source_commit.bytes().all(|b| b.is_ascii_hexdigit())
        || serde_json::to_value(&build.controls)? != serde_json::to_value(controls())?
        || build.variants.len() != 4
    {
        return Err(invalid(
            "unsupported mechanism build or undeclared controls",
        ));
    }
    let reference = base.join(&build.reference_source).canonicalize()?;
    let mut roots = BTreeSet::from([reference.clone()]);
    let mut binaries = BTreeSet::new();
    let mut hashes = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut receipts = BTreeSet::new();
    // The complete Cargo build input closure must be present, not a few edited
    // lines offered as a source snapshot. Other files are checked by inventory.
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/cc-server/Cargo.toml",
        "crates/cc-search/Cargo.toml",
        "crates/cc-server/src/main.rs",
    ] {
        if !reference.join(path).is_file() {
            return Err(invalid("incomplete mechanism build source snapshot"));
        }
    }
    for variant in &build.variants {
        let (local, dense) = factors(&variant.id)?;
        let expected: BTreeSet<_> = [(local, LOCAL), (dense, DENSE)]
            .into_iter()
            .filter_map(|(enabled, id)| enabled.then_some(id))
            .collect();
        if variant
            .enabled
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>()
            != expected
        {
            return Err(invalid(
                "mechanism cell label disagrees with enabled source factors",
            ));
        }
        let source = base.join(&variant.source_root).canonicalize()?;
        let binary = base.join(&variant.binary).canonicalize()?;
        let receipt_path = base.join(&variant.build_receipt).canonicalize()?;
        let receipt: BuildReceipt = manifest::json_file(&receipt_path)?;
        if receipt.build_options["binding"]["source_commit"].as_str()
            != Some(build.source_commit.as_str())
        {
            return Err(invalid(
                "mechanism source commit differs from build receipt binding",
            ));
        }
        let projection = super::projected_semantic_identity(&receipt.build_options)?;
        let target = Path::new(&projection.cargo_target_dir).canonicalize()?;
        if !roots.insert(source.clone())
            || !binaries.insert(binary.clone())
            || !receipts.insert(receipt_path)
            || !targets.insert(target)
            || !hashes.insert(receipt.binary_sha256)
            || binary.starts_with(&source)
            || source.starts_with(&reference)
            || receipt.build_options["features"] != json!(["semantic-http"])
        {
            return Err(invalid(
                "mechanism snapshots, binaries, receipts or build targets alias",
            ));
        }
    }
    super::validate(
        &super::Plan {
            schema_version: 1,
            reference_source: build.reference_source.clone(),
            controls: build.controls.clone(),
            variants: build.variants.clone(),
            datasets: vec![Dataset {
                id: "locked_input".into(),
                suite: suite.canonicalize()?,
                hints: Hints::default(),
            }],
            seed: 19,
        },
        base,
    )
}

/// A witness comes from validated, unmodified product output. Source controls
/// establish what was built; these receipts establish which lanes the product
/// reported for this request. Presence alone never becomes positive recall.
pub fn lane_witness(raw: &Value, cell: &str) -> Result<Value> {
    let (local, dense) = factors(cell)?;
    normalizer::mcp(raw)?;
    let retrieval = &raw["evidence_summary"]["retrieval"];
    let lanes = retrieval
        .get("lanes")
        .or_else(|| retrieval.get("lane_receipts"))
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("missing actual lane receipts"))?;
    let expected: BTreeSet<_> = LOCAL_LANES
        .into_iter()
        .filter(|_| local)
        .chain(dense.then_some("semantic"))
        .collect();
    let observed: BTreeSet<_> = lanes
        .iter()
        .map(|lane| {
            lane["lane_id"]
                .as_str()
                .ok_or_else(|| invalid("lane receipt has no identity"))
        })
        .collect::<Result<_>>()?;
    if observed != expected || observed.len() != lanes.len() {
        return Err(invalid(
            "actual lane execution differs from isolated source factors",
        ));
    }
    let projected = lanes
        .iter()
        .map(|lane| {
            json!({
                "lane_id":lane["lane_id"], "status":lane["status"],
                "candidate_count":lane["candidate_count"], "coverage":lane["coverage"],
                "truncation_reason":lane["truncation_reason"], "elapsed_us":lane["elapsed_us"]
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({"profile":"fake", "cell":cell, "local_factor":local,
        "semantic_dense_factor":dense, "lane_receipts":projected,
        "source_of_observation":"validated actual MCP search response; no synthesized lane"}))
}

fn configuration(plan: &Plan, suite: &Suite) -> Result<cc_model::query::QueryConfig> {
    if plan.schema_version != 1 || plan.readiness.expected_space.is_none() {
        return Err(invalid(
            "mechanism experiment requires pinned loopback dense readiness",
        ));
    }
    // Reuse the public harness's strict config/loopback/readiness validation.
    // This descriptor is not executed; actual cells all request Semantic below.
    strategy::validate_configuration(
        &strategy::Plan {
            schema_version: 1,
            suite: plan.suite.clone(),
            source_snapshot: PathBuf::new(),
            binary: PathBuf::new(),
            build_receipt: PathBuf::new(),
            strategies: vec![RetrievalStrategy::Local, RetrievalStrategy::Semantic],
            network: strategy::Network::LoopbackOnly,
            readiness: plan.readiness.clone(),
        },
        suite,
    )
}

pub async fn run(plan_path: &Path, out: &Path) -> Result<i32> {
    let plan: Plan = manifest::json_file(plan_path)?;
    let base = plan_path.parent().unwrap_or(Path::new("."));
    let build_path = base.join(&plan.build).canonicalize()?;
    let build_base = build_path
        .parent()
        .ok_or_else(|| invalid("build manifest parent"))?;
    let build: Build = manifest::json_file(&build_path)?;
    let suite_path = base.join(&plan.suite).canonicalize()?;
    let loaded = manifest::load(&suite_path)?;
    let query = configuration(&plan, &loaded.suite)?;
    let projection = validate_build(&build, build_base, &suite_path)?;
    if out.exists() {
        return Err(invalid("mechanism output already exists; raw is immutable"));
    }
    std::fs::create_dir_all(out)?;
    report::json(&out.join("plan.json"), &plan)?;
    report::json(&out.join("mechanism-build.json"), &build)?;
    let mut cells = BTreeMap::new();
    let mut gates = BTreeMap::new();
    let mut summaries = BTreeMap::new();
    let mut costs = BTreeMap::new();
    let mut witnesses = BTreeMap::new();
    let mut exit = 0;
    let order = statistics::order(build.variants.len(), loaded.suite.seed);
    for index in &order {
        let variant = &build.variants[*index];
        let name = &variant.id;
        let (_, dense) = factors(name)?;
        let cell = out.join(name);
        std::fs::create_dir(&cell)?;
        let isolated = manifest::materialize(&loaded)?;
        let binary = build_base.join(&variant.binary).canonicalize()?;
        let receipt: BuildReceipt = manifest::json_file(&build_base.join(&variant.build_receipt))?;
        let mut provenance = runner::engine_provenance(Some(&binary))?;
        provenance["mechanism_ablation"] = json!({"spec":SPEC,"profile":"fake",
            "cell":name,"enabled":variant.enabled,"source_commit":build.source_commit,
            "build_receipt":receipt,"controls":build.controls,"build_options_projection":projection,
            "requested_strategy":"semantic","expected_effective":effective(dense),
            "query_budget":query,"readiness":plan.readiness,"network":"loopback_only",
            "input_scope":"new isolated project, index and caches for each cell",
            "effect_interpretation":"none; source-isolated engineering mechanism only"});
        let client = McpStdio::spawn(
            &binary,
            isolated.path(),
            Duration::from_millis(loaded.suite.timeout_ms),
        )
        .await;
        let result = match client {
            Ok(client) => {
                runner::run(
                    &loaded,
                    strategy::ProfileBackend::new(
                        client,
                        isolated.path().to_path_buf(),
                        cell.clone(),
                        RetrievalStrategy::Semantic,
                        query.clone(),
                        plan.readiness.clone(),
                    )?
                    .expect_effective(effective(dense)),
                    isolated.path(),
                    &cell,
                    provenance,
                    "fake",
                )
                .await
            }
            Err(error) => Err(error),
        };
        match result {
            Ok(gate) => {
                exit = exit.max(gate.exit_code);
                let rows = report::read_jsonl(&cell.join("normalized.jsonl"))?;
                let (summary, _) = report::summarize(
                    &rows,
                    &loaded.queries,
                    loaded.suite.scoring,
                    loaded.suite.seed,
                )?;
                summaries.insert(name.clone(), summary);
                gates.insert(name.clone(), gate);
            }
            Err(error) => {
                exit = 2;
                report::json(
                    &cell.join("failure.json"),
                    &json!({"error":error.to_string()}),
                )?;
                gates.insert(
                    name.clone(),
                    crate::benchmark::gate::Gate {
                        status: "invalid_measurement".into(),
                        exit_code: 2,
                        reasons: vec![error.to_string()],
                    },
                );
            }
        }
        let checked = strategy::read_observations_effective(
            &cell,
            RetrievalStrategy::Semantic,
            &query,
            effective(dense),
        )
        .and_then(|rows| {
            let mut evidence = Vec::new();
            for row in &rows {
                let raw: Value = manifest::json_file(&cell.join(&row.raw_path))?;
                let witness = lane_witness(&raw, name)?;
                evidence.push(
                    json!({"sequence":row.sequence,"input_digest":row.input_digest,
                        "raw_path":row.raw_path,"raw_sha256":row.raw_sha256,"witness":witness}),
                );
            }
            Ok((rows, evidence))
        });
        let (rows, evidence) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                exit = 2;
                report::json(
                    &cell.join("mechanism-integrity-failure.json"),
                    &json!({"error":error.to_string()}),
                )?;
                (Vec::new(), Vec::new())
            }
        };
        report::jsonl(&cell.join("lane-execution.jsonl"), &evidence)?;
        report::jsonl(
            &cell.join("strategy-costs.jsonl"),
            &strategy::reporting::cost_rows(&rows, "fake"),
        )?;
        costs.insert(
            name.clone(),
            strategy::reporting::cost_columns(&rows, "fake"),
        );
        witnesses.insert(name.clone(), evidence.len());
        cells.insert(name.clone(), rows);
    }
    // Re-read all source bytes, receipts and binaries after execution.
    validate_build(&build, build_base, &suite_path)?;
    let inputs = loaded
        .queries
        .iter()
        .map(|q| q.input(loaded.suite.top_k))
        .collect::<Vec<_>>();
    let comparison = strategy::compare_population(
        &cells,
        &inputs,
        loaded.suite.repetitions + loaded.suite.warmup,
    )
    .and_then(|()| strategy::compare_budgets(&cells));
    if comparison.is_err() {
        exit = 2;
    }
    let mut paired = Vec::new();
    if exit == 0 {
        let local = summaries
            .get("local")
            .ok_or_else(|| invalid("missing local source cell"))?;
        for name in ["dense_only", "hybrid"] {
            paired.push(json!({"baseline":"local","candidate":name,
                "observations":strategy::reporting::paired_summary(local, &summaries[name],
                    &loaded.queries, loaded.suite.repetitions, loaded.suite.seed, "fake")?}));
        }
    }
    report::json(
        &out.join("comparison.json"),
        &json!({"spec":SPEC,"profile":"fake",
        "exit_code":exit,"gates":gates,"source_commit":build.source_commit,
        "execution_order":order.iter().map(|i| &build.variants[*i].id).collect::<Vec<_>>(),
        "build_options_projection":projection,"equal_effective_budgets":comparison.is_ok(),
        "budgets_by_input":comparison.as_ref().ok(),
        "comparison_error":comparison.as_ref().err().map(ToString::to_string),
        "lane_witness_counts":witnesses,"cost_columns":costs,"paired_against_local":paired,
        "paired_status":if exit==0{"mechanism_only"}else{"unavailable_failed_integrity_or_population_gate"},
        "capability_scope":{"none":"all candidate lanes removed; negative control",
            "local":"local registry only; semantic dispatch removed",
            "dense_only":"semantic candidate lane only; entire local registry removed",
            "hybrid":"unmodified production local lanes plus semantic"},
        "full_p7_019_fake_mechanism_acceptance":false,
        "acceptance_note":"the fixed actual-product positive/negative fixture must pass separately",
        "held_out_semantic_quality_gate":"not_evaluated",
        "limitations":["fake/authored inputs have no real semantic quality interpretation",
            "full source/build receipts are local provenance, not remote attestation",
            "dense-only describes candidate retrieval; shared planning, source validation and packing remain",
            "budget equality includes hard scope, candidate caps and complete output bytes",
            "provider billing and cache-reuse attribution remain unknown, never zero-filled",
            "latency includes adapter validation and persistence; no product latency SLA"]}),
    )?;
    Ok(exit)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn raw(ids: &[&str]) -> Value {
        json!({"machine_pack":{"hits":[]},"evidence_summary":{"retrieval":{"lanes":
            ids.iter().map(|id| cc_model::retrieval::LaneOutcome::disabled(*id,1.0)).collect::<Vec<_>>()}}})
    }
    #[test]
    fn public_semantic_with_local_lanes_cannot_masquerade_as_dense_only() {
        let hybrid = raw(&[
            "exact_symbol",
            "path",
            "lexical",
            "grep",
            "graph",
            "semantic",
        ]);
        assert!(lane_witness(&hybrid, "hybrid").is_ok());
        assert!(lane_witness(&hybrid, "dense_only").is_err());
        assert!(lane_witness(&raw(&["semantic"]), "dense_only").is_ok());
        assert!(lane_witness(&raw(&["semantic"]), "local").is_err());
        assert!(lane_witness(&raw(&[]), "none").is_ok());
        assert!(lane_witness(&raw(&["semantic"]), "none").is_err());
    }
    #[test]
    fn absent_duplicate_and_unexpected_receipts_cannot_prove_isolation() {
        assert!(lane_witness(&json!({"machine_pack":{"hits":[]}}), "none").is_err());
        assert!(lane_witness(&raw(&["semantic", "semantic"]), "dense_only").is_err());
        assert!(lane_witness(&raw(&["invented"]), "dense_only").is_err());
    }
    #[test]
    fn fixed_controls_match_current_product_source_once() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for control in controls() {
            let current = std::fs::read_to_string(root.join(control.path)).unwrap();
            assert_eq!(current.matches(&control.on_text).count(), 1);
            assert!(!current.contains(&control.off_text));
        }
    }
}
