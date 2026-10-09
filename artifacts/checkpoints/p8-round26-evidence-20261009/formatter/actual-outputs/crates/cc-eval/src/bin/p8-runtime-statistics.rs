//! Recompute runtime observations from their original plan and every raw row.
//! Quantiles and confidence intervals belong to benchmark::statistics; this
//! adapter neither scores retrieval nor creates a new performance gate.
use cc_eval::benchmark::{invalid, statistics, Result};
use clap::Parser;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

const MAX_RAW_BYTES: u64 = 512 * 1024 * 1024;
const ACTIONS: [&str; 6] = [
    "bounded_symbol_churn",
    "add",
    "rename",
    "delete",
    "real_git_branch_switch",
    "restore_api",
];

#[derive(Parser)]
#[command(about = "Replay all original P8 runtime observations with existing statistics")]
struct Cli {
    #[arg(
        long,
        required_unless_present = "diagnostics_only",
        conflicts_with = "diagnostics_only"
    )]
    plan: Option<PathBuf>,
    #[arg(
        long,
        required_unless_present = "diagnostics_only",
        conflicts_with = "diagnostics_only"
    )]
    raw: Option<PathBuf>,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, conflicts_with = "diagnostics_only")]
    diagnostics: Option<PathBuf>,
    #[arg(long, conflicts_with_all = ["plan", "raw", "diagnostics"])]
    diagnostics_only: Option<PathBuf>,
}

#[derive(Deserialize)]
struct Plan {
    schema_version: u32,
    profile: String,
    concurrency: usize,
    operations: usize,
}

#[derive(Deserialize)]
struct Mutation {
    action: String,
}

#[derive(Deserialize)]
struct Row {
    id: usize,
    operation: String,
    status: String,
    scheduled_ns: u64,
    offered_ns: u64,
    started_ns: Option<u64>,
    call_started_ns: Option<u64>,
    finished_ns: u64,
    mutation_ordinal: Option<usize>,
    mutation: Option<Mutation>,
    response: Option<Value>,
}

