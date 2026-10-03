//! Whole-response budgets: retain complete source hits or explicit references.
//! No JSON prefix and no new source slice are produced here.
use super::BUDGET_SPEC;
use cc_model::{
    lane_receipt::LaneReceipt, retrieval::LaneOutcome, CcError, CcResult, ContextEnvelope,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub fn pack(envelope: ContextEnvelope, max_bytes: usize) -> CcResult<ContextEnvelope> {
    Ok(serde_json::from_value(pack_value(
        serde_json::to_value(envelope)?,
        max_bytes,
    )?)?)
}
fn bytes(value: &Value) -> CcResult<usize> {
    Ok(serde_json::to_vec(value)?.len())
}
fn measure(value: &mut Value) -> CcResult<usize> {
    // The size receipt and token estimate are part of their own budget.
    for _ in 0..8 {
        let size = bytes(value)?;
        value["token_estimate"] = json!(size.div_ceil(4));
        value["evidence_summary"]["packing"]["used_bytes"] = json!(size);
        if bytes(value)? == size {
            return Ok(size);
        }
    }
    Err(CcError::Search("budget accounting did not converge".into()))
}
fn keep(object: &mut Value, keys: &[&str]) {
    if let Some(map) = object.as_object_mut() {
        map.retain(|key, _| keys.contains(&key.as_str()));
    }
}
fn sync_hits(value: &mut Value) {
    let hits = value["machine_pack"]["hits"]
        .as_array()
        .expect("typed hits");
    let ids: BTreeSet<String> = hits
        .iter()
        .filter_map(|h| h["chunk_id"].as_str().map(|id| format!("search:{id}")))
        .collect();
    let files: BTreeSet<String> = hits
        .iter()
        .filter_map(|h| h["file_path"].as_str().map(str::to_owned))
        .collect();
    let count = hits.len();
    if let Some(nodes) = value["nodes"].as_array_mut() {
        nodes.retain(|n| {
            n["node_type"] != "SEARCH_HIT"
                || n["node_id"].as_str().is_some_and(|id| ids.contains(id))
        });
    }
    if let Some(spans) = value["spans"].as_array_mut() {
        spans.retain(|n| n["node_id"].as_str().is_some_and(|id| ids.contains(id)));
    }
    value["evidence_summary"]["search_hits"] = json!(count);
    value["evidence_summary"]["files"] = json!(files);
}
/// Also used after public-handler metadata is attached; extra envelope fields
/// participate in the same count rather than getting a free post-pack allowance.
pub fn pack_value(mut value: Value, max_bytes: usize) -> CcResult<Value> {
    let token_budget = value["token_budget"]
        .as_u64()
        .ok_or_else(|| CcError::InvalidParams("missing context token budget".into()))?;
    let cap = max_bytes.min(
        usize::try_from(token_budget)
            .unwrap_or(usize::MAX)
            .saturating_mul(4),
    );
    if cap < 1024 {
        return Err(CcError::InvalidParams(
            "context budget is too small for a structured receipt".into(),
        ));
    }
    if !value["machine_pack"]["hits"].is_array() || !value["evidence_summary"].is_object() {
        return Err(CcError::InvalidParams(
            "invalid context envelope for packing".into(),
        ));
    }
    if ["nodes", "spans"].iter().any(|key| !value[*key].is_array())
        || value["machine_pack"]
            .get("references")
            .is_some_and(|refs| !refs.is_array())
    {
        return Err(CcError::InvalidParams(
            "invalid structured context arrays".into(),
        ));
    }
    if value["machine_pack"]["hits"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| !h.is_object() || !h["metadata"].is_object())
        || ["nodes", "spans"].iter().any(|key| {
            value[*key]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| !v.is_object())
        })
    {
        return Err(CcError::InvalidParams(
            "invalid context evidence objects".into(),
        ));
    }
    if let Some(packing) = value["evidence_summary"].get("packing") {
        if packing.get("spec").and_then(Value::as_str) != Some(BUDGET_SPEC)
            || packing.get("partial").and_then(Value::as_bool).is_none()
        {
            return Err(CcError::InvalidParams(
                "invalid previous packing receipt".into(),
            ));
        }
    }
    // Idempotent final-boundary enforcement. Never reset an earlier omission.
    let previous = value["evidence_summary"]["packing"].clone();
    let original_hits = previous["original_hits"]
        .as_u64()
        .unwrap_or(value["machine_pack"]["hits"].as_array().unwrap().len() as u64);
    let original_nodes = previous["original_nodes"]
        .as_u64()
        .unwrap_or(value["nodes"].as_array().map_or(0, Vec::len) as u64);
    let mut omitted_hits = previous["omitted_hits"].as_u64().unwrap_or(0);
    let mut omitted_nodes = previous["omitted_nodes"].as_u64().unwrap_or(0);
    value["evidence_summary"]["packing"] = json!({
        "spec":BUDGET_SPEC,"limit_bytes":cap,"configured_max_bytes":max_bytes,
        "used_bytes":0,"token_estimate_method":"ceil(serialized_utf8_bytes/4); not a tokenizer",
        "original_hits":original_hits,"original_nodes":original_nodes,
        "omitted_hits":omitted_hits,"omitted_nodes":omitted_nodes,
        "partial":previous["partial"].as_bool().unwrap_or(false),
        "compacted":previous["compacted"].as_bool().unwrap_or(false),
        "continuation":"Use files region/expand on retained references; recheck source version."
    });
    for key in [
        "request_description_omitted",
        "metadata_omitted",
        "references_omitted",
        "machine_hit_nodes_omitted",
    ] {
        if let Some(flag) = previous.get(key) {
            value["evidence_summary"]["packing"][key] = flag.clone();
        }
    }
    if measure(&mut value)? <= cap {
        return Ok(value);
    }
    value["evidence_summary"]["packing"]["compacted"] = json!(true);

    // Remove duplicate source rendering only: each retained complete machine
    // hit still carries its original text, source proof and document identity.
    value["rendered_prompt"] =
        json!("Complete source is in machine_pack.hits; references have no source body.");
    if let Some(nodes) = value["nodes"].as_array_mut() {
        for node in nodes {
            if node["node_type"] == "SEARCH_HIT" {
                node["text"] = json!("");
                node["token_estimate"] = json!(0);
                node["metadata"] = json!({"representation":"machine_hit_reference"});
                node["reasons"] = json!([]);
                node["invalidation_keys"] = json!([]);
                node["span_kind"] = json!("reference");
            }
        }
    }
    if measure(&mut value)? <= cap {
        return Ok(value);
    }
    // Counts/status projection does not pretend omitted ranked candidates are
    // an empty completed lane. Its distinct field has a strict wire model.
    if let Some(raw) = value["evidence_summary"]["retrieval"]
        .as_object_mut()
        .and_then(|o| o.remove("lanes"))
    {
        let lanes: Vec<LaneOutcome> = serde_json::from_value(raw)?;
        let mut receipts = Vec::with_capacity(lanes.len());
        for lane in lanes {
            lane.validate()?;
            receipts.push(LaneReceipt::from(&lane));
        }
        value["evidence_summary"]["retrieval"]["lane_receipts"] = serde_json::to_value(receipts)?;
    }
    let mut reference_reasons = std::collections::BTreeMap::new();
    if let Some(hits) = value["machine_pack"]["hits"].as_array_mut() {
        for hit in hits {
            let file_reasons: Vec<String> = hit["metadata"]["stage_a_file_reasons"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .take(4)
                .map(|s| s.chars().take(128).collect())
                .collect();
            keep(
                &mut hit["metadata"],
                &[
                    "source_evidence",
                    "document",
                    "source_freshness",
                    "symbol_name",
                    "symbol_kind",
                    "qname",
                    "exact_identity",
                    "evidence_priority",
                ],
            );
            hit["metadata"]["details_omitted"] = json!(true);
            let file_reasons = if file_reasons.is_empty() {
                hit["reasons"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter(|s| s.starts_with("symbol:") || *s == "fts-summary")
                    .take(4)
                    .map(|s| s.chars().take(128).collect())
                    .collect()
            } else {
                file_reasons
            };
            if let Some(id) = hit["chunk_id"].as_str() {
                reference_reasons.insert(id.to_owned(), file_reasons);
            }
        }
    }
    // Numeric work counts remain intact; only verbose explanatory prose is
    // replaced by a reference to the documented receipt schema.
    if value
        .pointer("/evidence_summary/retrieval/cost/coverage")
        .is_some()
    {
        value["evidence_summary"]["retrieval"]["cost"]["coverage"] =
            json!("originating_work; full scope: retrieval-cost schema v1");
    }
    let policy = &mut value["evidence_summary"]["retrieval"]["policy"];
    if policy.is_object() {
        keep(
            policy,
            &[
                "version",
                "graph_source_mapping",
                "path_source_domain",
                "requested",
                "effective",
                "intent",
                "semantic_state",
                "deadline_ms",
                "lane_timeout_ms",
                "semantic_timeout_ms",
                "semantic_top_k",
            ],
        );
        // Compress only this known, equivalent explanation, not arbitrary
        // caller metadata. Keep scoped canonical/exact vs bounded fallback
        // and the non-whole-body domain before removing actual source.
        if policy["path_source_domain"].as_str() == Some(crate::query_policy::PATH_SOURCE_DOMAIN) {
            policy["path_source_domain"] =
                json!("canonical_scoped_exact_else_bounded_tokens;not_whole_body:v3");
        }
        if policy["graph_source_mapping"].as_str()
            == Some(crate::query_policy::GRAPH_SOURCE_MAPPING)
        {
            policy["graph_source_mapping"] = json!("uid_byte_decl_docs;not_whole_body:v2");
        }
        policy["details_omitted"] = json!(true);
    }
    // Exact production explanations only. Unknown/caller labels and every
    // numeric count, source identity and priority remain untouched. Labels are
    // versioned by QUERY_EXECUTION.md; reapplying this mapping is a no-op.
    for (pointer, known, compact) in [
        (
            "/evidence_summary/selection/source_support_scope",
            super::coverage::SOURCE_SUPPORT_SCOPE,
            "cue_window:v1",
        ),
        (
            "/evidence_summary/source_freshness/scope",
            crate::evidence_hydrator::SOURCE_FRESHNESS_SCOPE,
            "disk_generation:v1",
        ),
        (
            "/evidence_summary/source_freshness/scope",
            crate::evidence::SOURCE_FRESHNESS_SCOPE,
            "disk_files:v1",
        ),
    ] {
        if let Some(label) = value.pointer_mut(pointer) {
            if label.as_str() == Some(known) {
                *label = json!(compact);
            }
        }
    }
    // Verbose schema prose is not source evidence. Keep every scope/budget
    // value and the exact ordering semantics, but use documented terse labels
    // before dropping any body or required facet. No scan/error/partial count
    // is changed by this projection.
    let scope = &mut value["evidence_summary"]["retrieval"]["scope"];
    if scope.is_object() {
        scope["budget"]["units"] =
            json!("chunks; grep_scan_cap=decompressions; scope_receipt_v1; not_total_cost");
        scope["hard"]["semantics"] =
            json!("conjunctive_case_sensitive_repo_paths; explicit_empty_denies");
        scope["soft"]["contributions"] =
            json!("score_trace=additive; stage_a_already_included; raw_hint_paths_omitted");
        scope["ordering"] = json!(["exact_identity_first", "rerank_score_desc", "chunk_id_asc"]);
        scope["details_omitted"] = json!(true);
    }
    for field in ["task", "query", "summary"] {
        if value[field].as_str().is_some_and(|s| s.len() > 512) {
            value[field] = json!("Oversized request description omitted from bounded output.");
            value["evidence_summary"]["packing"]["request_description_omitted"] = json!(true);
        }
    }
    if value["machine_pack"]["query"]
        .as_str()
        .is_some_and(|s| s.len() > 512)
    {
        value["machine_pack"]["query"] = value["query"].clone();
    }
    // Detailed stale-file diagnostics are not allowed to crowd out valid
    // source. Keep their completeness state and counts, not a huge path map.
    if bytes(&value["evidence_summary"]["source_freshness"])? > cap / 8 {
        let freshness = &mut value["evidence_summary"]["source_freshness"];
        let omitted_file_count = freshness
            .get("omitted_files")
            .and_then(Value::as_object)
            .map(|m| m.len());
        keep(
            freshness,
            &[
                "partial",
                "checked_files",
                "budget_exhausted",
                "read_budget_charged",
                "generation",
                "hydrator",
                "verified_hits",
                "omitted_file_count",
            ],
        );
        freshness["details_omitted"] = json!(true);
        if let Some(count) = omitted_file_count {
            freshness["omitted_file_count"] = json!(count);
        }
    }
    sync_hits(&mut value);
    if measure(&mut value)? <= cap {
        return Ok(value);
    }
    // Source already lives in complete machine hits. Drop only their duplicate
    // node projection before evicting any evidence; keep graph descriptions.
    let nodes = value["nodes"].as_array_mut().unwrap();
    let duplicates = nodes
        .iter()
        .filter(|n| n["node_type"] == "SEARCH_HIT")
        .count();
    nodes.retain(|n| n["node_type"] != "SEARCH_HIT");
    let node_ids: BTreeSet<String> = nodes
        .iter()
        .filter_map(|n| n["node_id"].as_str().map(str::to_owned))
        .collect();
    value["spans"].as_array_mut().unwrap().retain(|s| {
        s["node_id"]
            .as_str()
            .is_some_and(|id| node_ids.contains(id))
    });
    let old_duplicates = previous["machine_hit_nodes_omitted"].as_u64().unwrap_or(0);
    value["evidence_summary"]["packing"]["machine_hit_nodes_omitted"] =
        json!(old_duplicates.saturating_add(duplicates as u64));
    if measure(&mut value)? <= cap {
        return Ok(value);
    }
    // Lower-ranked body references have strictly lower priority than retained
    // complete hits. Allocate them only AFTER selecting the full-hit prefix.
    // Otherwise references accumulated by tail eviction can evict rank one.
    let mut references = value["machine_pack"]
        .as_object_mut()
        .unwrap()
        .remove("references")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    value["machine_pack"]["references"] = json!([]);
    value["evidence_summary"]["packing"]["references_omitted"] = json!(true);
    // Keep the full source prefix first. Optional outlines are packed only
    // into residual space and cannot displace a fitting body.
    while measure(&mut value)? > cap {
        // Even a large derived graph description has lower priority than
        // the final complete primary hit. Its omission remains explicit.
        if value["machine_pack"]["hits"].as_array().unwrap().len() == 1
            && value["nodes"].as_array_mut().unwrap().pop().is_some()
        {
            omitted_nodes += 1;
            value["evidence_summary"]["packing"]["omitted_nodes"] = json!(omitted_nodes);
            value["evidence_summary"]["packing"]["partial"] = json!(true);
            continue;
        }
        let hits = value["machine_pack"]["hits"].as_array_mut().unwrap();
        // Preserve rank one and bounded required source-support evidence before
        // lower-priority incidental hits. Original ordering/scores stay intact;
        // if even these cannot fit, omission remains explicitly Partial.
        let remove = hits
            .iter()
            .enumerate()
            .rev()
            .find(|(index, hit)| {
                *index > 0
                    && !matches!(
                        hit["metadata"]["evidence_priority"].as_str(),
                        Some("intent_facet" | "distinctive_source_support")
                    )
            })
            .map(|(index, _)| index)
            .or_else(|| {
                hits.iter()
                    .enumerate()
                    .rev()
                    .find(|(index, hit)| {
                        *index > 0
                            && hit["metadata"]["evidence_priority"] == "distinctive_source_support"
                    })
                    .map(|(index, _)| index)
            })
            .or_else(|| hits.len().checked_sub(1));
        let hit = remove.map(|index| hits.remove(index));
        let Some(hit) = hit else {
            break;
        };
        let reference = json!({"kind":"document_reference","file_path":hit["file_path"],
            "symbol_name":hit["symbol_name"],"symbol_kind":hit["symbol_kind"],"language":hit["language"],
            "retrieval_reasons":hit["chunk_id"].as_str().and_then(|id| reference_reasons.get(id)),
            "source_digest":hit["metadata"]["source_evidence"]["source"]["content_digest"],
            "chunk_id":hit["chunk_id"],"document":hit["metadata"]["document"],
            "source_span":hit["metadata"]["source_evidence"]["span"],
            "body_omitted":true,"reason":"output_budget"});
        references.insert(0, reference);
        omitted_hits += 1;
        value["evidence_summary"]["packing"]["omitted_hits"] = json!(omitted_hits);
        value["evidence_summary"]["packing"]["partial"] = json!(true);
        sync_hits(&mut value);
        value["summary"] = json!("Bounded evidence response; omitted source is represented by references where space permits.");
    }
    // Remaining overhead can itself be oversized. Keep explicit omission
    // truth and give an error if even the minimal truthful envelope cannot fit.
    if measure(&mut value)? > cap {
        omitted_nodes += value["nodes"].as_array().unwrap().len() as u64;
        value["evidence_summary"]["packing"]["omitted_nodes"] = json!(omitted_nodes);
        value["nodes"] = json!([]);
        value["spans"] = json!([]);
        value["reasons"] = json!(["output budget omitted detailed context"]);
        value["invalidations"] = json!([]);
        let freshness = &mut value["evidence_summary"]["source_freshness"];
        if freshness.is_object() {
            keep(
                freshness,
                &[
                    "partial",
                    "checked_files",
                    "budget_exhausted",
                    "read_budget_charged",
                    "generation",
                    "hydrator",
                ],
            );
            freshness["details_omitted"] = json!(true);
        }
        value["evidence_summary"]["packing"]["partial"] = json!(true);
        value["evidence_summary"]["packing"]["metadata_omitted"] = json!(true);
    }
    if measure(&mut value)? > cap {
        return Err(CcError::Search(
            "required context metadata exceeds output budget".into(),
        ));
    }
    if references.is_empty() && previous["references_omitted"] != true {
        value["evidence_summary"]["packing"]
            .as_object_mut()
            .unwrap()
            .remove("references_omitted");
    }
    if !references.is_empty() {
        // Reserve the omission marker itself before optional references.
        value["evidence_summary"]["packing"]["references_omitted"] = json!(true);
        // Prefer sources absent from the retained bodies, then named entities.
        // Reference-only selection never rewrites source relevance scores.
        let represented: BTreeSet<String> = value["machine_pack"]["hits"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|h| h["file_path"].as_str().map(str::to_owned))
            .collect();
        references.sort_by_key(|r| {
            (
                r["file_path"]
                    .as_str()
                    .is_some_and(|p| represented.contains(p)),
                r.get("symbol_name").and_then(Value::as_str).is_none(),
            )
        });
        for reference in references.iter().take(8) {
            value["machine_pack"]["references"]
                .as_array_mut()
                .unwrap()
                .push(reference.clone());
            if measure(&mut value)? > cap {
                value["machine_pack"]["references"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
                // A file-level reference has no chunk/line claim but retains
                // its exact observed file digest and actionable diagnostics.
                let outline = json!({"kind":"file_reference","file_path":reference["file_path"],
                    "source_digest":reference["source_digest"],"retrieval_reasons":reference["retrieval_reasons"]});
                value["machine_pack"]["references"]
                    .as_array_mut()
                    .unwrap()
                    .push(outline);
                if measure(&mut value)? > cap {
                    value["machine_pack"]["references"]
                        .as_array_mut()
                        .unwrap()
                        .pop();
                }
            }
        }
        if value["machine_pack"]["references"]
            .as_array()
            .unwrap()
            .len()
            == references.len()
            && previous["references_omitted"] != true
        {
            value["evidence_summary"]["packing"]
                .as_object_mut()
                .unwrap()
                .remove("references_omitted");
        }
    }
    if measure(&mut value)? > cap {
        return Err(CcError::Search(
            "required context receipt exceeds output budget".into(),
        ));
    }
    Ok(value)
}
