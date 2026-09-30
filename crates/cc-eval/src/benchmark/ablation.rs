//! Locked full-factorial experiments over separately built local MCP binaries.
//! Counterfactual source lives in isolated snapshots, never production feature flags.
use super::{
    adapters::{mcp_stdio::McpStdio, Backend},
    invalid, manifest,
    readiness::Readiness,
    report, runner,
    schema::SearchInput,
    statistics, BenchError, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub id: String,
    pub path: String,
    pub on_text: String,
    pub off_text: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub enabled: Vec<String>,
    pub source_root: PathBuf,
    pub binary: PathBuf,
    pub build_receipt: PathBuf,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Hints {
    pub pinned_files: Vec<String>,
    pub file_preselect_limit: Option<usize>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dataset {
    pub id: String,
    pub suite: PathBuf,
    #[serde(default)]
    pub hints: Hints,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub reference_source: PathBuf,
    pub controls: Vec<Control>,
    pub variants: Vec<Variant>,
    pub datasets: Vec<Dataset>,
    pub seed: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildReceipt {
    pub source_files: BTreeMap<String, String>,
    pub binary_sha256: String,
    pub build_options: Value,
    pub exit_code: i32,
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn inventory(root: &Path) -> Result<BTreeMap<String, String>> {
    fn visit(
        root: &Path,
        dir: &Path,
        depth: usize,
        total: &mut u64,
        map: &mut BTreeMap<String, String>,
    ) -> Result<()> {
        if depth > 32 {
            return Err(invalid("source snapshot depth"));
        }
        for e in std::fs::read_dir(dir)? {
            let e = e?;
            let t = e.file_type()?;
            if t.is_symlink() {
                return Err(invalid("symlink in experiment source"));
            }
            if t.is_dir() {
                visit(root, &e.path(), depth + 1, total, map)?;
            } else if t.is_file() {
                let size = e.metadata()?.len();
                *total = total.saturating_add(size);
                if *total > 128 * 1024 * 1024 || size > 4 * 1024 * 1024 || map.len() >= 5000 {
                    return Err(invalid("source snapshot too large"));
                }
                let key = e
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| invalid("snapshot path"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                map.insert(key, sha(&std::fs::read(e.path())?));
            } else {
                return Err(invalid("non-regular experiment source"));
            }
        }
        Ok(())
    }
    let mut map = BTreeMap::new();
    visit(root, root, 0, &mut 0, &mut map)?;
    Ok(map)
}
/// Validate every source byte, exact counterfactual patch and build/binary receipt
/// before executing any variant. Receipts are provenance, not remote attestation.
pub fn validate(plan: &Plan, base: &Path) -> Result<()> {
    if plan.schema_version != 1
        || !(1..=4).contains(&plan.controls.len())
        || plan.variants.len() != 1 << plan.controls.len()
        || plan.datasets.is_empty()
        || plan.datasets.len() > 16
    {
        return Err(invalid("ablation version or matrix bounds"));
    }
    let reference = base.join(&plan.reference_source);
    let original = inventory(&reference)?;
    if original.is_empty() {
        return Err(invalid("empty reference source"));
    }
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for c in &plan.controls {
        super::validation::relative_path(&c.path)?;
        if !slug(&c.id)
            || !ids.insert(c.id.clone())
            || !paths.insert(c.path.clone())
            || c.on_text.is_empty()
            || c.on_text == c.off_text
        {
            return Err(invalid("invalid/aliased control"));
        }
        let text = std::fs::read_to_string(reference.join(&c.path))?;
        if text.matches(&c.on_text).count() != 1 {
            return Err(invalid("control must match reference exactly once"));
        }
    }
    let mut variants = BTreeSet::new();
    let mut cells = BTreeSet::new();
    let mut build_options = None;
    for v in &plan.variants {
        let enabled: BTreeSet<_> = v.enabled.iter().cloned().collect();
        if !slug(&v.id)
            || !variants.insert(&v.id)
            || enabled.len() != v.enabled.len()
            || !enabled.is_subset(&ids)
            || !cells.insert(enabled.clone())
        {
            return Err(invalid("duplicate/unknown matrix cell"));
        }
        let mut expected = original.clone();
        for c in &plan.controls {
            if !enabled.contains(&c.id) {
                let text = std::fs::read_to_string(reference.join(&c.path))?;
                expected.insert(
                    c.path.clone(),
                    sha(text.replacen(&c.on_text, &c.off_text, 1).as_bytes()),
                );
            }
        }
        let observed = inventory(&base.join(&v.source_root))?;
        let receipt: BuildReceipt = manifest::json_file(&base.join(&v.build_receipt))?;
        if observed != expected || receipt.source_files != observed || receipt.exit_code != 0 {
            return Err(invalid(format!(
                "{} source has undeclared changes or invalid build",
                v.id
            )));
        }
        if sha(&std::fs::read(base.join(&v.binary))?) != receipt.binary_sha256 {
            return Err(invalid("binary digest drift"));
        }
        if receipt.build_options.is_null()
            || build_options
                .as_ref()
                .is_some_and(|b| b != &receipt.build_options)
        {
            return Err(invalid("build options differ or unavailable"));
        }
        build_options = Some(receipt.build_options);
    }
    let mut datasets = BTreeSet::new();
    for d in &plan.datasets {
        if !slug(&d.id)
            || !datasets.insert(&d.id)
            || d.hints.pinned_files.len() > 200
            || d.hints
                .file_preselect_limit
                .is_some_and(|n| !(1..=4000).contains(&n))
        {
            return Err(invalid("dataset or hint bounds"));
        }
        for p in &d.hints.pinned_files {
            super::validation::relative_path(p)?;
        }
        manifest::load(&base.join(&d.suite))?;
    }
    Ok(())
}
struct WithHints {
    client: McpStdio,
    hints: Hints,
}
impl Backend for WithHints {
    fn name(&self) -> &'static str {
        "mcp-stdio"
    }
    fn pid(&self) -> Option<u32> {
        self.client.pid()
    }
    async fn prepare(&mut self, files: &[manifest::FileRecord]) -> Result<Value> {
        self.client.prepare(files).await
    }
    async fn readiness(&mut self, files: &[manifest::FileRecord]) -> Result<Readiness> {
        self.client.readiness(files).await
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value> {
        let mut args = json!({"query":input.query,"top_k":input.top_k,"mode":"hybrid"});
        if let Some(p) = &input.path_prefix {
            args["path_prefix"] = json!(p);
        }
        if !self.hints.pinned_files.is_empty() {
            args["pinned_files"] = json!(self.hints.pinned_files);
        }
        if let Some(n) = self.hints.file_preselect_limit {
            args["file_preselect_limit"] = json!(n);
        }
        self.client.call("search", args).await
    }
    async fn close(&mut self) -> Result<()> {
        self.client.close().await
    }
}
/// Execute all cells against each unchanged dataset; pair only one-factor edges.
/// Raw runs and failing gates are retained. This is a diagnostic experiment, not
/// a release quality or latency certification; no best-of or gold-conditioned input.
pub async fn run(plan_path: &Path, out: &Path) -> Result<i32> {
    let plan: Plan = manifest::json_file(plan_path)?;
    let base = plan_path.parent().unwrap_or(Path::new("."));
    validate(&plan, base)?;
    if out.exists() {
        return Err(invalid("ablation output already exists"));
    }
    std::fs::create_dir_all(out)?;
    report::json(&out.join("plan.json"), &plan)?;
    let mut all_gates = Vec::new();
    let mut edges = Vec::new();
    let mut exit = 0;
    for (di, d) in plan.datasets.iter().enumerate() {
        let loaded = manifest::load(&base.join(&d.suite))?;
        let mut summaries = BTreeMap::new();
        for vi in statistics::order(plan.variants.len(), plan.seed.wrapping_add(di as u64)) {
            let v = &plan.variants[vi];
            let path = out.join(format!("{}--{}", d.id, v.id));
            std::fs::create_dir(&path)?;
            let isolated = manifest::materialize(&loaded)?;
            let binary = base.join(&v.binary);
            let mut provenance = runner::engine_provenance(Some(&binary))?;
            provenance["ablation"] = json!({"matrix_plan_sha256":sha(&std::fs::read(plan_path)?),"variant":v.id,"enabled":v.enabled,"request_hints":d.hints,"build_receipt":manifest::json_file::<Value>(&base.join(&v.build_receipt))?,"scope":"isolated counterfactual; not production flags"});
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
                        WithHints {
                            client,
                            hints: d.hints.clone(),
                        },
                        isolated.path(),
                        &path,
                        provenance,
                        "ablation-diagnostic",
                    )
                    .await
                }
                Err(e) => Err(e),
            };
            match result {
                Ok(g) => {
                    exit = exit.max(g.exit_code);
                    all_gates.push(json!({"dataset":d.id,"variant":v.id,"gate":g}));
                    let s: report::Summary = manifest::json_file(&path.join("metrics.json"))?;
                    summaries.insert(v.id.clone(), s);
                }
                Err(e) => {
                    exit = 2;
                    let g = json!({"status":"invalid_measurement","exit_code":2,"reasons":[e.to_string()]});
                    report::json(&path.join("gate.json"), &g)?;
                    all_gates.push(json!({"dataset":d.id,"variant":v.id,"gate":g}));
                }
            }
        }
        for c in &plan.controls {
            for off in plan.variants.iter().filter(|v| !v.enabled.contains(&c.id)) {
                let mut enabled: BTreeSet<_> = off.enabled.iter().collect();
                enabled.insert(&c.id);
                let on = plan
                    .variants
                    .iter()
                    .find(|v| v.enabled.iter().collect::<BTreeSet<_>>() == enabled)
                    .expect("complete validated matrix");
                let (Some(a), Some(b)) = (summaries.get(&off.id), summaries.get(&on.id)) else {
                    continue;
                };
                let cases:Vec<Value>=loaded.queries.iter().map(|q|{
                    let left=a.cases.iter().find(|x|x.id==q.id);let right=b.cases.iter().find(|x|x.id==q.id);
                    json!({"id":q.id,"category":q.category,"language":q.language,"query_family":q.query_family,"delta_top1":left.zip(right).map(|(x,y)|y.top1-x.top1),"delta_ndcg10":left.zip(right).map(|(x,y)|y.ndcg10-x.ndcg10)})
                }).collect();
                edges.push(json!({"dataset":d.id,"factor":c.id,"off":off.id,"on":on.id,"fixed_enabled":off.enabled,"delta_top1":b.mean_top1-a.mean_top1,"delta_ndcg10":b.mean_ndcg10-a.mean_ndcg10,"cases":cases}));
            }
        }
    }
    // Re-check snapshots and binaries after execution; invalid provenance cannot pass.
    validate(&plan, base)?;
    report::json(
        &out.join("ablation.json"),
        &json!({"schema_version":1,"exit_code":exit,"status":"diagnostic_factorial_observations_not_release_certification","gates":all_gates,"one_factor_edges":edges,"limitations":["small authored cases are not holdout quality","build receipts are local provenance, not cryptographic source-to-binary attestation","counterfactuals use the same reference scaffold, not the entire historical product"]}),
    )?;
    Ok(exit)
}

/// A fixed-source, fixed-retriever comparison of the production chunk policy
/// against the isolated no-coalescing counterfactual. This deliberately does
/// not reuse the general binary factorial runner: both cells exercise the same
/// current parser, database, search engine and public result envelope, changing
/// only `merge_min_bytes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkPolicyAblationPlan {
    pub schema_version: u32,
    pub name: String,
    pub top_k: usize,
    pub policy: cc_model::chunk_policy::ChunkPolicy,
    pub candidate_merge_min_bytes: u32,
    pub files: BTreeMap<String, String>,
    pub queries: Vec<ChunkQualityQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkQualityQuery {
    pub id: String,
    pub query: String,
    pub expected_path: String,
    pub expected_symbol: Option<String>,
    pub required_facets: Vec<ChunkFacet>,
    pub min_facet_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChunkFacet {
    pub path: String,
    pub marker: String,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkCaseResult {
    id: String,
    top1_path: bool,
    reciprocal_rank: f64,
    symbol_top1: Option<bool>,
    facet_coverage: f64,
    evidence_valid: bool,
    duplication_rate: f64,
    hit_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct ChunkVariantResult {
    id: String,
    merge_min_bytes: u32,
    documents: usize,
    max_source_bytes: usize,
    source_partition_bytes: usize,
    admitted_source_bytes: usize,
    model_input_bytes: usize,
    manifest_json_bytes: usize,
    exact_source_partition: bool,
    build_elapsed_ms: u64,
    cases: Vec<ChunkCaseResult>,
}

fn validate_chunk_plan(plan: &ChunkPolicyAblationPlan) -> Result<()> {
    if plan.schema_version != 1
        || plan.name.is_empty()
        || !(1..=32).contains(&plan.top_k)
        || plan.files.is_empty()
        || plan.files.len() > 512
        || plan.queries.is_empty()
        || plan.queries.len() > 256
        || plan.policy.merge_min_bytes != 0
        || plan.candidate_merge_min_bytes == 0
        || plan.candidate_merge_min_bytes > 1_048_576
    {
        return Err(invalid("invalid chunk ablation bounds or factor"));
    }
    plan.policy
        .validate()
        .map_err(|e| invalid(format!("invalid baseline chunk policy: {e}")))?;
    let mut candidate = plan.policy;
    candidate.merge_min_bytes = plan.candidate_merge_min_bytes;
    candidate
        .validate()
        .map_err(|e| invalid(format!("invalid candidate chunk policy: {e}")))?;
    let mut source_bytes = 0usize;
    for (path, source) in &plan.files {
        super::validation::relative_path(path)?;
        if !cc_model::repo_path::is_canonical_file(path) || path.starts_with(".codecortex") {
            return Err(invalid("invalid admitted source path"));
        }
        source_bytes = source_bytes.saturating_add(source.len());
    }
    if source_bytes > 16 * 1024 * 1024 {
        return Err(invalid("chunk ablation source budget exceeded"));
    }
    let mut ids = BTreeSet::new();
    for query in &plan.queries {
        if query.id.is_empty()
            || !ids.insert(query.id.as_str())
            || query.query.trim().is_empty()
            || !plan.files.contains_key(&query.expected_path)
            || !(0.0..=1.0).contains(&query.min_facet_coverage)
            || query.required_facets.is_empty()
            || query.required_facets.len() > 32
        {
            return Err(invalid("invalid chunk quality query"));
        }
        for facet in &query.required_facets {
            let Some(source) = plan.files.get(&facet.path) else {
                return Err(invalid("facet path is not admitted"));
            };
            if facet.marker.is_empty() || !source.contains(&facet.marker) {
                return Err(invalid("facet marker is not independently present"));
            }
        }
    }
    Ok(())
}

fn open_document_records(root: &Path) -> Result<Vec<cc_model::identity::DocumentRecord>> {
    let connection = rusqlite::Connection::open(root.join(".codecortex/index.sqlite3"))
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    let mut statement = connection
        .prepare("SELECT record_json FROM document_manifest ORDER BY file_path,doc_key")
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    let mut records = Vec::new();
    for row in rows {
        let raw = row.map_err(|e| BenchError::Protocol(e.to_string()))?;
        records.push(serde_json::from_str(&raw)?);
    }
    Ok(records)
}

fn exact_partition(
    files: &BTreeMap<String, String>,
    records: &[cc_model::identity::DocumentRecord],
) -> bool {
    files.iter().all(|(path, source)| {
        let mut spans: Vec<_> = records
            .iter()
            .filter(|record| &record.file_path == path)
            .map(|record| record.source.span)
            .collect();
        spans.sort_by_key(|span| (span.start, span.end));
        let mut position = 0usize;
        for span in spans {
            if span.start != position
                || span.end > source.len()
                || !source.is_char_boundary(span.start)
                || !source.is_char_boundary(span.end)
            {
                return false;
            }
            position = span.end;
        }
        position == source.len()
    })
}

fn hit_span(hit: &Value, files: &BTreeMap<String, String>) -> Result<(String, usize, usize)> {
    let path = hit
        .get("file_path")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("hit path missing"))?;
    let source = files
        .get(path)
        .ok_or_else(|| invalid("hit escaped admitted source"))?;
    let evidence = hit
        .pointer("/metadata/source_evidence")
        .ok_or_else(|| invalid("verified source evidence missing"))?;
    let start = evidence
        .pointer("/span/start")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid("source span start missing"))?;
    let end = evidence
        .pointer("/span/end")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| invalid("source span end missing"))?;
    let text = hit
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("hit text missing"))?;
    let exact = source
        .get(start..end)
        .ok_or_else(|| invalid("hit span outside source"))?;
    let digest = evidence
        .get("slice_digest")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("slice digest missing"))?;
    if exact != text
        || blake3::hash(exact.as_bytes()).to_hex().as_str() != digest
        || hit
            .pointer("/metadata/source_freshness/status")
            .and_then(Value::as_str)
            != Some("current_verified")
    {
        return Err(invalid("hit source bytes, digest or freshness mismatch"));
    }
    Ok((path.to_owned(), start, end))
}