fn validate(plan: &Plan, rows: &[Row]) -> Result<()> {
    if plan.schema_version != 1
        || !["mixed", "soak"].contains(&plan.profile.as_str())
        || ![1, 4, 8, 16].contains(&plan.concurrency)
        || plan.operations == 0
        || plan.operations > 10000
        || rows.len() != plan.operations
    {
        return Err(invalid(
            "runtime plan or complete terminal denominator is invalid",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut mutations = BTreeSet::new();
    for row in rows {
        let expected_operation = if row.id % 3 == 0 { "build" } else { "read" };
        if row.id >= plan.operations
            || !ids.insert(row.id)
            || row.operation != expected_operation
            || !["success", "error", "canceled", "queue_rejected"].contains(&row.status.as_str())
            || row.offered_ns < row.scheduled_ns
            || row.finished_ns < row.offered_ns
            || row
                .started_ns
                .is_some_and(|start| start < row.offered_ns || start > row.finished_ns)
            || row.call_started_ns.is_some_and(|call| {
                row.started_ns.is_none_or(|start| call < start) || call > row.finished_ns
            })
        {
            return Err(invalid(
                "runtime row has duplicate/unknown identity, outcome or reversed timing",
            ));
        }
        if row.status == "queue_rejected"
            && (row.started_ns.is_some() || row.call_started_ns.is_some() || row.mutation.is_some())
        {
            return Err(invalid("rejected runtime row claims execution"));
        }
        if row.operation == "read" && (row.mutation.is_some() || row.mutation_ordinal.is_some()) {
            return Err(invalid("runtime read row claims a source mutation"));
        }
        if let Some(mutation) = &row.mutation {
            let ordinal = row
                .mutation_ordinal
                .ok_or_else(|| invalid("runtime mutation has no admission ordinal"))?;
            if ordinal >= plan.operations.div_ceil(3)
                || mutation.action != ACTIONS[ordinal % ACTIONS.len()]
                || !mutations.insert(ordinal)
            {
                return Err(invalid(
                    "runtime mutation action or unique admission ordinal is invalid",
                ));
            }
        }
        if row.status == "success" {
            if row.call_started_ns.is_none() {
                return Err(invalid("successful runtime row has no actual call timing"));
            }
            let response = row.response.as_ref().ok_or_else(|| {
                invalid("successful runtime row is missing its original response")
            })?;
            if row.operation == "build" {
                if row.mutation.is_none()
                    || response.get("parse_errors") != Some(&json!([]))
                    || response.pointer("/resolution_freshness/complete") != Some(&json!(true))
                {
                    return Err(invalid(
                        "successful runtime build lacks complete original evidence",
                    ));
                }
            } else if !response.as_array().is_some_and(|hits| {
                hits.iter().any(|hit| {
                    hit.get("name") == Some(&json!("p8_runtime_stable_signal"))
                        && hit.get("file_path") == Some(&json!("stable.py"))
                })
            }) {
                return Err(invalid(
                    "successful runtime read lacks the exact original public hit",
                ));
            }
        }
    }
    if mutations.iter().copied().ne(0..mutations.len()) {
        return Err(invalid(
            "runtime mutation admission sequence has a missing ordinal",
        ));
    }
    Ok(())
}

fn timings(values: &[u64], expected: usize) -> Value {
    json!({
        "recorded_samples": values.len(),
        "missing_samples": expected.saturating_sub(values.len()),
        "distribution": statistics::distribution(values),
        "p95_ci": statistics::quantile_interval(values, 0.95),
        "p99_ci": statistics::quantile_interval(values, 0.99),
    })
}

fn summarize(plan: &Plan, operation: &str, mutation: Option<&str>, rows: &[&Row]) -> Value {
    let mut outcomes = BTreeMap::<&str, usize>::new();
    let mut all = Vec::with_capacity(rows.len());
    let mut successful = Vec::new();
    let mut scheduled = Vec::with_capacity(rows.len());
    let mut dispatch = Vec::new();
    let mut preparation = Vec::new();
    let mut call = Vec::new();
    for row in rows {
        *outcomes.entry(&row.status).or_default() += 1;
        // Match the existing sampler's integer microsecond resolution. Original
        // nanoseconds remain in raw and the driver's legacy summary fields.
        let elapsed = (row.finished_ns - row.offered_ns) / 1000;
        all.push(elapsed);
        scheduled.push((row.finished_ns - row.scheduled_ns) / 1000);
        if row.status == "success" {
            successful.push(elapsed);
        }
        if let Some(start) = row.started_ns {
            dispatch.push((start - row.offered_ns) / 1000);
            if let Some(started_call) = row.call_started_ns {
                preparation.push((started_call - start) / 1000);
                call.push((row.finished_ns - started_call) / 1000);
            }
        }
    }
    json!({
        "profile": plan.profile,
        "configured_concurrency": plan.concurrency,
        "operation": operation,
        "mutation_action": mutation,
        "expected_samples_in_group": rows.len(),
        "recorded_samples": rows.len(),
        "outcomes": outcomes,
        "all_attempt_offered_to_terminal": timings(&all, rows.len()),
        "successful_offered_to_terminal": timings(&successful, successful.len()),
        "successful_samples": successful.len(),
        "non_success_samples": rows.len() - successful.len(),
        "scheduled_to_terminal": timings(&scheduled, rows.len()),
        "client_dispatch_wait": timings(&dispatch, rows.len()),
        "write_admission_and_preparation": timings(&preparation, rows.len()),
        "client_call_to_terminal_including_validation": timings(&call, rows.len()),
        "cache_state": "not_isolated_under_mixed_concurrency",
        "population_scope": if operation == "build" && mutation.is_none() {
            "primary build class includes distinct mutation actions; inspect the separate action strata"
        } else { "one operation/action stratum within one profile and configured concurrency" },
    })
}

fn legacy_ns(rows: &[Row]) -> Value {
    let summarize = |operation: Option<&str>| {
        let values: Vec<_> = rows
            .iter()
            .filter(|row| operation.is_none_or(|kind| row.operation == kind))
            .map(|row| row.finished_ns - row.offered_ns)
            .collect();
        statistics::legacy_latency_ns(&values)
    };
    json!({
        "latency": summarize(None),
        "latency_by_operation": {
            "read": summarize(Some("read")),
            "build": summarize(Some("build")),
        },
    })
}

fn replay(plan: &Plan, rows: &[Row]) -> Result<Value> {
    validate(plan, rows)?;
    let mut outcomes = BTreeMap::<&str, usize>::new();
    for row in rows {
        *outcomes.entry(&row.status).or_default() += 1;
    }
    let by_operation: Vec<_> = ["read", "build"]
        .into_iter()
        .map(|kind| {
            let selected: Vec<_> = rows.iter().filter(|row| row.operation == kind).collect();
            summarize(plan, kind, None, &selected)
        })
        .collect();
    let mut mutations = BTreeMap::<&str, Vec<&Row>>::new();
    for row in rows.iter().filter(|row| row.operation == "build") {
        mutations
            .entry(
                row.mutation
                    .as_ref()
                    .map_or("not_started_or_mutation_unavailable", |mutation| {
                        mutation.action.as_str()
                    }),
            )
            .or_default()
            .push(row);
    }
    let by_mutation: Vec<_> = mutations
        .into_iter()
        .map(|(action, selected)| summarize(plan, "build", Some(action), &selected))
        .collect();
    let successful = rows.iter().filter(|row| row.status == "success").count();
    Ok(json!({
        "schema_version": 1,
        "profile": plan.profile,
        "configured_concurrency": plan.concurrency,
        "expected_samples": plan.operations,
        "recorded_samples": rows.len(),
        "missing_samples": 0,
        "unexpected_samples": 0,
        "outcomes": outcomes,
        "observation_status": if successful == rows.len() { "all_original_terminal_outcomes_successful" }
            else { "original_terminal_failures_retained" },
        "exit_code": if successful == rows.len() { 0 } else { 1 },
        "by_operation": by_operation,
        "by_build_mutation": by_mutation,
        "legacy_ns": legacy_ns(rows),
        "units": "integer microseconds; original nanoseconds retained in raw",
        "statistics_owner": "cc_eval::benchmark::statistics::{distribution,quantile_interval}",
        "statistical_scope": "descriptive all-attempt and successful-attempt observations; existing IID quantile intervals; serial correlation and mixed mutation classes are not certified homogeneous tails",
        "population_rule": "one profile/concurrency per input; primary and mutation views overlap and must not be added or pooled to meet a sample floor",
        "timeout_semantics": "error/canceled/rejected attempts stay in the offered denominator; terminal elapsed is not necessarily time to a valid answer",
        "latency_scope": "client observations include transport, admission and validation; no separated backend service or queue claim",
        "new_performance_gate": false,
        "release_certified": false,
    }))
}

fn file_sha256(path: &Path) -> Result<String> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let size = input.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

const MAX_DIAGNOSTICS_BYTES: u64 = 128 * 1024 * 1024;
const DIAGNOSTIC_PHASES: [&str; 21] = [
    "mcp_server_call",
    "handler_dispatch_wait",
    "handler_service",
    "cpu_caller",
    "cpu_admission",
    "cpu_permit_wait",
    "cpu_dispatch_wait",
    "cpu_service",
    "async_caller",
    "async_admission",
    "async_permit_wait",
    "async_service",
    "semantic_network_admission",
    "semantic_network_permit_wait",
    "semantic_network_direct_admission",
    "semantic_encoding_caller",
    "semantic_encoding_dispatch_wait",
    "semantic_encoding_service",
    "semantic_background_permit_wait",
    "semantic_background_dispatch_wait",
    "semantic_background_service",
];
const PHYSICAL_WORKERS: [&str; 4] = [
    "handler_service",
    "cpu_service",
    "semantic_encoding_service",
    "semantic_background_service",
];
const DIAGNOSTIC_WAITS: [&str; 7] = [
    "db_write_mutex",
    "db_read_pool_guard",
    "db_read_pool",
    "index_read_lock",
    "index_write_lock",
    "build_gate",
    "provider_gate_attempt",
];

// Unlike serde's default Option handling, nullable evidence must be present:
// absent data and an explicit unknown/null observation are distinct.
fn required_nullable<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct DiagnosticContext {
    #[serde(deserialize_with = "required_nullable")]
    operation_id: Option<usize>,
    #[serde(deserialize_with = "required_nullable")]
    operation: Option<String>,
    role: String,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum DiagnosticOrigin {
    McpOrigin {
        request_seq: u64,
        request_id_kind: String,
        request_id: Value,
        request_id_bytes: u64,
        #[serde(deserialize_with = "required_nullable")]
        request_id_blake3: Option<String>,
    },
    LocalOperation {
        operation_seq: u64,
        seed: u64,
        scenario: String,
        ordinal: u64,
        concurrency: u64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticPhase {
    instance: String,
    process_id: u64,
    #[serde(deserialize_with = "required_nullable")]
    origin: Option<DiagnosticOrigin>,
    context: DiagnosticContext,
    phase: String,
    phase_id: u64,
    start_ns: u64,
    end_ns: u64,
    elapsed_ns: u64,
    outcome: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticWait {
    instance: String,
    process_id: u64,
    #[serde(deserialize_with = "required_nullable")]
    origin: Option<DiagnosticOrigin>,
    context: DiagnosticContext,
    worker_phase_id: u64,
    complete: bool,
    wait: String,
    count: u64,
    #[serde(deserialize_with = "required_nullable")]
    sum_ns: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    max_ns: Option<u64>,
    failed: u64,
    incomplete: u64,
    #[serde(rename = "sqlite_busy_ns")]
    _sqlite_busy_ns: (),
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct DiagnosticEvidenceInput {
    path: String,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Diagnostics {
    schema_version: u32,
    status: String,
    #[serde(deserialize_with = "required_nullable")]
    plan_sha256: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    raw_sha256: Option<String>,
    units: String,
    evidence_inputs: Vec<DiagnosticEvidenceInput>,
    phases: Vec<DiagnosticPhase>,
    worker_waits: Vec<DiagnosticWait>,
}

fn bounded_label(value: &str, maximum: usize) -> bool {
    !value.is_empty() && value.len() <= maximum && value.is_ascii()
}

fn digest_label(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

impl DiagnosticOrigin {
    fn validate(&self) -> bool {
        match self {
            Self::McpOrigin {
                request_seq,
                request_id_kind,
                request_id,
                request_id_bytes,
                request_id_blake3,
            } => {
                *request_seq != 0
                    && match request_id_kind.as_str() {
                        "number" => {
                            request_id.as_i64().is_some()
                                && *request_id_bytes == 0
                                && request_id_blake3.is_none()
                        }
                        "string" => {
                            request_id.as_str().is_some_and(|value| {
                                value.len() <= 128 && value.len() as u64 == *request_id_bytes
                            }) && request_id_blake3.is_none()
                        }
                        "string_hash" => {
                            request_id.is_null()
                                && *request_id_bytes > 128
                                && request_id_blake3.as_deref().is_some_and(digest_label)
                        }
                        _ => false,
                    }
            }
            Self::LocalOperation {
                operation_seq,
                scenario,
                concurrency,
                ..
            } => *operation_seq != 0 && *concurrency != 0 && bounded_label(scenario, 80),
        }
    }

    fn local_stratum(&self) -> (Option<u64>, Option<&str>, Option<u64>) {
        match self {
            Self::LocalOperation {
                seed,
                scenario,
                concurrency,
                ..
            } => (Some(*seed), Some(scenario.as_str()), Some(*concurrency)),
            Self::McpOrigin { .. } => (None, None, None),
        }
    }

    fn identity(&self) -> (&'static str, u64) {
        match self {
            Self::McpOrigin { request_seq, .. } => ("mcp_origin", *request_seq),
            Self::LocalOperation { operation_seq, .. } => ("local_operation", *operation_seq),
        }
    }
}

fn validate_diagnostic_context(
    context: &DiagnosticContext,
    rows: Option<&BTreeMap<usize, &Row>>,
) -> bool {
    if !bounded_label(&context.role, 80)
        || context
            .operation
            .as_deref()
            .is_some_and(|value| !["read", "build"].contains(&value))
    {
        return false;
    }
    match (context.operation_id, context.operation.as_deref(), rows) {
        (Some(id), Some(operation), Some(rows)) => {
            rows.get(&id).is_some_and(|row| row.operation == operation)
        }
        (None, None, _) => true,
        // A local API operation may have a class but no invented runtime row ID.
        (None, Some(_), None) => true,
        _ => false,
    }
}

fn valid_diagnostic_outcome(phase: &str, outcome: &str) -> bool {
    let allowed: &[&str] = match phase {
        "mcp_server_call" => &[
            "returned",
            "tool_error",
            "protocol_error",
            "dropped",
            "unwinding",
        ],
        "handler_dispatch_wait"
        | "cpu_dispatch_wait"
        | "semantic_encoding_dispatch_wait"
        | "semantic_background_dispatch_wait" => &["entered", "dropped", "unwinding"],
        "semantic_background_permit_wait" => {
            &["acquired", "cancelled", "closed", "dropped", "unwinding"]
        }
        "semantic_background_service" => &["returned", "dropped", "unwinding"],
        _ => &[
            "ok",
            "busy",
            "cancelled",
            "timed_out",
            "error",
            "dropped",
            "unwinding",
        ],
    };
    allowed.contains(&outcome)
}

fn checked_diagnostic_total(mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0_u64, |total, value| {
        total
            .checked_add(value)
            .ok_or_else(|| invalid("diagnostic aggregate overflow"))
    })
}

fn summarize_diagnostics(input: &Diagnostics, rows: Option<&[Row]>) -> Result<Value> {
    if input.schema_version != 1
        || input.status != "complete"
        || input.units != "process_monotonic_relative_ns"
    {
        return Err(invalid("unsupported or incomplete diagnostic evidence"));
    }
    let row_index = rows.map(|rows| {
        rows.iter()
            .map(|row| (row.id, row))
            .collect::<BTreeMap<_, _>>()
    });
    let mut phases = BTreeMap::new();
    let mut processes = BTreeMap::new();
    let mut origins = BTreeMap::new();
    for phase in &input.phases {
        if !bounded_label(&phase.instance, 80)
            || phase.process_id == 0
            || phase.phase_id == 0
            || !DIAGNOSTIC_PHASES.contains(&phase.phase.as_str())
            || !valid_diagnostic_outcome(&phase.phase, &phase.outcome)
            || phase.end_ns.checked_sub(phase.start_ns) != Some(phase.elapsed_ns)
            || !validate_diagnostic_context(&phase.context, row_index.as_ref())
            || phase
                .origin
                .as_ref()
                .is_some_and(|origin| !origin.validate())
            || (phase.origin.is_none() && phase.context.operation_id.is_some())
            || phases
                .insert((phase.instance.as_str(), phase.phase_id), phase)
                .is_some()
        {
            return Err(invalid("invalid, duplicate or reversed diagnostic phase"));
        }
        if processes
            .insert(phase.instance.as_str(), phase.process_id)
            .is_some_and(|old| old != phase.process_id)
        {
            return Err(invalid("one diagnostic instance claims multiple processes"));
        }
        if let Some(origin) = &phase.origin {
            let key = (phase.instance.as_str(), origin.identity());
            if origins.insert(key, origin).is_some_and(|old| old != origin) {
                return Err(invalid(
                    "one diagnostic origin identity has conflicting attribution",
                ));
            }
        }
    }
    let mut waits = BTreeSet::new();
    for wait in &input.worker_waits {
        let parent = phases
            .get(&(wait.instance.as_str(), wait.worker_phase_id))
            .ok_or_else(|| invalid("diagnostic wait has no physical worker phase"))?;
        if !PHYSICAL_WORKERS.contains(&parent.phase.as_str())
            || !wait.complete
            || wait.process_id != parent.process_id
            || wait.origin != parent.origin
            || wait.context != parent.context
            || !DIAGNOSTIC_WAITS.contains(&wait.wait.as_str())
            || !waits.insert((
                wait.instance.as_str(),
                wait.worker_phase_id,
                wait.wait.as_str(),
            ))
            || wait.failed > wait.count
            || wait.incomplete != 0
            || (wait.count == 0 && (wait.sum_ns.is_some() || wait.max_ns.is_some()))
            || (wait.count != 0
                && !matches!((wait.sum_ns, wait.max_ns),
                (Some(sum), Some(maximum)) if sum >= maximum))
        {
            return Err(invalid(
                "invalid, duplicate or incomplete physical worker wait",
            ));
        }
    }
    let workers = input
        .phases
        .iter()
        .filter(|phase| PHYSICAL_WORKERS.contains(&phase.phase.as_str()))
        .count();
    if waits.len()
        != workers
            .checked_mul(DIAGNOSTIC_WAITS.len())
            .ok_or_else(|| invalid("diagnostic worker count overflow"))?
    {
        return Err(invalid("physical worker is missing a wait category"));
    }

    let mut phase_groups = BTreeMap::<_, Vec<&DiagnosticPhase>>::new();
    for phase in &input.phases {
        phase_groups
            .entry((
                phase.instance.as_str(),
                phase.context.operation.as_deref(),
                phase.context.role.as_str(),
                phase.phase.as_str(),
                phase.outcome.as_str(),
                phase
                    .origin
                    .as_ref()
                    .map(DiagnosticOrigin::local_stratum)
                    .unwrap_or_default(),
            ))
            .or_default()
            .push(phase);
    }
    let by_phase: Vec<_> = phase_groups
        .into_iter()
        .map(|(key, phases)| {
            let values: Vec<_> = phases.iter().map(|phase| phase.elapsed_ns / 1000).collect();
            json!({
                "instance": key.0, "operation": key.1, "role": key.2,
                "phase": key.3, "outcome": key.4,
                "local_seed": key.5.0, "local_scenario": key.5.1, "local_concurrency": key.5.2,
                "physical_worker": PHYSICAL_WORKERS.contains(&key.3),
                "elapsed": timings(&values, phases.len()),
            })
        })
        .collect();
    let mut wait_groups = BTreeMap::<_, Vec<&DiagnosticWait>>::new();
    for wait in &input.worker_waits {
        let parent = phases
            .get(&(wait.instance.as_str(), wait.worker_phase_id))
            .ok_or_else(|| invalid("diagnostic wait lost its validated parent"))?;
        wait_groups
            .entry((
                wait.instance.as_str(),
                wait.context.operation.as_deref(),
                wait.context.role.as_str(),
                wait.wait.as_str(),
                parent.phase.as_str(),
                parent.outcome.as_str(),
                wait.origin
                    .as_ref()
                    .map(DiagnosticOrigin::local_stratum)
                    .unwrap_or_default(),
            ))
            .or_default()
            .push(wait);
    }
    let mut by_wait = Vec::new();
    for (key, waits) in wait_groups {
        let sum_us: Vec<_> = waits
            .iter()
            .filter_map(|wait| wait.sum_ns.map(|ns| ns / 1000))
            .collect();
        let maximum_us: Vec<_> = waits
            .iter()
            .filter_map(|wait| wait.max_ns.map(|ns| ns / 1000))
            .collect();
        by_wait.push(json!({
            "instance": key.0, "operation": key.1, "role": key.2, "wait": key.3,
            "worker_phase": key.4, "worker_outcome": key.5,
            "local_seed": key.6.0, "local_scenario": key.6.1, "local_concurrency": key.6.2,
            "worker_observations": waits.len(),
            "acquisitions": checked_diagnostic_total(waits.iter().map(|wait| wait.count))?,
            "failed_acquisitions": checked_diagnostic_total(waits.iter().map(|wait| wait.failed))?,
            "incomplete_acquisitions": 0,
            "zero_acquisition_workers": waits.iter().filter(|wait| wait.count == 0).count(),
            "per_worker_sum": timings(&sum_us, waits.len()),
            "per_worker_maximum": timings(&maximum_us, waits.len()),
            "sqlite_busy_ns": Value::Null,
            "sqlite_busy_missing_workers": waits.len(),
        }));
    }
    Ok(json!({
        "schema_version": 1,
        "status": "observations_only",
        "processes": processes,
        "recorded_phases": input.phases.len(),
        "physical_worker_phases": workers,
        "recorded_worker_waits": input.worker_waits.len(),
        "by_phase": by_phase,
        "by_wait": by_wait,
        "units": "integer microseconds; original process-relative nanoseconds retained in diagnostics",
        "population_rule": "separate instance/operation/role/phase/outcome and local seed/scenario/concurrency strata; overlapping caller and physical worker spans must not be summed into E2E",
        "origin_scope": "origin identifies attribution, including background work; it does not bound work to a synchronous client operation",
        "wait_scope": "distributions describe each worker's sum and maximum, not individual lock acquisition quantiles; zero-acquisition workers retain null timings; failed_acquisitions counts non-Ok API returns including poisoned guards and does not mean the physical lock was not acquired",
        "server_call_scope": "mcp_server_call ends when the handler returns; async_service is future lifetime, neither is client E2E",
        "statistical_scope": "descriptive observations with existing IID quantile intervals; no homogeneous or independent tail certification",
        "new_performance_gate": false,
        "release_certified": false,
    }))
}

fn verify_diagnostic_evidence(directory: &Path, entries: &[DiagnosticEvidenceInput]) -> Result<()> {
    let mut paths = BTreeSet::new();
    for entry in entries {
        let path = Path::new(&entry.path);
        let normalized: PathBuf = path.components().map(|part| part.as_os_str()).collect();
        if entry.path.is_empty()
            || !path
                .components()
                .all(|part| matches!(part, std::path::Component::Normal(_)))
            || !paths.insert(normalized)
            || !digest_label(&entry.sha256)
        {
            return Err(invalid(
                "invalid or duplicate diagnostic evidence reference",
            ));
        }
        let actual = directory.join(path);
        if fs::metadata(&actual)?.len() != entry.bytes || file_sha256(&actual)? != entry.sha256 {
            return Err(invalid(
                "diagnostic evidence reference does not match original bytes",
            ));
        }
    }
    Ok(())
}

fn read_diagnostics(path: &Path, binding: Option<(&str, &str, &[Row])>) -> Result<Value> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_DIAGNOSTICS_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DIAGNOSTICS_BYTES {
        return Err(invalid(
            "diagnostic sidecar exceeds its 128 MiB evidence budget",
        ));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let input: Diagnostics = serde_json::from_slice(&bytes)?;
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    match binding {
        Some((plan, raw, _)) => {
            if input.plan_sha256.as_deref() != Some(plan)
                || input.raw_sha256.as_deref() != Some(raw)
                || !input.evidence_inputs.is_empty()
            {
                return Err(invalid(
                    "diagnostics are not bound to these original runtime inputs",
                ));
            }
        }
        None => {
            if input.plan_sha256.is_some()
                || input.raw_sha256.is_some()
                || input.evidence_inputs.is_empty()
            {
                return Err(invalid(
                    "diagnostics-only requires original evidence, not invented runtime hashes",
                ));
            }
            verify_diagnostic_evidence(directory, &input.evidence_inputs)?;
        }
    }
    let mut report = summarize_diagnostics(&input, binding.map(|(_, _, rows)| rows))?;
    if binding.is_none() {
        verify_diagnostic_evidence(directory, &input.evidence_inputs)?;
    }
    if file_sha256(path)? != digest {
        return Err(invalid("diagnostic sidecar changed while replaying"));
    }
    report["input_sha256"] = json!(digest);
    report["plan_sha256"] = json!(input.plan_sha256);
    report["raw_sha256"] = json!(input.raw_sha256);
    report["evidence_inputs"] = json!(input.evidence_inputs);
    report["evidence_binding"] = json!(if binding.is_some() {
        "original_runtime_plan_and_raw"
    } else {
        "original_relative_evidence_files_verified_before_and_after"
    });
    Ok(report)
}

fn run(cli: Cli) -> Result<i32> {
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&cli.output)?;
    let result = (|| -> Result<Value> {
        if let Some(path) = &cli.diagnostics_only {
            if cli.plan.is_some() || cli.raw.is_some() || cli.diagnostics.is_some() {
                return Err(invalid(
                    "diagnostics-only cannot be mixed with runtime inputs",
                ));
            }
            return Ok(json!({
                "schema_version": 1, "exit_code": 0,
                "observation_status": "diagnostic_observations_only",
                "diagnostics": read_diagnostics(path, None)?,
                "replay_binary_sha256": file_sha256(&std::env::current_exe()?)?,
                "release_certified": false,
            }));
        }
        let plan_path = cli
            .plan
            .as_deref()
            .ok_or_else(|| invalid("missing original runtime plan"))?;
        let raw_path = cli
            .raw
            .as_deref()
            .ok_or_else(|| invalid("missing original runtime raw"))?;
        if fs::metadata(plan_path)?.len() > 16 * 1024 * 1024
            || fs::metadata(raw_path)?.len() > MAX_RAW_BYTES
        {
            return Err(invalid(
                "runtime statistics input exceeds the retained evidence budget",
            ));
        }
        let plan_sha256 = file_sha256(plan_path)?;
        let raw_sha256 = file_sha256(raw_path)?;
        let plan: Plan = serde_json::from_slice(&fs::read(plan_path)?)?;
        let mut rows = Vec::new();
        for line in BufReader::new(fs::File::open(raw_path)?).lines() {
            let line = line?;
            if line.trim().is_empty() {
                return Err(invalid("empty/truncated runtime raw event"));
            }
            let value: Value = serde_json::from_str(&line)?;
            if value.get("kind").and_then(Value::as_str).is_none() {
                return Err(invalid("runtime raw event has no kind"));
            }
            if value["kind"] == "operation" {
                if rows.len() >= 10000 {
                    return Err(invalid("runtime terminal observation bound exceeded"));
                }
                rows.push(serde_json::from_value(value)?);
            }
        }
        rows.sort_by_key(|row: &Row| row.id);
        let mut report = replay(&plan, &rows)?;
        if let Some(path) = &cli.diagnostics {
            report["diagnostics"] =
                read_diagnostics(path, Some((&plan_sha256, &raw_sha256, &rows)))?;
        }
        if file_sha256(plan_path)? != plan_sha256 || file_sha256(raw_path)? != raw_sha256 {
            return Err(invalid("runtime statistics inputs changed while replaying"));
        }
        report["plan_sha256"] = json!(plan_sha256);
        report["raw_sha256"] = json!(raw_sha256);
        report["replay_binary_sha256"] = json!(file_sha256(&std::env::current_exe()?)?);
        Ok(report)
    })();
    let report = match result {
        Ok(report) => report,
        Err(error) => json!({"schema_version":1,"exit_code":2,
                            "observation_status":"invalid_original_runtime_evidence",
                            "error":error.to_string(),"release_certified":false}),
    };
    serde_json::to_writer_pretty(&mut output, &report)?;
    writeln!(output)?;
    Ok(report["exit_code"].as_i64().unwrap_or(2) as i32)
}

fn main() {
    match run(Cli::parse()) {
        Ok(code) => std::process::exit(code),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(operations: usize) -> Plan {
        Plan {
            schema_version: 1,
            profile: "mixed".into(),
            concurrency: 4,
            operations,
        }
    }

    fn row(id: usize) -> Row {
        let build = id.is_multiple_of(3);
        serde_json::from_value(json!({
            "id":id,"operation":if build {"build"} else {"read"},"status":"success",
            "scheduled_ns":0,"offered_ns":1000,"started_ns":2000,"call_started_ns":3000,
            "finished_ns":10000 + id as u64 * 1000,
            "mutation_ordinal":if build {Some(id / 3)} else {None},
            "mutation":if build {json!({"action":ACTIONS[(id / 3) % 6]})} else {Value::Null},
            "response":if build {json!({"parse_errors":[],"resolution_freshness":{"complete":true}})}
                else {json!([{"name":"p8_runtime_stable_signal","file_path":"stable.py"}])},
        })).unwrap()
    }

    #[test]
    fn missing_duplicate_reversed_and_false_success_rows_are_rejected() {
        assert!(replay(&plan(3), &[row(0), row(1)]).is_err());
        assert!(replay(&plan(3), &[row(0), row(1), row(1)]).is_err());
        let mut bad = row(2);
        bad.finished_ns = 999;
        assert!(replay(&plan(3), &[row(0), row(1), bad]).is_err());
        let mut false_hit = row(2);
        false_hit.response = Some(json!({"query":"p8_runtime_stable_signal"}));
        assert!(replay(&plan(3), &[row(0), row(1), false_hit]).is_err());
    }

    #[test]
    fn rejected_and_failed_attempts_keep_the_original_denominator() {
        let mut rejected = row(1);
        rejected.status = "queue_rejected".into();
        rejected.started_ns = None;
        rejected.call_started_ns = None;
        rejected.response = None;
        let mut failed = row(2);
        failed.status = "error".into();
        failed.response = None;
        let report = replay(&plan(3), &[row(0), rejected, failed]).unwrap();
        assert_eq!(report["exit_code"], 1);
        assert_eq!(report["recorded_samples"], 3);
        let reads = &report["by_operation"][0];
        assert_eq!(
            reads["all_attempt_offered_to_terminal"]["distribution"]["samples"],
            2
        );
        assert_eq!(reads["successful_samples"], 0);
        assert_eq!(reads["client_dispatch_wait"]["missing_samples"], 1);
        assert_eq!(reads["outcomes"]["queue_rejected"], 1);
        assert_eq!(reads["outcomes"]["error"], 1);
        assert!(reads["successful_offered_to_terminal"]["p99_ci"].is_null());
    }

    #[test]
    fn legacy_ns_keeps_every_terminal_outcome_and_original_precision() {
        let mut failed = row(1);
        failed.status = "error".into();
        failed.started_ns = Some(1000);
        failed.call_started_ns = Some(1000);
        failed.finished_ns = 2001;
        failed.response = None;
        let mut canceled = row(2);
        canceled.status = "canceled".into();
        canceled.started_ns = None;
        canceled.call_started_ns = None;
        canceled.finished_ns = 1999;
        canceled.response = None;
        let mut rejected = row(3);
        rejected.status = "queue_rejected".into();
        rejected.started_ns = None;
        rejected.call_started_ns = None;
        rejected.finished_ns = 2000;
        rejected.mutation_ordinal = None;
        rejected.mutation = None;
        rejected.response = None;
        let mut rows = vec![row(0), failed, canceled, rejected];
        let report = replay(&plan(4), &rows).unwrap();
        let expected = |n, p50, p95| {
            json!({
                "n": n, "p50_ns": p50, "p95_ns": p95, "p99_ns": p95,
                "maximum_ns": p95,
                "scope": "all_offered_terminal_outcomes_including_rejections_and_failures",
                "tail_stability_claim": false,
            })
        };
        assert_eq!(
            report["legacy_ns"],
            json!({
                "latency": expected(4, 1000, 9000),
                "latency_by_operation": {
                    "read": expected(2, 999, 1001),
                    "build": expected(2, 1000, 9000),
                },
            })
        );
        assert_eq!(report["exit_code"], 1);
        assert_eq!(
            report["outcomes"],
            json!({
                "success": 1, "error": 1, "canceled": 1, "queue_rejected": 1,
            })
        );
        // The existing microsecond population and truncation stay unchanged.
        let reads = &report["by_operation"][0];
        assert_eq!(
            reads["all_attempt_offered_to_terminal"]["distribution"],
            json!({
                "samples": 2, "p50_us": 0, "p95_us": 1, "max_us": 1,
                "tail_claim": "insufficient_for_tail_claim",
            })
        );
        assert_eq!(reads["successful_samples"], 0);
        assert_eq!(reads["non_success_samples"], 2);
        assert!(reads["successful_offered_to_terminal"]["p99_ci"].is_null());
        rows.reverse();
        assert_eq!(replay(&plan(4), &rows).unwrap(), report);
    }

    #[test]
    fn legacy_ns_empty_operation_stays_null_without_admitting_an_empty_plan() {
        let report = replay(&plan(1), &[row(0)]).unwrap();
        assert_eq!(
            report["legacy_ns"]["latency_by_operation"]["read"],
            json!({
                "n": 0, "p50_ns": null, "p95_ns": null, "p99_ns": null,
                "maximum_ns": null,
                "scope": "all_offered_terminal_outcomes_including_rejections_and_failures",
                "tail_stability_claim": false,
            })
        );
        assert_eq!(report["legacy_ns"]["latency"]["n"], 1);
        assert!(replay(&plan(0), &[]).is_err());
    }

    #[test]
    fn mutation_views_are_separate_and_never_added_to_the_primary_population() {
        let rows: Vec<_> = (0..18).map(row).collect();
        let report = replay(&plan(18), &rows).unwrap();
        assert_eq!(report["recorded_samples"], 18);
        assert_eq!(report["by_operation"][0]["recorded_samples"], 12);
        assert_eq!(report["by_operation"][1]["recorded_samples"], 6);
        let actions = report["by_build_mutation"].as_array().unwrap();
        assert_eq!(actions.len(), 6);
        assert!(actions.iter().all(|value| value["recorded_samples"] == 1));
        let mut invalid_action = row(0);
        invalid_action.mutation.as_mut().unwrap().action = "unrecorded_work".into();
        assert!(replay(&plan(3), &[invalid_action, row(1), row(2)]).is_err());
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    // Explicit protocol fixtures. They are not measured product observations.
    fn fixture() -> Value {
        let origin = json!({
            "kind": "local_operation", "operation_seq": 1, "seed": 7,
            "scenario": "quiet", "ordinal": 0, "concurrency": 4,
        });
        let context = json!({"operation_id": null, "operation": "read", "role": "local"});
        json!({
            "schema_version": 1, "status": "complete",
            "plan_sha256": null, "raw_sha256": null,
            "units": "process_monotonic_relative_ns", "evidence_inputs": [],
            "phases": [{
                "instance": "seed-7", "process_id": 17, "origin": origin,
                "context": context, "phase": "cpu_service", "phase_id": 2,
                "start_ns": 1000, "end_ns": 10001, "elapsed_ns": 9001,
                "outcome": "error",
            }],
            "worker_waits": DIAGNOSTIC_WAITS.iter().map(|wait| json!({
                "instance": "seed-7", "process_id": 17, "origin": origin,
                "context": context, "worker_phase_id": 2,
                "complete": true, "wait": wait, "count": 0,
                "sum_ns": null, "max_ns": null, "failed": 0, "incomplete": 0, "sqlite_busy_ns": null,
            })).collect::<Vec<_>>(),
        })
    }

    fn summary(value: Value) -> Result<Value> {
        let input: Diagnostics = serde_json::from_value(value)?;
        summarize_diagnostics(&input, None)
    }

    #[test]
    fn optional_modes_keep_original_required_arguments_and_reject_mixing() {
        let original = Cli::try_parse_from([
            "stats",
            "--plan",
            "plan.json",
            "--raw",
            "raw.jsonl",
            "--output",
            "out.json",
        ])
        .unwrap();
        assert!(original.diagnostics.is_none());
        assert!(original.diagnostics_only.is_none());
        assert!(Cli::try_parse_from(["stats", "--output", "out.json"]).is_err());
        assert!(Cli::try_parse_from([
            "stats",
            "--diagnostics",
            "diag.json",
            "--output",
            "out.json",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "stats",
            "--diagnostics-only",
            "diag.json",
            "--output",
            "out.json",
        ])
        .is_ok());
        for extra in ["--plan", "--raw", "--diagnostics"] {
            assert!(Cli::try_parse_from([
                "stats",
                "--diagnostics-only",
                "diag.json",
                "--output",
                "out.json",
                extra,
                "extra.json",
            ])
            .is_err());
        }
    }

    #[test]
    fn wait_statistics_use_worker_aggregates_and_preserve_zero_nulls_and_failures() {
        let mut value = fixture();
        value["worker_waits"][0]["count"] = json!(2);
        value["worker_waits"][0]["sum_ns"] = json!(3500);
        value["worker_waits"][0]["max_ns"] = json!(2000);
        value["worker_waits"][0]["failed"] = json!(1);
        let report = summary(value).unwrap();
        assert_eq!(report["recorded_phases"], 1);
        assert_eq!(report["physical_worker_phases"], 1);
        assert_eq!(report["recorded_worker_waits"], 7);
        assert_eq!(report["by_phase"][0]["outcome"], "error");
        assert_eq!(
            report["by_phase"][0]["elapsed"]["distribution"]["max_us"],
            9
        );
        let waits = report["by_wait"].as_array().unwrap();
        let write = waits
            .iter()
            .find(|wait| wait["wait"] == "db_write_mutex")
            .unwrap();
        assert_eq!(write["acquisitions"], 2);
        assert_eq!(write["failed_acquisitions"], 1);
        assert_eq!(write["per_worker_sum"]["distribution"]["max_us"], 3);
        assert_eq!(write["per_worker_maximum"]["distribution"]["max_us"], 2);
        for wait in waits.iter().filter(|wait| wait["wait"] != "db_write_mutex") {
            assert_eq!(wait["zero_acquisition_workers"], 1);
            assert_eq!(wait["per_worker_sum"]["missing_samples"], 1);
            assert!(wait["per_worker_sum"]["distribution"]["p50_us"].is_null());
            assert!(wait["sqlite_busy_ns"].is_null());
        }
        let mut empty = fixture();
        empty["phases"] = json!([]);
        empty["worker_waits"] = json!([]);
        let empty = summary(empty).unwrap();
        assert_eq!(empty["recorded_phases"], 0);
        assert_eq!(empty["status"], "observations_only");
        assert_eq!(empty["by_phase"], json!([]));
    }

    #[test]
    fn malformed_phase_identity_clock_and_origin_are_rejected() {
        for (pointer, bad) in [
            ("/schema_version", json!(2)),
            ("/status", json!("partial")),
            ("/phases/0/start_ns", json!(10002)),
            ("/phases/0/elapsed_ns", json!(9000)),
            ("/phases/0/process_id", json!(0)),
            ("/phases/0/phase", json!("invented_service")),
            ("/phases/0/outcome", json!("invented_outcome")),
            ("/phases/0/outcome", json!("returned")),
            ("/phases/0/context/operation_id", json!(0)),
            ("/phases/0/context/role", json!("x".repeat(81))),
            ("/phases/0/origin/operation_seq", json!(0)),
            ("/phases/0/origin/seed", json!(1.5)),
        ] {
            let mut value = fixture();
            *value.pointer_mut(pointer).unwrap() = bad;
            assert!(summary(value).is_err(), "{pointer}");
        }
        let mut duplicate = fixture();
        let phase = duplicate["phases"][0].clone();
        duplicate["phases"].as_array_mut().unwrap().push(phase);
        assert!(summary(duplicate).is_err());
        let mut absent = fixture();
        absent["phases"][0]
            .as_object_mut()
            .unwrap()
            .remove("origin");
        assert!(summary(absent).is_err());
        let text = serde_json::to_string(&fixture()).unwrap();
        let duplicate_key = text.replacen(
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
            1,
        );
        assert!(serde_json::from_str::<Diagnostics>(&duplicate_key).is_err());
        let mut unknown = fixture();
        unknown["phases"][0]["unexpected"] = json!(1);
        assert!(summary(unknown).is_err());
    }

    #[test]
    fn waits_require_each_physical_worker_category_and_exact_parent_attribution() {
        for (pointer, bad) in [
            ("/worker_waits/0/complete", json!(false)),
            ("/worker_waits/0/worker_phase_id", json!(9)),
            ("/worker_waits/0/process_id", json!(18)),
            ("/worker_waits/0/origin/seed", json!(8)),
            ("/worker_waits/0/context/role", json!("other")),
            ("/worker_waits/0/count", json!(1)),
            ("/worker_waits/0/sum_ns", json!(0)),
            ("/worker_waits/0/failed", json!(1)),
            ("/worker_waits/0/incomplete", json!(1)),
            ("/worker_waits/0/sqlite_busy_ns", json!(0)),
            ("/worker_waits/0/wait", json!("sqlite_internal_wait")),
            ("/phases/0/phase", json!("async_service")),
        ] {
            let mut value = fixture();
            *value.pointer_mut(pointer).unwrap() = bad;
            assert!(summary(value).is_err(), "{pointer}");
        }
        let mut missing = fixture();
        missing["worker_waits"].as_array_mut().unwrap().pop();
        assert!(summary(missing).is_err());
        let mut duplicate = fixture();
        duplicate["worker_waits"][1] = duplicate["worker_waits"][0].clone();
        assert!(summary(duplicate).is_err());
        let mut reversed = fixture();
        reversed["worker_waits"][0]["count"] = json!(1);
        reversed["worker_waits"][0]["sum_ns"] = json!(9);
        reversed["worker_waits"][0]["max_ns"] = json!(10);
        assert!(summary(reversed).is_err());
        assert!(checked_diagnostic_total([u64::MAX, 1].into_iter()).is_err());
    }

    #[test]
    fn diagnostics_only_verifies_actual_original_evidence_without_fabricated_runtime() {
        let directory = tempfile::tempdir().unwrap();
        let evidence = directory.path().join("seed.json");
        fs::write(&evidence, b"{\"seed\":7}\n").unwrap();
        let path = directory.path().join("diagnostics.json");
        let mut value = fixture();
        value["evidence_inputs"] = json!([{
            "path": "seed.json", "bytes": fs::metadata(&evidence).unwrap().len(),
            "sha256": file_sha256(&evidence).unwrap(),
        }]);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let report = read_diagnostics(&path, None).unwrap();
        assert!(report["plan_sha256"].is_null());
        assert!(report["raw_sha256"].is_null());
        assert_eq!(report["recorded_phases"], 1);
        fs::write(&evidence, b"{\"seed\":8}\n").unwrap();
        assert!(read_diagnostics(&path, None).is_err());
        fs::write(&evidence, b"{\"seed\":7}\n").unwrap();
        for invalid_path in ["../seed.json", "/seed.json"] {
            let mut bad = value.clone();
            bad["evidence_inputs"][0]["path"] = json!(invalid_path);
            fs::write(&path, serde_json::to_vec(&bad).unwrap()).unwrap();
            assert!(read_diagnostics(&path, None).is_err());
        }
        let entry = value["evidence_inputs"][0].clone();
        value["evidence_inputs"].as_array_mut().unwrap().push(entry);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_diagnostics(&path, None).is_err());
        value["evidence_inputs"] = json!([]);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_diagnostics(&path, None).is_err());
    }

    #[test]
    fn runtime_diagnostics_match_hashes_and_actual_operation_while_legacy_stays_identical() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("diagnostics.json");
        let mut rows: Vec<Row> = serde_json::from_value(json!([{
            "id": 0, "operation": "build", "status": "error",
            "scheduled_ns": 0, "offered_ns": 1, "started_ns": 2,
            "call_started_ns": 3, "finished_ns": 4, "mutation_ordinal": null,
            "mutation": null, "response": null,
        }, {
            "id": 1, "operation": "read", "status": "error",
            "scheduled_ns": 0, "offered_ns": 1, "started_ns": 2,
            "call_started_ns": 3, "finished_ns": 4, "mutation_ordinal": null,
            "mutation": null, "response": null,
        }]))
        .unwrap();
        let plan = Plan {
            schema_version: 1,
            profile: "mixed".into(),
            concurrency: 4,
            operations: 2,
        };
        let original = replay(&plan, &rows).unwrap();
        let plan_hash = "a".repeat(64);
        let raw_hash = "b".repeat(64);
        let mut value = fixture();
        value["plan_sha256"] = json!(plan_hash);
        value["raw_sha256"] = json!(raw_hash);
        let origin = json!({
            "kind": "mcp_origin", "request_seq": 1, "request_id_kind": "number",
            "request_id": 7, "request_id_bytes": 0, "request_id_blake3": null,
        });
        let context = json!({"operation_id": 0, "operation": "build", "role": "build"});
        value["phases"][0]["origin"] = origin.clone();
        value["phases"][0]["context"] = context.clone();
        for wait in value["worker_waits"].as_array_mut().unwrap() {
            wait["origin"] = origin.clone();
            wait["context"] = context.clone();
        }
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut enriched = original.clone();
        enriched["diagnostics"] =
            read_diagnostics(&path, Some((&plan_hash, &raw_hash, &rows))).unwrap();
        enriched.as_object_mut().unwrap().remove("diagnostics");
        assert_eq!(enriched, original);
        assert_eq!(replay(&plan, &rows).unwrap(), original);
        assert!(original.get("diagnostics").is_none());
        let ordered = read_diagnostics(&path, Some((&plan_hash, &raw_hash, &rows))).unwrap();
        rows.reverse();
        assert_eq!(replay(&plan, &rows).unwrap(), original);
        assert_eq!(
            read_diagnostics(&path, Some((&plan_hash, &raw_hash, &rows))).unwrap(),
            ordered
        );
        assert!(read_diagnostics(&path, Some((&raw_hash, &plan_hash, &rows))).is_err());
        assert!(read_diagnostics(&path, None).is_err());
        value["phases"][0]["context"]["operation"] = json!("read");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(read_diagnostics(&path, Some((&plan_hash, &raw_hash, &rows))).is_err());
    }

    #[test]
    fn local_concurrency_strata_do_not_pool_while_ordinals_are_not_sample_groups() {
        let mut value = fixture();
        let mut second = value["phases"][0].clone();
        second["phase_id"] = json!(3);
        second["origin"]["operation_seq"] = json!(2);
        second["origin"]["concurrency"] = json!(8);
        value["phases"].as_array_mut().unwrap().push(second.clone());
        let additional: Vec<_> = value["worker_waits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|wait| {
                let mut wait = wait.clone();
                wait["worker_phase_id"] = json!(3);
                wait["origin"] = second["origin"].clone();
                wait
            })
            .collect();
        value["worker_waits"]
            .as_array_mut()
            .unwrap()
            .extend(additional);
        let report = summary(value.clone()).unwrap();
        assert_eq!(report["by_phase"].as_array().unwrap().len(), 2);
        assert_eq!(report["by_wait"].as_array().unwrap().len(), 14);
        assert_eq!(report["by_phase"][0]["local_concurrency"], 4);
        assert_eq!(report["by_phase"][1]["local_concurrency"], 8);
        value["phases"][1]["origin"]["concurrency"] = json!(4);
        value["phases"][1]["origin"]["ordinal"] = json!(1);
        let second_origin = value["phases"][1]["origin"].clone();
        for wait in &mut value["worker_waits"].as_array_mut().unwrap()[7..] {
            wait["origin"] = second_origin.clone();
        }
        let report = summary(value.clone()).unwrap();
        assert_eq!(report["by_phase"].as_array().unwrap().len(), 1);
        assert_eq!(report["by_phase"][0]["elapsed"]["recorded_samples"], 2);
        assert_eq!(report["by_wait"].as_array().unwrap().len(), 7);
        value["phases"][1]["outcome"] = json!("ok");
        let report = summary(value).unwrap();
        assert_eq!(report["by_phase"].as_array().unwrap().len(), 2);
        assert_eq!(report["by_wait"].as_array().unwrap().len(), 14);
    }
}
