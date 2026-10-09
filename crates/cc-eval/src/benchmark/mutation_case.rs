//! Self-contained deterministic mutation cases, independent fact assertions and
//! bounded failure-preserving stage minimization. Uses real parser/SQLite builds;
//! this layer is not a public-MCP or compiler oracle.
use super::{invalid, mutations::Mutation, oracle, report, Result};
use cc_server::engine::CodeIndex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FactAssertion {
    pub id: String,
    pub table: String,
    /// Exact columns; fixture authors supply expectations independently.
    pub matches: BTreeMap<String, Value>,
    pub count: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage {
    pub mutation: Mutation,
    #[serde(default)]
    pub reopen: bool,
    /// false observes an incomplete intermediate build, never certifies parity.
    pub settle: bool,
    #[serde(default)]
    pub assertions: Vec<FactAssertion>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationCase {
    pub schema_version: u32,
    pub name: String,
    pub initial: BTreeMap<String, String>,
    pub stages: Vec<Stage>,
    pub dirty_budget: usize,
    pub max_resume_builds: usize,
}
impl MutationCase {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1
            || self.name.is_empty()
            || self.name.len() > 120
            || self.initial.is_empty()
            || self.initial.len() > 512
            || self.stages.is_empty()
            || self.stages.len() > 128
            || !self.stages.last().is_some_and(|s| s.settle)
            || self.dirty_budget == 0
            || self.dirty_budget > 10000
            || self.max_resume_builds == 0
            || self.max_resume_builds > 1024
        {
            return Err(invalid("invalid bounded mutation case"));
        }
        let mut bytes = 0usize;
        let mut ids = BTreeSet::new();
        for (path, source) in &self.initial {
            super::mutations::allowed(path)?;
            bytes = bytes.saturating_add(source.len());
        }
        for stage in &self.stages {
            stage.mutation.validate()?;
            if let Mutation::Write { content, .. } = &stage.mutation {
                bytes = bytes.saturating_add(content.len());
            }
            if stage.assertions.len() > 128 {
                return Err(invalid("too many fact assertions"));
            }
            for a in &stage.assertions {
                if a.id.is_empty()
                    || a.id.len() > 120
                    || !ids.insert(&a.id)
                    || a.matches.is_empty()
                    || a.matches.len() > 16
                    || !oracle::tables().contains(&a.table.as_str())
                {
                    return Err(invalid("invalid/duplicate assertion identity or table"));
                }
            }
        }
        if serde_json::to_vec(self)?.len() > 8 * 1024 * 1024 {
            return Err(invalid("mutation case payload too large"));
        }
        if bytes > 4 * 1024 * 1024 {
            return Err(invalid("mutation case source budget exceeded"));
        }
        Ok(())
    }
}

fn build(index: &mut CodeIndex, full: bool) -> Result<cc_index::IndexReport> {
    let r = index
        .build_index(full)
        .map_err(|e| super::BenchError::Protocol(e.to_string()))?;
    if !r.parse_errors.is_empty() {
        return Err(invalid(
            "parse errors are not a comparable mutation outcome",
        ));
    }
    Ok(r)
}

/// Each replay starts from two fresh disposable projects. No ambient corpus,
/// source mutation, network, or answer data is passed into the product.
pub fn evaluate(case: &MutationCase) -> Result<Value> {
    evaluate_internal(case, false)
}

/// Additional retained observations for the separately registered isolated
/// profile study. The ordinary replay's wire format and decisions are unchanged.
pub fn evaluate_with_initial_evidence(case: &MutationCase) -> Result<Value> {
    evaluate_internal(case, true)
}

fn evaluate_internal(case: &MutationCase, retain_initial: bool) -> Result<Value> {
    case.validate()?;
    let a = tempfile::tempdir()?;
    let b = tempfile::tempdir()?;
    for root in [a.path(), b.path()] {
        for (path, content) in &case.initial {
            Mutation::Write {
                path: path.clone(),
                content: content.clone(),
            }
            .apply(root)?;
        }
        std::fs::write(root.join(".codecortex.json"),json!({"auto_index":{"enabled":false},"indexing":{"dirty_propagation_max_files":case.dirty_budget,"db_read_pool_size":1}}).to_string())?;
    }
    let open =
        |p: &Path| CodeIndex::new(Some(p)).map_err(|e| super::BenchError::Protocol(e.to_string()));
    let mut inc = open(a.path())?;
    let mut full = open(b.path())?;
    let initial_evidence = if retain_initial {
        let initial_incremental_report = build(&mut inc, true)?;
        let initial_full_report = build(&mut full, true)?;
        let initial_incremental = oracle::canonical(a.path())?;
        let initial_full = oracle::canonical(b.path())?;
        let initial_equal = initial_incremental == initial_full;
        if !initial_equal {
            return Err(invalid("initial independent rebuilds disagree"));
        }
        let evidence = json!({
            "incremental": initial_incremental, "full": initial_full,
            "incremental_report": initial_incremental_report, "full_report": initial_full_report,
            "process_snapshot": super::sampler::process_snapshot(std::process::id()),
            "resource_scope": "single worker snapshot, not peak or process-tree proof",
            "tables": oracle::tables(), "equal": initial_equal,
            "runtime_config": {"auto_index":{"enabled":false},"indexing":{
                "dirty_propagation_max_files":case.dirty_budget,"db_read_pool_size":1}},
        });
        if evidence["incremental_report"]["resolution_freshness"]["complete"] != true
            || evidence["full_report"]["resolution_freshness"]["complete"] != true
        {
            return Err(invalid("isolated fanout initial full builds are incomplete"));
        }
        Some(evidence)
    } else {
        build(&mut inc, true)?;
        build(&mut full, true)?;
        let initial_equal = oracle::canonical(a.path())? == oracle::canonical(b.path())?;
        if !initial_equal {
            return Err(invalid("initial independent rebuilds disagree"));
        }
        None
    };
    let mut columns = BTreeMap::new();
    for assertion in case.stages.iter().flat_map(|s| &s.assertions) {
        if !columns.contains_key(&assertion.table) {
            columns.insert(
                assertion.table.clone(),
                oracle::table_columns(a.path(), &assertion.table)?,
            );
        }
        if assertion
            .matches
            .keys()
            .any(|k| !columns[&assertion.table].contains(k))
        {
            return Err(invalid("unknown asserted column"));
        }
    }
    let mut checkpoints = Vec::new();
    let mut failure: Option<String> = None;
    for (n, stage) in case.stages.iter().enumerate() {
        stage.mutation.apply(a.path())?;
        stage.mutation.apply(b.path())?;
        if stage.reopen {
            drop(inc);
            inc = open(a.path())?;
        }
        let first = build(&mut inc, false)?;
        let mut reports = vec![json!(first)];
        if stage.settle {
            for _ in 0..case.max_resume_builds {
                if reports.last().unwrap()["resolution_freshness"]["complete"] == true {
                    break;
                }
                reports.push(json!(build(&mut inc, false)?));
            }
        }
        let full_report = if retain_initial {
            Some(json!(build(&mut full, true)?))
        } else {
            build(&mut full, true)?;
            None
        };
        if full_report.as_ref().is_some_and(|report| {
            report["resolution_freshness"]["complete"] != true
        }) {
            return Err(invalid("isolated fanout final full build is incomplete"));
        }
        let ca = oracle::canonical(a.path())?;
        let cb = oracle::canonical(b.path())?;
        let complete = reports.last().unwrap()["resolution_freshness"]["complete"] == true;
        let differences: Vec<_> = oracle::tables()
            .iter()
            .filter(|t| ca.get(**t) != cb.get(**t))
            .copied()
            .collect();
        let mut truth = Vec::new();
        for assertion in &stage.assertions {
            let rows = &ca[&assertion.table];
            // Misspelled columns cannot accidentally pass a zero-count assertion.
            if rows
                .iter()
                .any(|r| assertion.matches.keys().any(|k| r.get(k).is_none()))
            {
                return Err(invalid("unknown asserted column"));
            }
            let found = rows
                .iter()
                .filter(|r| assertion.matches.iter().all(|(k, v)| r.get(k) == Some(v)))
                .count();
            let passed = found == assertion.count;
            truth.push(json!({"id":assertion.id,"expected":assertion.count,"actual":found,"passed":passed}));
            if !passed && failure.is_none() {
                failure = Some(format!("truth:{}", assertion.id));
            }
        }
        if stage.settle && !complete && failure.is_none() {
            failure = Some("incomplete_after_resume_budget".into());
        }
        if complete && !differences.is_empty() && failure.is_none() {
            failure = Some(format!("parity:{}", differences[0]));
        }
        checkpoints.push(json!({"stage":n,"status":if complete{"compared"}else{"incomplete_not_certified"},"settle_required":stage.settle,"different_tables":differences,"truth":truth,"reports":reports,
            "incremental":ca,"full":cb}));
        if retain_initial {
            let checkpoint = checkpoints.last_mut().unwrap();
            checkpoint["full_report"] = full_report.unwrap();
            checkpoint["process_snapshot"] = json!(super::sampler::process_snapshot(std::process::id()));
        }
        if failure.is_some() {
            break;
        }
    }
    let mut result = json!({"schema_version":1,"evidence_layer":"actual_parser_sqlite_with_independent_fact_assertions","name":case.name,
        "passed":failure.is_none(),"failure_signature":failure,"checkpoints":checkpoints,"tables":oracle::tables()});
    if let Some(initial) = initial_evidence {
        result["initial_evidence"] = initial;
    }
    Ok(result)
}

/// Deterministic 1-minimal stage-deletion search, subject to a strict evaluation
/// budget. Invalid candidates/errors never stand in for the original failure.
/// Not a globally minimal program or a compiler-guided source reducer.
pub fn shrink(
    case: &MutationCase,
    signature: &str,
    max_attempts: usize,
) -> Result<(MutationCase, Value)> {
    if max_attempts > 128 {
        return Err(invalid("shrink budget exceeds 128"));
    }
    let mut current = case.clone();
    let mut attempts = 0;
    let mut removed = 0;
    let mut i = 0;
    let initial = evaluate(&current)?;
    if initial["failure_signature"].as_str() != Some(signature) {
        return Err(invalid("failure did not reproduce before reduction"));
    }
    while current.stages.len() > 1 && i < current.stages.len() && attempts < max_attempts {
        let mut candidate = current.clone();
        candidate.stages.remove(i);
        attempts += 1;
        let preserves = evaluate(&candidate)
            .ok()
            .is_some_and(|r| r["failure_signature"].as_str() == Some(signature));
        if preserves {
            current = candidate;
            removed += 1;
            i = 0;
        } else {
            i += 1;
        }
    }
    let check = evaluate(&current)?;
    if check["failure_signature"].as_str() != Some(signature) {
        return Err(invalid("minimized failure was not reproducible"));
    }
    let done = i >= current.stages.len() || current.stages.len() == 1;
    Ok((
        current,
        json!({"attempts":attempts,"removed_stages":removed,"budget":max_attempts,"stage_deletion_minimal":done,"reproduced":true,"failure_signature":signature}),
    ))
}

pub fn run(case: &MutationCase, out: &Path, shrink_attempts: usize) -> Result<Value> {
    if shrink_attempts > 128 {
        return Err(invalid("shrink budget exceeds 128"));
    }
    if out.exists() {
        return Err(invalid("mutation output exists; raw evidence is immutable"));
    }
    std::fs::create_dir_all(out)?;
    report::json(&out.join("case.json"), case)?;
    let mut result = evaluate(case)?;
    report::json(&out.join("result.json"), &result)?;
    if let Some(signature) = result["failure_signature"].as_str().map(str::to_owned) {
        if shrink_attempts > 0 {
            let (minimal, receipt) = shrink(case, &signature, shrink_attempts)?;
            report::json(&out.join("minimal-case.json"), &minimal)?;
            report::json(&out.join("shrink.json"), &receipt)?;
            result["shrink"] = receipt;
        }
    }
    Ok(result)
}