fn duplication_rate(spans: &[(String, usize, usize)]) -> f64 {
    let total: usize = spans.iter().map(|(_, start, end)| end - start).sum();
    if total == 0 {
        return 0.0;
    }
    let mut by_path: BTreeMap<&str, Vec<(usize, usize)>> = BTreeMap::new();
    for (path, start, end) in spans {
        by_path.entry(path).or_default().push((*start, *end));
    }
    let mut union = 0usize;
    for intervals in by_path.values_mut() {
        intervals.sort_unstable();
        let mut current: Option<(usize, usize)> = None;
        for &(start, end) in intervals.iter() {
            current = match current {
                None => Some((start, end)),
                Some((a, z)) if start <= z => Some((a, z.max(end))),
                Some((a, z)) => {
                    union += z - a;
                    Some((start, end))
                }
            };
        }
        if let Some((start, end)) = current {
            union += end - start;
        }
    }
    (total.saturating_sub(union)) as f64 / total as f64
}

fn evaluate_chunk_variant(
    plan: &ChunkPolicyAblationPlan,
    id: &str,
    merge_min_bytes: u32,
) -> Result<ChunkVariantResult> {
    let root = tempfile::tempdir()?;
    let mut policy = plan.policy;
    policy.merge_min_bytes = merge_min_bytes;
    let config = json!({
        "auto_index": {"enabled": false},
        "indexing": {
            "chunk_line_budget": policy.lines,
            "chunk_byte_budget": policy.bytes,
            "chunk_char_budget": policy.chars,
            "chunk_token_budget": policy.estimated_tokens,
            "chunk_merge_min_bytes": policy.merge_min_bytes
        }
    });
    std::fs::write(
        root.path().join(".codecortex.json"),
        serde_json::to_vec(&config)?,
    )?;
    for (path, source) in &plan.files {
        let destination = root.path().join(path);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, source.as_bytes())?;
    }
    let mut index = cc_server::engine::CodeIndex::new(Some(root.path()))
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    let report = index
        .build_index(true)
        .map_err(|e| BenchError::Protocol(e.to_string()))?;
    if !report.parse_errors.is_empty() || !report.resolution_freshness.complete {
        return Err(invalid(
            "chunk ablation build is not a complete comparable input",
        ));
    }
    let records = open_document_records(root.path())?;
    let exact_source_partition = exact_partition(&plan.files, &records);
    if !exact_source_partition {
        return Err(invalid(
            "document chunks do not exactly partition admitted sources",
        ));
    }
    let max_source_bytes = records
        .iter()
        .map(|record| record.source.span.len())
        .max()
        .unwrap_or(0);
    if max_source_bytes > policy.effective_bytes() {
        return Err(invalid("chunk exceeds configured effective byte budget"));
    }
    let source_partition_bytes = records.iter().map(|record| record.source.span.len()).sum();
    let model_input_bytes = records
        .iter()
        .filter_map(|record| record.input.as_ref())
        .map(|input| input.text.len())
        .sum();
    let manifest_json_bytes = records
        .iter()
        .map(|record| serde_json::to_vec(record).map(|row| row.len()))
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .sum();
    let mut cases = Vec::new();
    for query in &plan.queries {
        let envelope = index
            .search()
            .search_in_context(&query.query, plan.top_k, None)
            .map_err(|e| BenchError::Tool(e.to_string()))?;
        let hits = envelope
            .machine_pack
            .get("hits")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("public hit array missing"))?;
        let path_rank = hits.iter().position(|hit| {
            hit.get("file_path").and_then(Value::as_str) == Some(&query.expected_path)
        });
        let reciprocal_rank = path_rank.map_or(0.0, |rank| 1.0 / (rank + 1) as f64);
        let symbol_top1 = query.expected_symbol.as_ref().map(|symbol| {
            hits.first()
                .and_then(|hit| hit.get("symbol_name"))
                .and_then(Value::as_str)
                == Some(symbol.as_str())
        });
        let facet_hits = query
            .required_facets
            .iter()
            .filter(|facet| {
                hits.iter().any(|hit| {
                    hit.get("file_path").and_then(Value::as_str) == Some(facet.path.as_str())
                        && hit
                            .get("text")
                            .and_then(Value::as_str)
                            .is_some_and(|text| text.contains(&facet.marker))
                })
            })
            .count();
        let facet_coverage = facet_hits as f64 / query.required_facets.len() as f64;
        let mut spans = Vec::new();
        let mut evidence_valid = true;
        for hit in hits {
            match hit_span(hit, &plan.files) {
                Ok(span) => spans.push(span),
                Err(_) => evidence_valid = false,
            }
        }
        let case = ChunkCaseResult {
            id: query.id.clone(),
            top1_path: path_rank == Some(0),
            reciprocal_rank,
            symbol_top1,
            facet_coverage,
            evidence_valid,
            duplication_rate: duplication_rate(&spans),
            hit_count: hits.len(),
        };
        if !case.top1_path
            || case.symbol_top1 == Some(false)
            || case.facet_coverage + f64::EPSILON < query.min_facet_coverage
            || !case.evidence_valid
        {
            return Err(invalid(format!(
                "authored quality contract failed: {}",
                query.id
            )));
        }
        cases.push(case);
    }
    Ok(ChunkVariantResult {
        id: id.into(),
        merge_min_bytes,
        documents: records.len(),
        max_source_bytes,
        source_partition_bytes,
        admitted_source_bytes: plan.files.values().map(String::len).sum(),
        model_input_bytes,
        manifest_json_bytes,
        exact_source_partition,
        build_elapsed_ms: report.elapsed_ms,
        cases,
    })
}

