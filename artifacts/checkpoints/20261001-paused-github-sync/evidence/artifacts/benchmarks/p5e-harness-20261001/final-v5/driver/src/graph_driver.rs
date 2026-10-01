//! Independent typed/directed public graph oracle, never merged-neighbor scoring.
use crate::mixed_stdio::{call, terminate_owned};
use rmcp::{
    transport::{async_rw::AsyncRwTransport, ConfigureCommandExt},
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::{path::Path, process::Stdio, sync::Arc, time::Duration};
fn check(q: &Value, raw: &Value) -> Value {
    let mut actual = raw["results"].as_array().cloned().unwrap_or_default();
    let mut expected = q["expected"].as_array().cloned().unwrap();
    actual.sort_by_key(Value::to_string);
    expected.sort_by_key(Value::to_string);
    let valid_shape = raw["results"].is_array()
        && raw["truncated"] == false
        && raw["row_count"].as_u64() == Some(actual.len() as u64);
    json!({"passed":valid_shape&&actual==expected,"kind":q["kind"],"actual_ordered_edges":actual,"expected_ordered_edges":expected,"scope":"actual typed MATCH relationship,directed ordered source-target identity,file-path hard negatives"})
}
// Restricted generated Rust fixture witness, not a general Rust parser.
fn declaration(source: &str, name: &str) -> Option<String> {
    let start = source.find(&format!("pub fn {name}("))?;
    let open = start + source[start..].find('{')?;
    let mut depth = 0usize;
    for (offset, ch) in source[open..].char_indices() {
        if ch == '{' {
            depth += 1;
        }
        if ch == '}' {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                return Some(source[start..open + offset + 1].to_string());
            }
        }
    }
    None
}
fn typed_source_witness(q: &Value, root: &Path) -> Value {
    let mut proofs = Vec::new();
    for edge in q["expected"].as_array().unwrap() {
        let source_path = edge["source_file"].as_str().unwrap();
        let target_path = edge["target_file"].as_str().unwrap();
        let a = std::fs::read_to_string(root.join(source_path)).unwrap();
        let b = std::fs::read_to_string(root.join(target_path)).unwrap();
        let source = declaration(&a, edge["source_name"].as_str().unwrap());
        let target = declaration(&b, edge["target_name"].as_str().unwrap());
        let calls = source.as_ref().is_some_and(|body| {
            body[body.find('{').unwrap()..]
                .contains(&format!("{}(", edge["target_name"].as_str().unwrap()))
        });
        proofs.push(json!({"edge":edge,"source_file_blake3":blake3::hash(a.as_bytes()).to_hex().to_string(),"target_file_blake3":blake3::hash(b.as_bytes()).to_hex().to_string(),"source_complete_declaration":source,"target_complete_declaration":target,"actual_call_direction":calls,"passed":calls&&target.is_some()}));
    }
    json!({"passed":proofs.iter().all(|p|p["passed"]==true),"proofs":proofs,"scope":"generated Rust CALLS fixture only;negative edge absence checked separately by public typed query"})
}
pub async fn run(binary: &Path, plan_path: &Path, out: &Path) -> i32 {
    assert!(!out.exists());
    std::fs::create_dir_all(out).unwrap();
    let bytes = std::fs::read(plan_path).unwrap();
    let plan: Value = serde_json::from_slice(&bytes).unwrap();
    super::write(&out.join("plan.json"), &plan);
    let root = tempfile::tempdir().unwrap();
    for (p, s) in plan["files"].as_object().unwrap() {
        cc_eval::benchmark::validation::relative_path(p).unwrap();
        let f = root.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s.as_str().unwrap()).unwrap();
    }
    let config = plan
        .get("engine_config")
        .cloned()
        .unwrap_or_else(|| json!({"auto_index":{"enabled":false}}));
    std::fs::write(
        root.path().join(".codecortex.json"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    let binary = binary.canonicalize().unwrap();
    let project = root.path().to_path_buf();
    super::write(
        &out.join("input-lock.json"),
        &json!({"plan_blake3":blake3::hash(&bytes).to_hex().to_string(),"binary_blake3":blake3::hash(&std::fs::read(&binary).unwrap()).to_hex().to_string(),"harness_blake3":blake3::hash(&std::fs::read(std::env::current_exe().unwrap()).unwrap()).to_hex().to_string(),"profile":"independent fixed typed graph fixture,not holdout"}),
    );
    let stderr = std::fs::File::create(out.join("server-stderr.log")).unwrap();
    let mut child = tokio::process::Command::new(&binary)
        .configure(|cmd| {
            for (k, _) in std::env::vars_os() {
                if k.to_string_lossy().starts_with("CODECORTEX_") {
                    cmd.env_remove(k);
                }
            }
            cmd.env("HOME", &project)
                .env("XDG_CONFIG_HOME", project.join(".config"))
                .env("XDG_CACHE_HOME", project.join(".cache"))
                .env("CODECORTEX_PPID_POLL_MS", "0")
                .arg("mcp")
                .arg("--project-path")
                .arg(&project)
                .current_dir(&project)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped());
        })
        .stderr(Stdio::from(stderr))
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let transport = AsyncRwTransport::<RoleClient, _, _>::new(
        child.stdout.take().unwrap(),
        child.stdin.take().unwrap(),
    );
    let service = match tokio::time::timeout(Duration::from_secs(30), ().serve(transport)).await {
        Ok(Ok(s)) => Arc::new(s),
        e => {
            super::write(
                &out.join("failure.json"),
                &json!({"stage":"initialize","raw_debug":format!("{e:?}")}),
            );
            let t = terminate_owned(None, &mut child).await;
            super::write(&out.join("termination.json"), &t);
            return 2;
        }
    };
    let build = call(&service, "index", json!({"path":project,"full":true})).await;
    super::write(
        &out.join("prepare.json"),
        &json!({"raw_response":build.raw_response,"result":build.result.as_ref().ok(),"error":build.result.as_ref().err()}),
    );
    // Native-only bounded observer; sampling is not a protocol request workload.
    let owned_pid = child.id();
    let sample_start = std::time::Instant::now();
    let sampling_stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sampler_stop = sampling_stop.clone();
    let sampler_task = std::thread::spawn(move || {
        let mut samples = Vec::new();
        while !sampler_stop.load(std::sync::atomic::Ordering::Acquire) {
            let started_us = sample_start.elapsed().as_micros() as u64;
            let runner = cc_eval::benchmark::sampler::process_snapshot(std::process::id());
            let server = owned_pid.and_then(cc_eval::benchmark::sampler::process_snapshot);
            samples.push(json!({"started_us":started_us,"finished_us":sample_start.elapsed().as_micros() as u64,"runner":runner,"server":server}));
            std::thread::sleep(Duration::from_millis(50));
        }
        samples
    });
    let mut rows = Vec::new();
    let mut failed = usize::from(build.result.is_err());
    for q in plan["queries"].as_array().unwrap() {
        let response = call(&service, "graph_query", json!({"query":q["query"]})).await;
        let verdict = match &response.result {
            Ok(raw) => {
                let mut verdict = check(q, raw);
                if plan["fanout_timing"] == true {
                    let proof = typed_source_witness(q, root.path());
                    verdict["passed"] = json!(verdict["passed"] == true && proof["passed"] == true);
                    verdict["source_witness"] = proof;
                }
                verdict
            }
            Err(_) => json!({"passed":false}),
        };
        if verdict["passed"] != true {
            failed += 1;
        }
        rows.push(json!({"id":q["id"],"query":q["query"],"check":verdict,"raw":response.result.ok(),"raw_response":response.raw_response,"classification":response.classification}));
    }
    let mut timed = Vec::new();
    if plan["fanout_timing"] == true {
        for rep in 0..plan["repetitions"].as_u64().unwrap() {
            let started = std::time::Instant::now();
            let response = call(
                &service,
                "search",
                json!({"query":plan["query"],"top_k":plan["top_k"],"mode":"hybrid"}),
            )
            .await;
            let elapsed_us = started.elapsed().as_micros() as u64;
            let witness = match &response.result {
                Ok(raw) => check_fanout_body(&plan, raw, root.path()),
                Err(_) => json!({"passed":false,"status":"error"}),
            };
            if witness["passed"] != true {
                failed += 1;
            }
            timed.push(json!({"repetition":rep,"elapsed_us":elapsed_us,"check":witness,"raw":response.result.ok(),"raw_response":response.raw_response,"classification":response.classification}));
        }
        super::write(&out.join("whole-query-observations.json"), &json!(timed));
    }
    let q = &plan["source_query"];
    let body = call(
        &service,
        "search",
        json!({"query":q["query"],"top_k":q["top_k"],"mode":"hybrid"}),
    )
    .await;
    let source_check = match &body.result {
        Ok(raw) if plan["fanout_timing"] == true => check_fanout_body(&plan, raw, root.path()),
        Ok(raw) => graph_task_inventory(q, raw, root.path()),
        Err(_) => json!({"passed":false}),
    };
    if source_check
        .get("locked_task_evidence_complete")
        .unwrap_or(&source_check["passed"])
        != &json!(true)
    {
        failed += 1;
    }
    let drift: Vec<_> = plan["files"]
        .as_object()
        .unwrap()
        .iter()
        .filter_map(|(p, s)| {
            (std::fs::read(root.path().join(p)).ok().as_deref()
                != Some(s.as_str().unwrap().as_bytes()))
            .then_some(p)
        })
        .collect();
    if !drift.is_empty() {
        failed += 1;
    }
    super::write(&out.join("observations.json"), &json!(rows));
    super::write(
        &out.join("source-body.json"),
        &json!({"raw":body.result.ok(),"raw_response":body.raw_response,"check":source_check,"drifted_files":drift}),
    );
    sampling_stop.store(true, std::sync::atomic::Ordering::Release);
    let resource_samples = sampler_task.join().unwrap();
    super::write(
        &out.join("resource-samples.json"),
        &json!({"interval_target_ms":50,"server_pid":owned_pid,"runner_pid":std::process::id(),"samples":resource_samples,"scope":"native actual PIDs;all missing intervals retained;observed max not absolutepeak or process-tree; checked CPU by locked external validator"}),
    );
    let lifecycle = terminate_owned(Some(service), &mut child).await;
    let passed = failed == 0 && lifecycle["lifecycle_passed"] == true;
    super::write(&out.join("termination.json"), &lifecycle);
    super::write(
        &out.join("summary.json"),
        &json!({"status":if passed{"passed_independent_typed_graph_fixture"}else{"failed"},"exit_code":if passed{0}else{1},"source_inventory_status":source_check["inventory_status"],"source_legacy_strict_pass":source_check["legacy_strict_pass"],"locked_source_task_complete":source_check["locked_task_evidence_complete"],"queries":rows.len(),"whole_query_samples":timed.len(),"whole_query_latency":cc_eval::benchmark::statistics::distribution(&timed.iter().map(|r|r["elapsed_us"].as_u64().unwrap()).collect::<Vec<_>>()),"failed_checks":failed,"lifecycle":lifecycle,"limits":["finite hand-read graph+body fixture,not real-repository/holdout","typed CALLS ordered endpoints and hard negatives,not merged-neighbor set"]}),
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
    assert_eq!(
        lock["plan_blake3"],
        blake3::hash(&bytes).to_hex().to_string()
    );
    let rows: Vec<Value> =
        serde_json::from_slice(&std::fs::read(observed.join("observations.json")).unwrap())
            .unwrap();
    let mut mismatches = Vec::new();
    let expected_ids: std::collections::BTreeSet<_> = plan["queries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|q| q["id"].to_string())
        .collect();
    let actual_ids: std::collections::BTreeSet<_> =
        rows.iter().map(|r| r["id"].to_string()).collect();
    if rows.len() != expected_ids.len() || actual_ids != expected_ids {
        mismatches.push(json!({"field":"typed_query_count_and_unique_ids"}));
    }
    for r in &rows {
        let q = plan["queries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["id"] == r["id"])
            .unwrap();
        let verdict = if r["raw"].is_null() {
            json!({"passed":false})
        } else {
            let mut verdict = check(q, &r["raw"]);
            if plan["fanout_timing"] == true {
                let witness_root = tempfile::tempdir().unwrap();
                for (p, s) in plan["files"].as_object().unwrap() {
                    std::fs::write(witness_root.path().join(p), s.as_str().unwrap()).unwrap();
                }
                let proof = typed_source_witness(q, witness_root.path());
                verdict["passed"] = json!(verdict["passed"] == true && proof["passed"] == true);
                verdict["source_witness"] = proof;
            }
            verdict
        };
        if verdict != r["check"] {
            mismatches.push(json!({"id":r["id"],"field":"typed_ordered_edge_check","actual":verdict,"recorded":r["check"]}));
        }
    }
    let root = tempfile::tempdir().unwrap();
    for (p, s) in plan["files"].as_object().unwrap() {
        std::fs::write(root.path().join(p), s.as_str().unwrap()).unwrap();
    }
    let source: Value =
        serde_json::from_slice(&std::fs::read(observed.join("source-body.json")).unwrap()).unwrap();
    let computed = if source["raw"].is_null() {
        json!({"passed":false})
    } else {
        if plan["fanout_timing"] == true {
            check_fanout_body(&plan, &source["raw"], root.path())
        } else {
            graph_task_inventory(&plan["source_query"], &source["raw"], root.path())
        }
    };
    if computed != source["check"] {
        mismatches.push(json!({"field":"independent_raw_source_body","actual":computed,"recorded":source["check"]}));
    }
    if plan["fanout_timing"] == true {
        let timed: Vec<Value> = serde_json::from_slice(
            &std::fs::read(observed.join("whole-query-observations.json")).unwrap(),
        )
        .unwrap();
        if timed.len() != plan["repetitions"].as_u64().unwrap() as usize {
            mismatches.push(json!({"field":"expected_whole_query_N"}));
        }
        let mut repetitions = std::collections::BTreeSet::new();
        for r in &timed {
            if !r["repetition"]
                .as_u64()
                .is_some_and(|n| n < plan["repetitions"].as_u64().unwrap() && repetitions.insert(n))
            {
                mismatches.push(
                    json!({"field":"unique_whole_query_repetition","actual":r["repetition"]}),
                );
            }
            let check = if r["raw"].is_null() {
                json!({"passed":false,"status":"error"})
            } else {
                check_fanout_body(&plan, &r["raw"], root.path())
            };
            if check != r["check"] {
                mismatches
                    .push(json!({"field":"whole_query_source_guard","repetition":r["repetition"]}));
            }
        }
    }
    let t: Value =
        serde_json::from_slice(&std::fs::read(observed.join("termination.json")).unwrap()).unwrap();
    let summary: Value =
        serde_json::from_slice(&std::fs::read(observed.join("summary.json")).unwrap()).unwrap();
    let live_ok = t["cancel_ok"] == true
        && t["forced"] == false
        && t["reaped"] == true
        && t["exit_code"] == 0;
    if t["lifecycle_passed"] != live_ok
        || summary["lifecycle"] != t
        || (summary["exit_code"] == 0 && !live_ok)
    {
        mismatches.push(json!({"field":"normal_lifecycle"}));
    }
    super::write(
        output,
        &json!({"status":if mismatches.is_empty(){"replay_consistent"}else{"invalid_measurement"},"queries":rows.len(),"mismatches":mismatches}),
    );
    if mismatches.is_empty() {
        0
    } else {
        2
    }
}

fn check_fanout_body(plan: &Value, raw: &Value, root: &Path) -> Value {
    let (mut hits, status) = match cc_eval::benchmark::normalizer::mcp(raw) {
        Ok(v) => v,
        Err(e) => {
            return json!({"passed":false,"status":"invalid_measurement","error":e.to_string()})
        }
    };
    let mut verified = Vec::new();
    for h in &mut hits {
        let error = cc_eval::benchmark::normalizer::verify_source(h, root)
            .err()
            .map(|e| e.to_string());
        verified.push(json!({"path":h.path,"valid":h.evidence_valid,"error":error,"span":h.span}));
    }
    let seed_witness = hits.iter().any(|h| {
        if h.evidence_valid != Some(true) {
            return false;
        }
        plan["seed_names"].as_array().unwrap().iter().any(|name| {
            let prefix = format!("pub fn {}(", name.as_str().unwrap());
            plan["files"]
                .as_object()
                .unwrap()
                .iter()
                .any(|(path, source)| {
                    path == &h.path
                        && source
                            .as_str()
                            .unwrap()
                            .lines()
                            .find(|line| line.starts_with(&prefix))
                            .is_some_and(|whole| {
                                h.text.as_deref().is_some_and(|text| text.contains(whole))
                            })
                })
        })
    });

    let required_anchors: Vec<_> = plan["source_query"]["representative_anchor_diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|facet| {
            let covered = hits.iter().any(|h| {
                h.evidence_valid == Some(true)
                    && h.path == facet["path"].as_str().unwrap()
                    && h.text
                        .as_deref()
                        .is_some_and(|text| text.contains(facet["marker"].as_str().unwrap()))
            });
            json!({"path":facet["path"],"marker":facet["marker"],"covered":covered})
        })
        .collect();
    let required_complete =
        !required_anchors.is_empty() && required_anchors.iter().all(|a| a["covered"] == true);
    let graph_receipts: Vec<_> = raw["evidence_summary"]["retrieval"]["lane_receipts"]
        .as_array()
        .map(|rows| rows.iter().filter(|r| r["lane_id"] == "graph").collect())
        .unwrap_or_default();
    let graph = graph_receipts.first().copied().unwrap_or(&Value::Null);
    let freshness = &raw["evidence_summary"]["source_freshness"];
    let graph_complete = graph_receipts.len() == 1
        && graph["status"] == "complete"
        && graph["coverage"]["complete"] == true
        && graph["truncation_reason"].is_null()
        && graph["candidate_count"].as_u64().is_some_and(|n| n > 0)
        && graph["weight"].as_f64().is_some_and(|w| w > 0.0);
    let fresh = freshness["partial"] == false
        && freshness["budget_exhausted"] == false
        && freshness["omitted_files"]
            .as_object()
            .is_some_and(|rows| rows.is_empty());
    let old_unmapped = raw.to_string().contains("graph_source_unmapped");
    json!({"representative_anchor_diagnostics":required_anchors,"both_diagnostic_anchors_covered":required_complete,"graph_mechanism_complete":graph_complete,"source_freshness_complete":fresh,"graph_receipt":graph,"mapped_chunk_candidates":graph["candidate_count"],"unmapped_symbol_count":Value::Null,"unmapped_count_limitation":"public receipt exposes reason,not exact unmapped symbol count;never default zero;complete receipt independently asserts completed mapping of attempted candidate domain","source_freshness":freshness,"graph_lane_receipts":raw["evidence_summary"]["retrieval"]["lane_receipts"],"graph_explain":raw["graph_explain"],"passed":graph_complete&&fresh&&seed_witness&&!old_unmapped&&hits.iter().all(|h|h.evidence_valid==Some(true)),"status":format!("{status:?}"),"seed_source_witness":seed_witness,"graph_source_unmapped":old_unmapped,"verified_source_spans":verified,"limits":"all status retained;packing inventory Partial remains explicit,separate strict quality gate—not rewritten as Success"})
}

/// Pre-registered task completeness does not redefine inventory completeness.
/// Only optional packing omissions may coexist with complete locked task proof.
fn graph_task_inventory(q: &Value, raw: &Value, root: &Path) -> Value {
    let mut check = super::evaluate_facets(q, raw, root);
    let legacy = check["passed"].clone();
    let inventory = check["status"].clone();
    let valid_body = check["facets"]
        .as_array()
        .is_some_and(|f| !f.is_empty() && f.iter().all(|f| f["covered"] == true))
        && check["verified_source_spans"]
            .as_array()
            .is_some_and(|hits| {
                !hits.is_empty()
                    && hits
                        .iter()
                        .all(|h| h["valid"] == true && h["verification_error"].is_null())
            });
    let freshness = &raw["evidence_summary"]["source_freshness"];
    let fresh = freshness["partial"] == false
        && freshness["budget_exhausted"] == false
        && freshness["omitted_files"]
            .as_object()
            .is_some_and(|f| f.is_empty());
    let lanes = &raw["evidence_summary"]["retrieval"]["lane_receipts"];
    let complete = lanes.as_array().is_some_and(|l| {
        !l.is_empty()
            && l.iter().all(|l| {
                l["status"] == "complete"
                    && l["coverage"]["complete"] == true
                    && l["truncation_reason"].is_null()
            })
    });
    let allowed_inventory = inventory == "Success"
        || (inventory == "Partial" && raw["evidence_summary"]["packing"]["partial"] == true);
    let no_foundation =
        raw["evidence_summary"]["retrieval"]["scope"]["hard"]["path_prefix_truncated"] == false
            && raw.pointer("/graph_explain/truncated") != Some(&json!(true));
    check["inventory_status"] = inventory;
    check["legacy_strict_pass"] = legacy;
    check["locked_task_evidence_complete"] =
        json!(valid_body && fresh && complete && allowed_inventory && no_foundation);
    check["boundary_contract"]=json!("round03 graph-task-inventory-boundary-review: complete required typed edges separately+all locked fullbody/span/source;Partial retained;only optional packing inventory omissions eligible");
    check
}
