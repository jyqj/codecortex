//! Same-product public strategy comparisons. `semantic` still includes local
//! lanes; this harness never relabels it dense-only or certifies live quality.
pub mod reporting;
use crate::benchmark::{
    adapters::{mcp_stdio::McpStdio, Backend},
    invalid, manifest,
    readiness::Readiness,
    report, runner,
    schema::{SearchInput, Suite},
    statistics, BenchError, Result,
};
use cc_model::query::{QueryConfig, RetrievalStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

pub const SPEC: &str = "public-strategy-ablation-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Disabled,
    /// Engineering fixtures only: literal loopback IP, explicit port, HTTP.
    LoopbackOnly,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyPolicy {
    pub expected_space: Option<String>,
    pub timeout_ms: u64,
    pub poll_interval_ms: u64,
    pub max_polls: usize,
}
impl ReadyPolicy {
    fn validate(&self) -> Result<()> {
        if !(1..=600_000).contains(&self.timeout_ms)
            || !(10..=1_000).contains(&self.poll_interval_ms)
            || !(1..=1_024).contains(&self.max_polls)
            || self.expected_space.as_ref().is_some_and(|space| {
                space.len() != 64 || !space.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        {
            return Err(invalid("strategy readiness limits or expected space"));
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema_version: u32,
    pub suite: PathBuf,
    /// Complete isolated source snapshot, checked against the build receipt.
    pub source_snapshot: PathBuf,
    pub binary: PathBuf,
    pub build_receipt: PathBuf,
    pub strategies: Vec<RetrievalStrategy>,
    pub network: Network,
    pub readiness: ReadyPolicy,
}
fn strategy_name(strategy: RetrievalStrategy) -> &'static str {
    match strategy {
        RetrievalStrategy::Local => "local",
        RetrievalStrategy::Auto => "auto",
        RetrievalStrategy::Semantic => "semantic",
    }
}
fn loopback_endpoint(endpoint: &str) -> bool {
    let Some(rest) = endpoint.strip_prefix("http://") else {
        return false;
    };
    if rest
        .chars()
        .any(|character| matches!(character, '?' | '#' | '@' | '\\'))
    {
        return false;
    }
    rest.split('/').next().is_some_and(|authority| {
        authority
            .parse::<SocketAddr>()
            .is_ok_and(|address| address.ip().is_loopback() && address.port() != 0)
    })
}
/// Validate the explicit strategy/configuration comparison before any process
/// starts. The same locked suite/config/binary is used by every cell.
pub fn validate_configuration(plan: &Plan, suite: &Suite) -> Result<QueryConfig> {
    let names: BTreeSet<_> = plan.strategies.iter().copied().map(strategy_name).collect();
    if plan.schema_version != 1
        || !(2..=3).contains(&plan.strategies.len())
        || names.len() != plan.strategies.len()
        || !names.contains("local")
        || !(1..=200).contains(&suite.top_k)
    {
        return Err(invalid("strategy comparison version, cells or top_k"));
    }
    plan.readiness.validate()?;
    let config: cc_model::config::ProjectConfig =
        serde_json::from_value(suite.engine_config.clone())?;
    config
        .query
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    if config.auto_index.enabled {
        return Err(invalid("strategy comparison requires explicit indexing"));
    }
    match plan.network {
        Network::Disabled if config.semantic.enabled => {
            return Err(invalid(
                "disabled-network comparison cannot enable semantic",
            ));
        }
        Network::LoopbackOnly
            if !config.semantic.enabled
                || !config.semantic.network_opt_in
                || !config.semantic.allow_http
                || !loopback_endpoint(&config.semantic.endpoint) =>
        {
            return Err(invalid(
                "semantic engineering run requires explicit literal-loopback configuration",
            ));
        }
        _ => {}
    }
    if plan.readiness.expected_space.is_some() != config.semantic.enabled
        || (names.contains("semantic") && plan.readiness.expected_space.is_none())
    {
        return Err(invalid(
            "configured semantic comparison requires a pinned ready space",
        ));
    }
    Ok(config.query)
}

fn protocol(message: impl Into<String>) -> BenchError {
    BenchError::Protocol(message.into())
}
fn number(value: &Value, key: &str) -> Result<u64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| protocol(format!("missing or invalid {key}")))
}
fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| protocol(format!("missing or invalid {key}")))
}
/// Ready means both the admitted file set and, when requested, the actual
/// current dense space. File counts never masquerade as vector counts.
pub fn status_is_ready(status: &Value, files: usize, policy: &ReadyPolicy) -> Result<bool> {
    if files == 0 {
        return Err(invalid("empty strategy input"));
    }
    let indexed = number(status, "indexed_files")?;
    if indexed > files as u64 {
        return Err(protocol("indexed files exceed admitted input"));
    }
    let retrieval = &status["retrieval"];
    if retrieval["spec"] != "retrieval-capabilities-v2"
        || retrieval["consistency"] != "point_in_time"
        || retrieval["generation_scope"] != "observed_database_snapshot"
        || retrieval["identity_validation"] != "checked_at_observation_boundary"
    {
        return Err(protocol(
            "strategy readiness lacks a checked capability observation",
        ));
    }
    if retrieval["local_state"] != "available" || indexed != files as u64 {
        return Ok(false);
    }
    let Some(space) = &policy.expected_space else {
        return Ok(true);
    };
    if let Some(observed) = retrieval["semantic_active_space"].as_str() {
        if observed != space {
            return Err(protocol(
                "strategy readiness observed a different semantic space",
            ));
        }
    } else {
        return Ok(false);
    }
    let desired = number(retrieval, "dense_desired")?;
    let published = number(retrieval, "dense_published")?;
    let pending = number(retrieval, "semantic_pending")?;
    let failed = number(retrieval, "semantic_failed")?;
    if published > desired {
        return Err(protocol("dense publication exceeds desired coverage"));
    }
    if failed > 0
        || retrieval["semantic_state"] == "failed"
        || retrieval["semantic_state"] == "degraded"
    {
        return Err(protocol("semantic readiness failed or degraded"));
    }
    Ok(desired > 0
        && published == desired
        && pending == 0
        && retrieval["semantic_state"] == "ready"
        && retrieval["dense_state"] == "ready")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveBudget {
    pub policy_version: String,
    pub deadline_ms: u64,
    pub lane_timeout_ms: u64,
    pub semantic_timeout_ms: u64,
    pub semantic_top_k: u64,
    pub token_budget: u64,
    pub configured_max_bytes: u64,
    pub limit_bytes: u64,
    pub packing_spec: String,
    pub scope_policy: String,
    /// Only the documented prose fields `units` and `semantics` are omitted
    /// from equality; compaction changes those labels, never these values.
    pub candidate_budget: Value,
    pub hard_scope: Value,
}
fn scope_identity(scope: &Value, key: &str, fields: &[&str], prose: &str) -> Result<Value> {
    let map = scope[key]
        .as_object()
        .ok_or_else(|| protocol(format!("missing scope {key}")))?;
    if fields.iter().any(|field| !map.contains_key(*field))
        || map
            .keys()
            .any(|field| !fields.contains(&field.as_str()) && field != prose)
    {
        return Err(protocol(format!(
            "incomplete or unknown scope {key} fields"
        )));
    }
    let mut identity = map.clone();
    identity.remove(prose);
    Ok(Value::Object(identity))
}
/// Verify the response without modifying it. Public output capacity and source
/// scope are part of comparison identity, in addition to the query budgets.
pub fn response_budget(
    raw: &Value,
    input: &SearchInput,
    strategy: RetrievalStrategy,
    query: &QueryConfig,
    dense_configured: bool,
) -> Result<EffectiveBudget> {
    let retrieval = &raw["evidence_summary"]["retrieval"];
    let policy = &retrieval["policy"];
    let effective = if strategy == RetrievalStrategy::Auto && !dense_configured {
        "local"
    } else {
        strategy_name(strategy)
    };
    if policy["requested"] != strategy_name(strategy) || policy["effective"] != effective {
        return Err(protocol(
            "requested/effective retrieval strategy was not preserved",
        ));
    }
    let deadline = number(policy, "deadline_ms")?;
    let lane = number(policy, "lane_timeout_ms")?;
    let semantic = number(policy, "semantic_timeout_ms")?;
    let semantic_top_k = number(policy, "semantic_top_k")?;
    if deadline != query.deadline_ms
        || lane != query.lane_timeout_ms.min(query.deadline_ms)
        || semantic != query.semantic_timeout_ms.min(query.deadline_ms)
        || semantic_top_k != query.semantic_top_k as u64
    {
        return Err(protocol(
            "effective query budgets differ from the locked configuration",
        ));
    }
    let scope = &retrieval["scope"];
    if scope["schema_version"] != 1 {
        return Err(protocol("unsupported source-scope receipt"));
    }
    let candidate_fields = [
        "top_k",
        "exact_symbol_candidates",
        "path_candidates",
        "lexical_candidates",
        "grep_candidates",
        "grep_scan_cap",
        "rerank_window",
        "grep_enabled",
    ];
    let candidate_budget = scope_identity(scope, "budget", &candidate_fields, "units")?;
    for field in candidate_fields
        .into_iter()
        .filter(|field| *field != "grep_enabled")
    {
        number(&candidate_budget, field)?;
    }
    if candidate_budget["grep_enabled"].as_bool().is_none()
        || candidate_budget["top_k"] != input.top_k as u64
    {
        return Err(protocol(
            "effective source candidate budget differs from request",
        ));
    }
    let hard_scope = scope_identity(
        scope,
        "hard",
        &[
            "path_prefix",
            "path_prefix_truncated",
            "languages",
            "explicit_file_count",
            "empty",
        ],
        "semantics",
    )?;
    if hard_scope["path_prefix_truncated"] != false || hard_scope["empty"].as_bool().is_none() {
        return Err(protocol("source scope cannot be completely compared"));
    }
    // Deserialize the actual public model for field types, retaining its
    // canonical scope values. Do not infer an inventory from a count.
    serde_json::from_value::<cc_model::context::SearchHardScopeExplain>(scope["hard"].clone())?;
    let packing = &raw["evidence_summary"]["packing"];
    let configured_max_bytes = number(packing, "configured_max_bytes")?;
    let limit_bytes = number(packing, "limit_bytes")?;
    let token_budget = number(raw, "token_budget")?;
    let token_bytes = token_budget
        .checked_mul(4)
        .ok_or_else(|| protocol("token budget overflow"))?;
    let raw_bytes = serde_json::to_vec(raw)?.len() as u64;
    if packing["spec"] != cc_model::context::CONTEXT_PACKING_SPEC
        || limit_bytes < 1_024
        || limit_bytes != configured_max_bytes.min(token_bytes)
        || number(packing, "used_bytes")? != raw_bytes
        || raw_bytes > limit_bytes
        || number(raw, "token_estimate")? != raw_bytes.div_ceil(4)
        || packing["partial"].as_bool().is_none()
    {
        return Err(protocol("invalid final whole-output budget receipt"));
    }
    // Keep original partial/error/coverage states. This validation also rejects
    // contradictory full/projected lane receipts; it does not demand a hit.
    crate::benchmark::normalizer::mcp(raw)?;
    if (effective != "local") == semantic_observation(raw).is_null() {
        return Err(protocol(
            "semantic lane presence disagrees with effective policy",
        ));
    }
    Ok(EffectiveBudget {
        policy_version: required_text(policy, "version")?.into(),
        deadline_ms: deadline,
        lane_timeout_ms: lane,
        semantic_timeout_ms: semantic,
        semantic_top_k,
        token_budget,
        configured_max_bytes,
        limit_bytes,
        packing_spec: required_text(packing, "spec")?.into(),
        scope_policy: required_text(scope, "policy")?.into(),
        candidate_budget,
        hard_scope,
    })
}

fn semantic_observation(raw: &Value) -> Value {
    let retrieval = &raw["evidence_summary"]["retrieval"];
    let lane = ["lanes", "lane_receipts"].iter().find_map(|key| {
        retrieval[*key]
            .as_array()
            .and_then(|lanes| lanes.iter().find(|lane| lane["lane_id"] == "semantic"))
    });
    match lane {
        Some(lane) => json!({
            "lane_id":lane["lane_id"], "status":lane["status"],
            "candidate_count":lane["candidate_count"], "coverage":lane["coverage"],
            "truncation_reason":lane["truncation_reason"]
        }),
        None => Value::Null,
    }
}

/// Minimal public RPC boundary, implemented by the real stdio client. Test
/// doubles only exercise protocol controls and are never quality evidence.
#[allow(async_fn_in_trait)]
pub trait PublicRpc {
    fn pid(&self) -> Option<u32> {
        None
    }
    async fn call(&mut self, tool: &str, args: Value) -> Result<Value>;
    async fn close(&mut self) -> Result<()>;
}
impl PublicRpc for McpStdio {
    fn pid(&self) -> Option<u32> {
        Backend::pid(self)
    }
    async fn call(&mut self, tool: &str, args: Value) -> Result<Value> {
        McpStdio::call(self, tool, args).await
    }
    async fn close(&mut self) -> Result<()> {
        Backend::close(self).await
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryObservation {
    pub sequence: usize,
    pub input: SearchInput,
    pub input_digest: String,
    pub raw_path: String,
    pub raw_sha256: String,
    pub requested: RetrievalStrategy,
    pub effective: Option<String>,
    pub budget: Option<EffectiveBudget>,
    pub semantic_coverage: Value,
    /// The original diagnostics, with missing values left null. They describe
    /// originating work and do not establish this cache-hit request's cost.
    pub originating_cost: Value,
    pub validation_work: Value,
    pub error: Option<String>,
}
fn append(path: &Path, row: &impl Serialize) -> Result<()> {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    serde_json::to_writer(&mut file, row)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}
pub struct ProfileBackend<C> {
    client: C,
    project: PathBuf,
    out: PathBuf,
    strategy: RetrievalStrategy,
    query: QueryConfig,
    readiness: ReadyPolicy,
    queries: usize,
    files: usize,
}
impl<C: PublicRpc> ProfileBackend<C> {
    pub fn new(
        client: C,
        project: PathBuf,
        out: PathBuf,
        strategy: RetrievalStrategy,
        query: QueryConfig,
        readiness: ReadyPolicy,
    ) -> Result<Self> {
        readiness.validate()?;
        query
            .validate()
            .map_err(|error| invalid(error.to_string()))?;
        std::fs::create_dir(out.join("profile-raw"))?;
        Ok(Self {
            client,
            project,
            out,
            strategy,
            query,
            readiness,
            queries: 0,
            files: 0,
        })
    }
    async fn observe_status(&mut self, phase: &str) -> Result<bool> {
        let raw = self
            .client
            .call("status", json!({"aspect":"capabilities"}))
            .await?;
        let result = status_is_ready(&raw, self.files, &self.readiness);
        append(
            &self.out.join("profile-readiness.jsonl"),
            &json!({"phase":phase,"raw":raw,
            "ready":result.as_ref().ok(),"error":result.as_ref().err().map(ToString::to_string)}),
        )?;
        result
    }
}
impl<C: PublicRpc> Backend for ProfileBackend<C> {
    fn name(&self) -> &'static str {
        SPEC
    }
    fn pid(&self) -> Option<u32> {
        self.client.pid()
    }
    async fn prepare(&mut self, files: &[manifest::FileRecord]) -> Result<Value> {
        self.files = files.len();
        self.client
            .call("index", json!({"path":self.project,"full":true}))
            .await
    }
    async fn readiness(&mut self, files: &[manifest::FileRecord]) -> Result<Readiness> {
        self.files = files.len();
        let deadline =
            tokio::time::Instant::now() + Duration::from_millis(self.readiness.timeout_ms);
        for poll in 0..self.readiness.max_polls {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let ready = tokio::time::timeout(remaining, self.observe_status("before_queries"))
                .await
                .map_err(|_| BenchError::Timeout("strategy readiness deadline".into()))??;
            if ready {
                return Readiness::new(files.len(), files.len(), 0, 0, 0);
            }
            if poll + 1 == self.readiness.max_polls || tokio::time::Instant::now() >= deadline {
                break;
            }
            tokio::time::sleep(
                Duration::from_millis(self.readiness.poll_interval_ms)
                    .min(deadline.saturating_duration_since(tokio::time::Instant::now())),
            )
            .await;
        }
        Err(BenchError::Timeout(
            "strategy readiness did not converge within declared bounds".into(),
        ))
    }
    async fn search(&mut self, input: &SearchInput) -> Result<Value> {
        let mut args = json!({"query":input.query,"top_k":input.top_k,"mode":"hybrid",
            "retrieval_strategy":strategy_name(self.strategy)});
        if let Some(prefix) = &input.path_prefix {
            args["path_prefix"] = json!(prefix);
        }
        let raw = self.client.call("search", args).await?;
        let raw_path = format!("profile-raw/{:06}.json", self.queries);
        let bytes = serde_json::to_vec(&raw)?;
        // Preserve the real response before rejecting a strategy or budget
        // mismatch. The generic runner otherwise retains only the error text.
        std::fs::write(self.out.join(&raw_path), &bytes)?;
        let budget = response_budget(
            &raw,
            input,
            self.strategy,
            &self.query,
            self.readiness.expected_space.is_some(),
        );
        let observation = QueryObservation {
            sequence: self.queries,
            input: input.clone(),
            input_digest: super::sha(&serde_json::to_vec(input)?),
            raw_path,
            raw_sha256: super::sha(&bytes),
            requested: self.strategy,
            effective: raw
                .pointer("/evidence_summary/retrieval/policy/effective")
                .and_then(Value::as_str)
                .map(str::to_owned),
            budget: budget.as_ref().ok().cloned(),
            semantic_coverage: semantic_observation(&raw),
            originating_cost: raw
                .pointer("/evidence_summary/retrieval/cost")
                .cloned()
                .unwrap_or(Value::Null),
            validation_work: raw
                .pointer("/evidence_summary/source_freshness/validation_work")
                .cloned()
                .unwrap_or(Value::Null),
            error: budget.as_ref().err().map(ToString::to_string),
        };
        append(&self.out.join("profile-queries.jsonl"), &observation)?;
        self.queries += 1;
        budget?;
        Ok(raw)
    }
    async fn close(&mut self) -> Result<()> {
        // A final checked observation rejects a model/coverage change during
        // the cell; it does not claim the whole run was one atomic snapshot.
        let final_state = if self.files > 0 {
            self.observe_status("after_queries").await
        } else {
            Ok(true)
        };
        let closed = self.client.close().await;
        if !final_state? {
            return Err(protocol("strategy coverage changed before close"));
        }
        closed
    }
}

/// Recompute serialized assertions from the exact saved product response.
/// Sequence-derived relative names reject traversal in the recorded path.
pub fn read_observations(
    cell: &Path,
    strategy: RetrievalStrategy,
    query: &QueryConfig,
    dense_configured: bool,
) -> Result<Vec<QueryObservation>> {
    let path = cell.join("profile-queries.jsonl");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let rows: Vec<QueryObservation> = report::read_jsonl(&path)?;
    for (sequence, row) in rows.iter().enumerate() {
        if row.sequence != sequence
            || row.raw_path != format!("profile-raw/{sequence:06}.json")
            || row.requested != strategy
            || row.input_digest != super::sha(&serde_json::to_vec(&row.input)?)
        {
            return Err(protocol("strategy observation identity drift"));
        }
        let bytes = std::fs::read(cell.join(&row.raw_path))?;
        if super::sha(&bytes) != row.raw_sha256 {
            return Err(protocol("strategy raw response digest drift"));
        }
        let raw: Value = serde_json::from_slice(&bytes)?;
        let budget = response_budget(&raw, &row.input, strategy, query, dense_configured);
        let effective = raw
            .pointer("/evidence_summary/retrieval/policy/effective")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let cost = raw
            .pointer("/evidence_summary/retrieval/cost")
            .cloned()
            .unwrap_or(Value::Null);
        let validation_work = raw
            .pointer("/evidence_summary/source_freshness/validation_work")
            .cloned()
            .unwrap_or(Value::Null);
        if row.budget != budget.as_ref().ok().cloned()
            || row.error != budget.as_ref().err().map(ToString::to_string)
            || row.effective != effective
            || row.semantic_coverage != semantic_observation(&raw)
            || row.originating_cost != cost
            || row.validation_work != validation_work
        {
            return Err(protocol(
                "strategy observation assertions differ from raw response",
            ));
        }
    }
    Ok(rows)
}

/// Compare all admitted per-input budgets, including repeated and warm-up
/// requests. No row is silently removed when a profile fails or is partial.
pub fn compare_budgets(
    cells: &BTreeMap<String, Vec<QueryObservation>>,
) -> Result<BTreeMap<String, EffectiveBudget>> {
    let mut expected: BTreeMap<String, EffectiveBudget> = BTreeMap::new();
    let mut expected_counts = None;
    for (name, rows) in cells {
        let mut counts = BTreeMap::new();
        for row in rows {
            if row.error.is_some() {
                return Err(protocol(format!("{name} contains rejected observations")));
            }
            let budget = row
                .budget
                .as_ref()
                .ok_or_else(|| protocol("missing effective budget"))?;
            let digest = super::sha(&serde_json::to_vec(&row.input)?);
            if digest != row.input_digest {
                return Err(protocol("strategy input digest drift"));
            }
            *counts.entry(digest.clone()).or_insert(0usize) += 1;
            if let Some(prior) = expected.get(&digest) {
                if prior != budget {
                    return Err(protocol(format!(
                        "unequal effective budget/source scope for input {digest}"
                    )));
                }
            } else {
                expected.insert(digest, budget.clone());
            }
        }
        if counts.is_empty() {
            return Err(protocol(format!("{name} has no observations")));
        }
        match &expected_counts {
            None => expected_counts = Some(counts),
            Some(prior) if prior != &counts => {
                return Err(protocol("strategy cells have unequal input populations"))
            }
            _ => {}
        }
    }
    if cells.len() < 2 {
        return Err(invalid("strategy comparison needs at least two cells"));
    }
    Ok(expected)
}

/// Equality between two truncated runs is not a complete experiment. Compare
/// their populations to the locked suite, including each warm-up request.
pub fn compare_population(
    cells: &BTreeMap<String, Vec<QueryObservation>>,
    inputs: &[SearchInput],
    repetitions_and_warmups: usize,
) -> Result<()> {
    if inputs.is_empty() || repetitions_and_warmups == 0 {
        return Err(invalid("empty expected strategy population"));
    }
    let mut expected = BTreeMap::new();
    for input in inputs {
        *expected
            .entry(super::sha(&serde_json::to_vec(input)?))
            .or_insert(0usize) += repetitions_and_warmups;
    }
    for (name, rows) in cells {
        let mut observed = BTreeMap::new();
        for row in rows {
            *observed.entry(row.input_digest.clone()).or_insert(0usize) += 1;
        }
        if observed != expected {
            return Err(protocol(format!(
                "{name} does not contain the complete locked input population"
            )));
        }
    }
    Ok(())
}

pub async fn run(plan_path: &Path, out: &Path) -> Result<i32> {
    let plan: Plan = manifest::json_file(plan_path)?;
    let base = plan_path.parent().unwrap_or(Path::new("."));
    let loaded = manifest::load(&base.join(&plan.suite))?;
    let query = validate_configuration(&plan, &loaded.suite)?;
    let receipt: super::BuildReceipt = manifest::json_file(&base.join(&plan.build_receipt))?;
    let source = super::inventory(&base.join(&plan.source_snapshot))?;
    if source.is_empty() || source != receipt.source_files || receipt.exit_code != 0 {
        return Err(invalid("strategy source/build receipt mismatch"));
    }
    super::projected_semantic_identity(&receipt.build_options)?;
    let binary = base.join(&plan.binary).canonicalize()?;
    if super::sha(&std::fs::read(&binary)?) != receipt.binary_sha256 {
        return Err(invalid("strategy product binary digest drift"));
    }
    if out.exists() {
        return Err(invalid(
            "strategy output already exists; old raw is immutable",
        ));
    }
    std::fs::create_dir_all(out)?;
    report::json(&out.join("plan.json"), &plan)?;
    report::json(&out.join("product-build-receipt.json"), &receipt)?;
    let mut cells = BTreeMap::new();
    let mut gates = BTreeMap::new();
    let mut summaries = BTreeMap::new();
    let mut cost_columns = BTreeMap::new();
    let profile = if plan.network == Network::LoopbackOnly {
        "fake"
    } else {
        "engineering_offline"
    };
    let mut exit = 0;
    for index in statistics::order(plan.strategies.len(), loaded.suite.seed) {
        let strategy = plan.strategies[index];
        let name = strategy_name(strategy);
        let cell = out.join(name);
        std::fs::create_dir(&cell)?;
        let isolated = manifest::materialize(&loaded)?;
        if super::sha(&std::fs::read(&binary)?) != receipt.binary_sha256 {
            return Err(invalid("strategy product changed between cells"));
        }
        let mut provenance = runner::engine_provenance(Some(&binary))?;
        provenance["strategy_ablation"] = json!({"spec":SPEC,"requested":strategy,
            "profile":profile,"effect_interpretation":"none; engineering mechanism only",
            "product_build_receipt":receipt,"network":plan.network,"readiness":plan.readiness,
            "same_locked_input_and_config":true,"query_budget":query,
            "semantic_meaning":"local_lanes_plus_required_semantic; not_dense_only"});
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
                    ProfileBackend::new(
                        client,
                        isolated.path().to_path_buf(),
                        cell.clone(),
                        strategy,
                        query.clone(),
                        plan.readiness.clone(),
                    )?,
                    isolated.path(),
                    &cell,
                    provenance,
                    profile,
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
                summaries.insert(name.to_owned(), summary);
                gates.insert(name.to_owned(), gate);
            }
            Err(error) => {
                exit = 2;
                report::json(
                    &cell.join("failure.json"),
                    &json!({"error":error.to_string()}),
                )?;
                gates.insert(
                    name.to_owned(),
                    crate::benchmark::gate::Gate {
                        status: "invalid_measurement".into(),
                        exit_code: 2,
                        reasons: vec![error.to_string()],
                    },
                );
            }
        }
        let rows = read_observations(
            &cell,
            strategy,
            &query,
            plan.readiness.expected_space.is_some(),
        )?;
        report::jsonl(
            &cell.join("strategy-costs.jsonl"),
            &reporting::cost_rows(&rows, profile),
        )?;
        cost_columns.insert(name.to_owned(), reporting::cost_columns(&rows, profile));
        cells.insert(name.to_owned(), rows);
    }
    if super::inventory(&base.join(&plan.source_snapshot))? != source
        || super::sha(&std::fs::read(&binary)?) != receipt.binary_sha256
    {
        return Err(invalid(
            "strategy source or product binary changed during the comparison",
        ));
    }
    let expected_inputs = loaded
        .queries
        .iter()
        .map(|q| q.input(loaded.suite.top_k))
        .collect::<Vec<_>>();
    let comparison = compare_population(
        &cells,
        &expected_inputs,
        loaded.suite.repetitions + loaded.suite.warmup,
    )
    .and_then(|()| compare_budgets(&cells));
    if comparison.is_err() {
        exit = 2;
    }
    let mut paired = Vec::new();
    if exit == 0 {
        let local = summaries
            .get("local")
            .ok_or_else(|| protocol("missing local comparison"))?;
        for (name, summary) in &summaries {
            if name != "local" {
                paired.push(json!({"baseline":"local","candidate":name,
                    "observations":reporting::paired_summary(local, summary, &loaded.queries,
                        loaded.suite.repetitions, loaded.suite.seed, profile)?}));
            }
        }
    }
    report::json(
        &out.join("comparison.json"),
        &json!({"spec":SPEC,"profile":profile,"effect_interpretation":"none; engineering mechanism only",
        "exit_code":exit,"status":"engineering_strategy_observations_not_quality_certification",
        "gates":gates,"equal_effective_budgets":comparison.is_ok(),
        "budgets_by_input":comparison.as_ref().ok(),
        "comparison_error":comparison.as_ref().err().map(ToString::to_string),
        "cost_columns":cost_columns,"paired_against_local":paired,
        "paired_status":if exit==0{"mechanism_only"}else{"unavailable_failed_integrity_or_population_gate"},
        "capability_scope":{"local":"local lanes","auto":"local plus optional semantic, with explicit local fallback",
            "semantic":"local plus required semantic port; local lanes retained","dense_only":"unsupported_by_current_public_api"},
        "limitations":["no live calls; literal loopback engineering fixtures only",
            "not an independent public holdout or V19 benefit certificate",
            "raw time includes adapter validation/persistence; not a product-only latency measure",
            "cost receipts describe originating work; missing/unknown is never zero",
            "a common build receipt is local provenance, not remote source-to-binary attestation"]}),
    )?;
    Ok(exit)
}