/// Run the P4-D chunk quality experiment. The return value is entirely derived
/// from admitted source, current public search output and persisted manifests.
pub fn run_chunk_policy_ablation(plan_path: &Path) -> Result<Value> {
    let plan: ChunkPolicyAblationPlan = manifest::json_file(plan_path)?;
    validate_chunk_plan(&plan)?;
    let baseline = evaluate_chunk_variant(&plan, "merge_disabled", 0)?;
    let candidate =
        evaluate_chunk_variant(&plan, "production_merge", plan.candidate_merge_min_bytes)?;
    let mut regressions = Vec::new();
    let mut comparisons = Vec::new();
    let mut source_evidence_failures = 0usize;
    for (before, after) in baseline.cases.iter().zip(&candidate.cases) {
        if before.id != after.id {
            return Err(invalid("variant case ordering drift"));
        }
        let regressed = (!after.top1_path && before.top1_path)
            || after.reciprocal_rank + f64::EPSILON < before.reciprocal_rank
            || after.facet_coverage + f64::EPSILON < before.facet_coverage
            || (before.symbol_top1 == Some(true) && after.symbol_top1 != Some(true));
        if regressed {
            regressions.push(before.id.clone());
        }
        source_evidence_failures += usize::from(!before.evidence_valid);
        source_evidence_failures += usize::from(!after.evidence_valid);
        comparisons.push(json!({
            "id": before.id,
            "top1_delta": i8::from(after.top1_path) - i8::from(before.top1_path),
            "reciprocal_rank_delta": after.reciprocal_rank - before.reciprocal_rank,
            "facet_coverage_delta": after.facet_coverage - before.facet_coverage,
            "duplication_rate_delta": after.duplication_rate - before.duplication_rate,
            "regressed": regressed,
        }));
    }
    if !regressions.is_empty() || source_evidence_failures > 0 {
        return Err(invalid(format!(
            "chunk quality/source gate failed: regressions={regressions:?}, source_failures={source_evidence_failures}"
        )));
    }
    if candidate.documents >= baseline.documents {
        return Err(invalid(
            "controlled merge factor did not reduce the authored document partition",
        ));
    }
    let reduction = baseline.documents - candidate.documents;
    Ok(json!({
        "schema_version": 1,
        "name": plan.name,
        "status": "passed",
        "factor": {
            "field": "chunk_policy.merge_min_bytes",
            "baseline": 0,
            "candidate": plan.candidate_merge_min_bytes,
            "all_other_policy_fields_fixed": true,
            "retriever_fixed": "current CodeIndex public hybrid search"
        },
        "baseline": baseline,
        "candidate": candidate,
        "comparison": {
            "quality_regressions": regressions.len(),
            "source_evidence_failures": source_evidence_failures,
            "candidate_document_reduction": reduction,
            "cases": comparisons,
        },
        "limitations": [
            "authored deterministic development corpus, not public holdout or general semantic quality certification",
            "in-process public engine envelope, with real SQLite/parser/search but not an MCP transport measurement",
            "document reduction is reported only after path/symbol/facet/source evidence gates; it is not itself a quality result",
            "no embedding provider, tokenizer, RSS, 100k scale or tail-latency claim"
        ]
    }))
}

#[cfg(test)]
#[path = "ablation_tests.rs"]
mod tests;
