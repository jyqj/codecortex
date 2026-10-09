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
    #[arg(long)]
    plan: PathBuf,
    #[arg(long)]
    raw: PathBuf,
    #[arg(long)]
    output: PathBuf,
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

fn run(cli: Cli) -> Result<i32> {
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&cli.output)?;
    let result = (|| -> Result<Value> {
        if fs::metadata(&cli.plan)?.len() > 16 * 1024 * 1024
            || fs::metadata(&cli.raw)?.len() > MAX_RAW_BYTES
        {
            return Err(invalid(
                "runtime statistics input exceeds the retained evidence budget",
            ));
        }
        let plan_sha256 = file_sha256(&cli.plan)?;
        let raw_sha256 = file_sha256(&cli.raw)?;
        let plan: Plan = serde_json::from_slice(&fs::read(&cli.plan)?)?;
        let mut rows = Vec::new();
        for line in BufReader::new(fs::File::open(&cli.raw)?).lines() {
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
        if file_sha256(&cli.plan)? != plan_sha256 || file_sha256(&cli.raw)? != raw_sha256 {
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
