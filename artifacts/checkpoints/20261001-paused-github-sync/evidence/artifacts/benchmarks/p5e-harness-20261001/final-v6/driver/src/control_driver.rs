//! Actual public-stdio control witness BEFORE any counterfactual measurement.
use crate::mixed_stdio::{call, terminate_owned};
use rmcp::{transport::async_rw::AsyncRwTransport, RoleClient, ServiceExt};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
fn cc_model_compatible_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && !path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
}
fn exact_query_set(plan: &Value) -> bool {
    plan["queries"].as_array().is_some_and(|queries| {
        queries.len() == 3
            && queries
                .iter()
                .map(|q| q["factor"].as_str())
                .collect::<std::collections::BTreeSet<_>>()
                == [
                    Some("path"),
                    Some("exact"),
                    Some("intent_aware_facet_reservation"),
                ]
                .into_iter()
                .collect()
            && queries
                .iter()
                .map(|q| q["id"].as_str())
                .collect::<std::collections::BTreeSet<_>>()
                == [
                    Some("path_behavior"),
                    Some("exact_behavior"),
                    Some("intent_behavior"),
                ]
                .into_iter()
                .collect()
    })
}
fn verdict(q: &Value, raw: &Value, root: &Path, enabled: &[String]) -> Value {
    let evidence = super::evaluate_facets(q, raw, root);
    let complete_body = evidence["facets"]
        .as_array()
        .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v["covered"] == true))
        && evidence["verified_source_spans"]
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(|v| v["valid"] == true));
    let retrieval = &raw["evidence_summary"]["retrieval"];
    let lane_rows = retrieval
        .get("lane_receipts")
        .or_else(|| retrieval.get("lanes"))
        .and_then(Value::as_array);
    let fresh = &raw["evidence_summary"]["source_freshness"];
    let packing = &raw["evidence_summary"]["packing"];
    let hard = &retrieval["scope"]["hard"];
    let scope_complete = hard.as_object().is_some_and(|h| {
        [
            "empty",
            "path_prefix",
            "languages",
            "explicit_file_count",
            "path_prefix_truncated",
        ]
        .iter()
        .all(|k| h.contains_key(*k))
    }) && hard["empty"] == false
        && hard["path_prefix"].is_null()
        && hard["languages"].is_null()
        && hard["explicit_file_count"].is_null()
        && hard["path_prefix_truncated"] == false;
    let fresh_complete = fresh["partial"] == false
        && fresh["budget_exhausted"] == false
        && fresh["omitted_files"]
            .as_object()
            .is_some_and(|a| a.is_empty());
    let packing_complete = packing["partial"] == false
        && packing["omitted_hits"] == 0
        && packing["omitted_nodes"] == 0;
    let local_complete = lane_rows.is_some_and(|lanes| {
        let ids: std::collections::BTreeSet<_> =
            lanes.iter().filter_map(|l| l["lane_id"].as_str()).collect();
        if lanes.len() != 5
            || ids
                != ["path", "exact_symbol", "lexical", "grep", "graph"]
                    .into_iter()
                    .collect()
        {
            return false;
        }
        lanes.iter().all(|l| {
            let id = l["lane_id"].as_str().unwrap();
            let allowed_disabled = id == "graph"
                || (id == "path" && !enabled.iter().any(|v| v == "path"))
                || (id == "exact_symbol"
                    && (!enabled.iter().any(|v| v == "exact") || q["factor"] != "exact"));
            if l["status"] == "disabled" {
                allowed_disabled && l["candidate_count"] == 0 && l["truncation_reason"].is_null()
            } else {
                l["status"] == "complete"
                    && l["coverage"]["complete"] == true
                    && l["truncation_reason"].is_null()
            }
        })
    });
    let all_source_paths = raw["machine_pack"]["hits"].as_array().is_some_and(|a| {
        a.iter().all(|h| {
            h["file_path"]
                .as_str()
                .is_some_and(cc_model_compatible_path)
        })
    });
    let foundation = scope_complete
        && fresh_complete
        && packing_complete
        && local_complete
        && all_source_paths
        && raw["invalidations"]
            .as_array()
            .is_some_and(|a| a.is_empty());
    let factor = q["factor"].as_str().unwrap();
    let on = enabled.iter().any(|id| id == factor);
    let selection = &raw["evidence_summary"]["selection"];
    let mechanism = if factor == "intent_aware_facet_reservation" {
        let actual = selection["intent"].as_str();
        let anchors = &selection["intent_facet_anchors"];
        let trusted = raw["machine_pack"]["hits"].as_array().is_some_and(|hits| {
            hits.iter()
                .any(|h| h["metadata"]["evidence_priority"] == "intent_facet")
        });
        let support = selection["source_support_anchors"]
            .as_array()
            .is_some_and(|a| !a.is_empty());
        let anchor_contract = if on {
            actual == Some("fix")
                && ["implementation", "test", "interface"]
                    .iter()
                    .all(|role| anchors[*role].as_str().is_some())
                && trusted
        } else {
            actual == Some("locate") && anchors.as_object().is_some_and(|a| a.is_empty())
        };
        anchor_contract
            && support
            && selection["original_ranks"]
                .as_array()
                .is_some_and(|a| a.first() == Some(&json!(1)))
    } else {
        let id = if factor == "exact" {
            "exact_symbol"
        } else {
            "path"
        };
        lane_rows.is_some_and(|lanes| {
            let rows: Vec<_> = lanes.iter().filter(|l| l["lane_id"] == id).collect();
            rows.len() == 1
                && if on {
                    rows[0]["status"] == "complete"
                        && rows[0]["coverage"]["complete"] == true
                        && rows[0]["candidate_count"].as_u64().is_some_and(|n| n > 0)
                        && rows[0]["truncation_reason"].is_null()
                } else {
                    rows[0]["status"] == "disabled" && rows[0]["candidate_count"] == 0
                }
        })
    };
    json!({"passed":complete_body&&mechanism&&foundation,"foundation_complete":foundation,"scope_complete":scope_complete,"freshness_complete":fresh_complete,"packing_no_body_omission":packing_complete,"declared_local_lanes_complete_or_intentionally_disabled":local_complete,"factor":factor,"registered_enabled":on,"complete_actual_source_body":complete_body,"actual_control_mechanism":mechanism,"body_source_checks":evidence,"actual_selection":selection,"actual_lanes":lane_rows,"actual_policy":retrieval["policy"],"limits":"Locate retains rank-anchor/defaultfill and source-support,not a fabricated implementation-anchor map;no corpus quality/timing claim"})
}
pub async fn run(binary: &Path, plan_path: &Path, enabled: &[String], out: &Path) -> i32 {
    assert!(!out.exists());
    std::fs::create_dir_all(out).unwrap();
    let bytes = std::fs::read(plan_path).unwrap();
    let plan: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        exact_query_set(&plan),
        "exact unique three-factor witness query set required"
    );
    let root = tempfile::tempdir().unwrap();
    for (p, text) in plan["files"].as_object().unwrap() {
        cc_eval::benchmark::validation::relative_path(p).unwrap();
        let file = root.path().join(p);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text.as_str().unwrap()).unwrap();
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        serde_json::to_vec(&plan["engine_config"]).unwrap(),
    )
    .unwrap();
    let binary = binary.canonicalize().unwrap();
    let stderr = std::fs::File::create(out.join("server-stderr.log")).unwrap();
    let mut command = tokio::process::Command::new(&binary);
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("CODECORTEX_") {
            command.env_remove(k);
        }
    }
    let mut child = command
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join(".config"))
        .env("XDG_CACHE_HOME", root.path().join(".cache"))
        .env("CODECORTEX_PPID_POLL_MS", "0")
        .arg("mcp")
        .arg("--project-path")
        .arg(root.path())
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    super::write(
        &out.join("input-lock.json"),
        &json!({"plan_blake3":blake3::hash(&bytes).to_hex().to_string(),"binary_blake3":blake3::hash(&std::fs::read(&binary).unwrap()).to_hex().to_string(),"harness_blake3":blake3::hash(&std::fs::read(std::env::current_exe().unwrap()).unwrap()).to_hex().to_string(),"registered_enabled":enabled,"scope":"premeasurement actualcontrol protocol/body/source witness;not latency"}),
    );
    let transport = AsyncRwTransport::<RoleClient, _, _>::new(
        child.stdout.take().unwrap(),
        child.stdin.take().unwrap(),
    );
    let service = match tokio::time::timeout(Duration::from_secs(30), ().serve(transport)).await {
        Ok(Ok(s)) => Arc::new(s),
        e => {
            super::write(
                &out.join("initialize-error.json"),
                &json!({"raw_debug":format!("{e:?}")}),
            );
            let termination = terminate_owned(None, &mut child).await;
            super::write(&out.join("termination.json"), &termination);
            return 2;
        }
    };
    let prepare = call(&service, "index", json!({"path":root.path(),"full":true})).await;
    let valid_build = prepare.result.as_ref().is_ok_and(|r| {
        r["files_parsed"].as_u64() == Some(plan["files"].as_object().unwrap().len() as u64)
            && r["files_skipped"] == 0
            && r["files_scanned"].as_u64() == Some(plan["files"].as_object().unwrap().len() as u64)
            && r["document_changes"]["files_projected"].as_u64()
                == Some(plan["files"].as_object().unwrap().len() as u64)
            && r["document_changes"]["render_failed"] == 0
            && r["parse_errors"].as_array().is_some_and(Vec::is_empty)
    });
    super::write(
        &out.join("prepare.json"),
        &json!({"raw_response":prepare.raw_response,"result":prepare.result.as_ref().ok(),"error":prepare.result.as_ref().err(),"passed":valid_build}),
    );
    let mut rows = Vec::new();
    for q in plan["queries"].as_array().unwrap() {
        let response = call(
            &service,
            "search",
            json!({"query":q["query"],"top_k":q["top_k"],"mode":"hybrid"}),
        )
        .await;
        let check = response
            .result
            .as_ref()
            .map(|raw| verdict(q, raw, root.path(), enabled))
            .unwrap_or_else(|_| json!({"passed":false,"reason":"protocol/tool/deadlinefailure"}));
        rows.push(json!({"id":q["id"],"raw":response.result.as_ref().ok(),"error":response.result.as_ref().err(),"raw_response":response.raw_response,"classification":response.classification,"check":check}));
    }
    super::write(&out.join("observations.json"), &json!(rows));
    let termination = terminate_owned(Some(service), &mut child).await;
    super::write(&out.join("termination.json"), &termination);
    let passed = valid_build
        && rows.len() == 3
        && rows.iter().all(|r| r["check"]["passed"] == true)
        && termination["lifecycle_passed"] == true;
    super::write(
        &out.join("summary.json"),
        &json!({"status":if passed {"actual_registered_three_controls_verified"}else{"control_witness_failed"},"passed":passed,"exit_code":if passed{0}else{1},"registered_enabled":enabled,"lifecycle":termination,"premeasurement_only":true}),
    );
    if passed {
        0
    } else {
        1
    }
}
pub fn replay(plan_path: &Path, observed: &Path, output: &Path) -> i32 {
    assert!(!output.exists());
    let bytes = std::fs::read(plan_path).unwrap();
    let plan: Value = serde_json::from_slice(&bytes).unwrap();
    let lock: Value =
        serde_json::from_slice(&std::fs::read(observed.join("input-lock.json")).unwrap()).unwrap();
    let enabled: Vec<String> = serde_json::from_value(lock["registered_enabled"].clone()).unwrap();
    assert!(
        exact_query_set(&plan),
        "exact unique three-factor witness query set required"
    );
    let root = tempfile::tempdir().unwrap();
    for (p, text) in plan["files"].as_object().unwrap() {
        let dest = root.path().join(p);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::write(dest, text.as_str().unwrap()).unwrap();
    }
    let rows: Vec<Value> =
        serde_json::from_slice(&std::fs::read(observed.join("observations.json")).unwrap())
            .unwrap();
    let mut errors = Vec::new();
    if lock["plan_blake3"] != blake3::hash(&bytes).to_hex().to_string() {
        errors.push(json!({"field":"input_plan_digest"}));
    }
    let expected: std::collections::BTreeSet<_> = plan["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].to_string())
        .collect();
    let actual: std::collections::BTreeSet<_> = rows.iter().map(|r| r["id"].to_string()).collect();
    if actual != expected || rows.len() != expected.len() {
        errors.push(json!({"field":"complete_unique_control_queries"}));
    }
    for row in &rows {
        let q = plan["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == row["id"]);
        let check = if let Some(q) = q {
            if row["raw"].is_null() {
                json!({"passed":false,"reason":"protocol/tool/deadlinefailure"})
            } else {
                verdict(q, &row["raw"], root.path(), &enabled)
            }
        } else {
            json!({"passed":false})
        };
        if check != row["check"] {
            errors.push(
                json!({"field":"actual_raw_control_body_proof","id":row["id"],"recomputed":check}),
            );
        }
    }
    let termination: Value =
        serde_json::from_slice(&std::fs::read(observed.join("termination.json")).unwrap()).unwrap();
    if !(termination["cancel_ok"] == true
        && termination["forced"] == false
        && termination["reaped"] == true
        && termination["exit_code"] == 0
        && termination["lifecycle_passed"] == true)
    {
        errors.push(json!({"field":"normal_owned_lifecycle"}));
    }
    super::write(
        output,
        &json!({"status":if errors.is_empty(){"control_replay_consistent"}else{"invalid_control_evidence"},"errors":errors}),
    );
    if errors.is_empty() {
        0
    } else {
        2
    }
}
