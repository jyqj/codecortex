//! Bounded shared-session public stdio load. No production algorithm copy.
use cc_eval::benchmark::{ablation::MixedPlan, sampler, statistics};
use rmcp::{
    model::CallToolRequestParams,
    service::RunningService,
    transport::{async_rw::AsyncRwTransport, ConfigureCommandExt},
    RoleClient, ServiceExt,
};
use serde_json::{json, Value};
use std::{
    path::Path,
    process::Stdio,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
struct CallOutcome {
    result: Result<Value, String>,
    raw_response: Value,
    classification: &'static str,
}
async fn call(client: &RunningService<RoleClient, ()>, name: &str, args: Value) -> CallOutcome {
    let outcome = tokio::time::timeout(
        Duration::from_secs(30),
        client.call_tool(
            CallToolRequestParams::new(name.to_string())
                .with_arguments(args.as_object().unwrap().clone()),
        ),
    )
    .await;
    match outcome {
        Err(_) => CallOutcome {
            result: Err(format!("deadline_censored MCP {name}")),
            raw_response: json!({"kind":"deadline_censored","deadline_ms":30000}),
            classification: "deadline_censored",
        },
        Ok(Err(e)) => {
            let raw = match &e {
                rmcp::service::ServiceError::McpError(error) => {
                    serde_json::to_value(error).unwrap()
                }
                _ => json!({"debug":format!("{e:?}"),"message":e.to_string()}),
            };
            CallOutcome {
                result: Err(e.to_string()),
                raw_response: raw,
                classification: "rpc_or_transport_error",
            }
        }
        Ok(Ok(result)) => {
            let raw = serde_json::to_value(&result).unwrap();
            if result.is_error == Some(true) {
                return CallOutcome {
                    result: Err(format!("tool error {name}")),
                    raw_response: raw,
                    classification: "tool_error",
                };
            }
            match result
                .structured_content
                .and_then(|v| v.get("result").cloned())
            {
                Some(v) => CallOutcome {
                    result: Ok(v),
                    raw_response: raw,
                    classification: "success",
                },
                None => CallOutcome {
                    result: Err("missing structured result".into()),
                    raw_response: raw,
                    classification: "protocol_error",
                },
            }
        }
    }
}
fn valid_build(payload: &Value, admitted: usize) -> bool {
    payload["parse_errors"]
        .as_array()
        .is_some_and(Vec::is_empty)
        && payload["document_changes"]["render_failed"].as_u64() == Some(0)
        && payload["resolution_freshness"]["status"] == "ready"
        && payload["files_scanned"].as_u64() == Some(admitted as u64)
        && payload["files_parsed"].as_u64() == Some(admitted as u64)
        && payload["files_skipped"].as_u64() == Some(0)
        && payload["document_changes"]["files_projected"].as_u64() == Some(admitted as u64)
        && payload["chunks_total"]
            .as_u64()
            .is_some_and(|n| n >= admitted as u64)
        && payload["symbols_total"]
            .as_u64()
            .is_some_and(|n| n >= admitted as u64)
}
pub async fn run(plan: &MixedPlan, binary: &Path, out: &Path) -> i32 {
    assert!(!out.exists());
    std::fs::create_dir_all(out).unwrap();
    super::write(&out.join("plan.json"), &serde_json::to_value(plan).unwrap());
    assert!([1, 4, 8, 16].contains(&plan.concurrency));
    assert!(!plan.queries.is_empty());
    assert!(plan.repetitions * plan.queries.len() <= 4096);
    assert!(plan.build_every > 0 && plan.top_k > 0 && plan.offered_interval_us <= 1_000_000);
    let root = tempfile::tempdir().unwrap();
    for (p, s) in &plan.files {
        cc_eval::benchmark::validation::relative_path(p).unwrap();
        let f = root.path().join(p);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    }
    for q in &plan.queries {
        for f in &q.required_facets {
            assert!(plan.files[&f.path].contains(&f.marker));
        }
    }
    std::fs::write(
        root.path().join(".codecortex.json"),
        "{\"auto_index\":{\"enabled\":false}}",
    )
    .unwrap();
    let binary = binary.canonicalize().unwrap();
    let project = root.path().to_path_buf();
    super::write(
        &out.join("input-lock.json"),
        &json!({"binary_blake3":blake3::hash(&std::fs::read(&binary).unwrap()).to_hex().to_string(),"driver_blake3":blake3::hash(&std::fs::read(std::env::current_exe().unwrap()).unwrap()).to_hex().to_string(),"source_files":plan.files.iter().map(|(p,s)|json!({"path":p,"bytes":s.len(),"blake3":blake3::hash(s.as_bytes()).to_hex().to_string()})).collect::<Vec<_>>(),"adapter":"real_shared_session_rmcp_stdio","deadline_ms":30000}),
    );
    let stderr = std::fs::File::create(out.join("server-stderr.log")).unwrap();
    let mut child = tokio::process::Command::new(&binary)
        .configure(|cmd| {
            for (key, _) in std::env::vars_os() {
                if key.to_string_lossy().starts_with("CODECORTEX_") {
                    cmd.env_remove(key);
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
    let pid = child.id();
    let stdout = child.stdout.take().unwrap();
    let stdin = child.stdin.take().unwrap();
    let transport = AsyncRwTransport::<RoleClient, _, _>::new(stdout, stdin);
    let service = match tokio::time::timeout(Duration::from_secs(30), ().serve(transport)).await {
        Ok(Ok(s)) => Arc::new(s),
        other => {
            super::write(
                &out.join("failure.json"),
                &json!({"status":"invalid_measurement","stage":"initialize","error":format!("{other:?}")}),
            );
            let termination = terminate_owned(None, &mut child).await;
            super::write(&out.join("termination.json"), &termination);
            return 2;
        }
    };
    let prepared = call(&service, "index", json!({"path":project,"full":true})).await;
    super::write(
        &out.join("prepare-raw-response.json"),
        &prepared.raw_response,
    );
    match prepared.result {
        Ok(v) if valid_build(&v, plan.files.len()) => super::write(&out.join("prepare.json"), &v),
        Ok(v) => {
            super::write(
                &out.join("failure.json"),
                &json!({"status":"invalid_measurement","stage":"initial_index_workload","raw":v}),
            );
            let termination = terminate_owned(Some(service), &mut child).await;
            super::write(&out.join("termination.json"), &termination);
            return 2;
        }
        Err(e) => {
            super::write(
                &out.join("failure.json"),
                &json!({"status":"invalid_measurement","stage":"initial_index","error":e}),
            );
            let termination = terminate_owned(Some(service), &mut child).await;
            super::write(&out.join("termination.json"), &termination);
            return 2;
        }
    }
    let initial_status = call(&service, "status", json!({"aspect":"index"})).await;
    super::write(
        &out.join("initial-status.json"),
        &json!({"raw_response":initial_status.raw_response,"result":initial_status.result.as_ref().ok(),"error":initial_status.result.as_ref().err()}),
    );
    let threads_before = pid.and_then(sampler::thread_count);
    let resources_before = json!({"runner":sampler::process_snapshot(std::process::id()),"server":pid.and_then(sampler::process_snapshot),"method":"native PID process_snapshot only;ps/process-tree not_run"});
    let plan = Arc::new(plan.clone());
    let mut jobs = Vec::new();
    let mut reads = 0;
    for rep in 0..plan.repetitions {
        for qi in statistics::order(plan.queries.len(), plan.seed.wrapping_add(rep as u64)) {
            jobs.push(Some((qi, rep)));
            reads += 1;
            if reads % plan.build_every == 0 {
                jobs.push(None);
            }
        }
    }
    let jobs = Arc::new(jobs);
    let next = Arc::new(AtomicUsize::new(0));
    let rows = Arc::new(Mutex::new(Vec::new()));
    let root_path = Arc::new(root.path().to_path_buf());
    let start = Instant::now();
    let mut tasks = tokio::task::JoinSet::new();
    let sampling_stop = Arc::new(AtomicBool::new(false));
    let sampler_stop = sampling_stop.clone();
    let sampler_task = tokio::spawn(async move {
        let mut samples = Vec::new();
        while !sampler_stop.load(Ordering::Acquire) {
            let started_us = start.elapsed().as_micros() as u64;
            let sampled = tokio::task::spawn_blocking(move || {
                (
                    sampler::process_snapshot(std::process::id()),
                    pid.and_then(sampler::process_snapshot),
                )
            })
            .await;
            let finished_us = start.elapsed().as_micros() as u64;
            match sampled {
                Ok((runner,server))=>samples.push(json!({"started_us":started_us,"finished_us":finished_us,"runner":runner,"server":server})),
                Err(e)=>samples.push(json!({"started_us":started_us,"finished_us":finished_us,"runner":null,"server":null,"error":e.to_string()})),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        samples
    });

    for worker in 0..plan.concurrency {
        let (service, plan, jobs, next, rows, root_path) = (
            service.clone(),
            plan.clone(),
            jobs.clone(),
            next.clone(),
            rows.clone(),
            root_path.clone(),
        );
        tasks.spawn(async move{loop{
            let seq=next.fetch_add(1,Ordering::Relaxed);let Some(job)=jobs.get(seq)else{break;};
            let offered_us=(seq as u64).saturating_mul(plan.offered_interval_us);let target=Duration::from_micros(offered_us);if let Some(delay)=target.checked_sub(start.elapsed()){tokio::time::sleep(delay).await;}
            let started_us=start.elapsed().as_micros() as u64;
            let response=match job{Some((qi,_))=>call(&service,"search",json!({"query":plan.queries[*qi].query,"top_k":plan.top_k,"mode":"hybrid"})).await,None=>call(&service,"index",json!({"path":*root_path,"full":true})).await};
            let finished_us=start.elapsed().as_micros() as u64;
            let check=match(job,&response.result){(Some((qi,_)),Ok(payload))=>super::evaluate_facets(&serde_json::to_value(&plan.queries[*qi]).unwrap(),payload,&root_path),(_,Err(e))=>json!({"status":if e.starts_with("deadline_censored"){"Timeout"}else{"error"},"passed":false}),(None,Ok(payload))=>json!({"status":"build","passed":valid_build(payload,plan.files.len())})};
            rows.lock().unwrap().push(json!({"sequence":seq,"worker":worker,"kind":if job.is_some(){"read"}else{"full_build"},"query_id":job.map(|(qi,_)|&plan.queries[qi].id),"repetition":job.map(|(_,rep)|rep),"offered_us":offered_us,"started_us":started_us,"finished_us":finished_us,"client_queue_us":started_us.saturating_sub(offered_us),"service_and_transport_us":finished_us-started_us,"end_to_end_us":finished_us.saturating_sub(offered_us),"check":check,"originating_work":response.result.as_ref().ok().and_then(sampler::retrieval_work),"error":response.result.as_ref().err(),"raw":response.result.ok(),"raw_response":response.raw_response,"response_classification":response.classification}));
        }});
    }
    let mut worker_failures = Vec::new();
    while let Some(result) = tasks.join_next().await {
        if let Err(e) = result {
            worker_failures.push(e.to_string());
        }
    }
    let elapsed_us = start.elapsed().as_micros() as u64;
    sampling_stop.store(true, Ordering::Release);
    let resource_samples = sampler_task.await.unwrap();
    let threads_after = pid.and_then(sampler::thread_count);
    let resources_after = json!({"runner":sampler::process_snapshot(std::process::id()),"server":pid.and_then(sampler::process_snapshot),"method":"native PID process_snapshot only;ps/process-tree not_run"});
    let final_status = call(&service, "status", json!({"aspect":"index"})).await;
    super::write(
        &out.join("pid-confirmation.json"),
        &json!({"server_pid":pid,"same_running_service":true,"final_public_status_raw":final_status.raw_response,"classification":final_status.classification,"error":final_status.result.as_ref().err()}),
    );
    let mut rows = Arc::try_unwrap(rows).unwrap().into_inner().unwrap();
    rows.sort_by_key(|r| r["sequence"].as_u64());
    let source_drift: Vec<_> = plan
        .files
        .iter()
        .filter_map(|(p, s)| {
            (std::fs::read(root.path().join(p)).ok().as_deref() != Some(s.as_bytes())).then_some(p)
        })
        .collect();
    let workload_counts_ok = initial_status
        .result
        .as_ref()
        .ok()
        .zip(final_status.result.as_ref().ok())
        .is_some_and(|(a, b)| {
            a["indexed_files"] == plan.files.len()
                && b["indexed_files"] == plan.files.len()
                && a["indexed_chunks"].as_u64().is_some()
                && a["indexed_chunks"] == b["indexed_chunks"]
                && a["indexed_symbols"].as_u64().is_some()
                && a["indexed_symbols"] == b["indexed_symbols"]
                && rows.iter().filter(|r| r["kind"] == "full_build").all(|r| {
                    r["raw"]["chunks_total"] == a["indexed_chunks"]
                        && r["raw"]["symbols_total"] == a["indexed_symbols"]
                })
        });
    super::write(
        &out.join("workload-count-lock.json"),
        &json!({"admitted_files":plan.files.len(),"initial_status":initial_status.result.as_ref().ok(),"final_status":final_status.result.as_ref().ok(),"initial_final_and_all_fullbuild_counts_identical":workload_counts_ok,"scope":"public report parsed/skipped/projected/scanned/chunks/symbols and public actual indexed counts,not mere scanner workload"}),
    );
    let failures = usize::from(!workload_counts_ok)
        + usize::from(final_status.result.is_err())
        + rows.iter().filter(|r| r["check"]["passed"] != true).count()
        + worker_failures.len()
        + usize::from(!source_drift.is_empty());
    let read_times: Vec<_> = rows
        .iter()
        .filter(|r| r["kind"] == "read")
        .map(|r| r["end_to_end_us"].as_u64().unwrap())
        .collect();
    let build_times: Vec<_> = rows
        .iter()
        .filter(|r| r["kind"] == "full_build")
        .map(|r| r["end_to_end_us"].as_u64().unwrap())
        .collect();
    let mut per_query = serde_json::Map::new();
    for q in &plan.queries {
        let group: Vec<_> = rows.iter().filter(|r| r["query_id"] == q.id).collect();
        let values: Vec<_> = group
            .iter()
            .map(|r| r["end_to_end_us"].as_u64().unwrap())
            .collect();
        per_query.insert(q.id.clone(),json!({"requests":group.len(),"failed":group.iter().filter(|r|r["check"]["passed"]!=true).count(),"latency":statistics::distribution(&values)}));
    }
    super::write(&out.join("observations.json"), &json!(rows));
    super::write(
        &out.join("resource-samples.json"),
        &json!({"interval_target_ms":50,"server_pid":pid,"runner_pid":std::process::id(),"samples":resource_samples,"scope":"actual PIDs; each sample start/end and missing-null retained; maxima are observed sampled values not absolute peaks; CPU ns only owner calibrated platform"}),
    );
    super::write(
        &out.join("resource-summary.json"),
        &resource_summary(&resource_samples, pid),
    );
    super::write(
        &out.join("resources.json"),
        &json!({"before":resources_before,"after":resources_after,"server_threads_before":threads_before,"server_threads_after":threads_after,"scope":"actual child server; before/after are snapshots not peak or continuous interval"}),
    );
    super::write(
        &out.join("summary.json"),
        &json!({"status":"pending_process_termination","exit_code":2,"files":plan.files.len(),"source_bytes":plan.files.values().map(String::len).sum::<usize>(),"concurrency":plan.concurrency,"offered_jobs":jobs.len(),"completed_jobs":rows.len(),"elapsed_us":elapsed_us,"completed_requests_per_second":rows.len() as f64/(elapsed_us as f64/1e6),"failures":failures,"worker_failures":worker_failures,"source_drift":source_drift,"read_latency":statistics::distribution(&read_times),"build_latency":statistics::distribution(&build_times),"per_query":per_query,"limitations":["same-source full rebuild,not mutation freshness","client queue includes worker admission;server queue not independently observable","all error/Partial/timeout retained in latency;deadline censored explicit","originating work receipts are not actual cache-hit cost","thread/resident samples use 50ms target plus bounded-probe time;not tree/absolute peaks","homogeneous per-query N and pooled profile N separate;no p99 certification"]}),
    );
    let lifecycle = terminate_owned(Some(service), &mut child).await;
    super::write(&out.join("termination.json"), &lifecycle);
    let passed = failures == 0 && rows.len() == jobs.len() && lifecycle["lifecycle_passed"] == true;
    let mut final_summary: Value =
        serde_json::from_slice(&std::fs::read(out.join("summary.json")).unwrap()).unwrap();
    final_summary["lifecycle"] = lifecycle;
    final_summary["status"] = json!(if passed {
        "passed_mechanism_scope"
    } else {
        "failed"
    });
    final_summary["exit_code"] = json!(if passed { 0 } else { 1 });
    super::write(&out.join("summary.json"), &final_summary);
    if passed {
        0
    } else {
        1
    }
}

async fn terminate_owned(
    service: Option<Arc<RunningService<RoleClient, ()>>>,
    child: &mut tokio::process::Child,
) -> Value {
    let pid = child.id();
    let (cancel_ok, cancel_result) = match service {
        Some(service) => match Arc::try_unwrap(service) {
            Ok(s) => match tokio::time::timeout(Duration::from_secs(5), s.cancel()).await {
                Ok(Ok(reason)) => (true, format!("Ok({reason:?})")),
                Ok(Err(e)) => (false, format!("cancel error: {e}")),
                Err(_) => (false, "cancel timeout after 5s".into()),
            },
            Err(_) => (false, "unreleased service workers".into()),
        },
        None => (
            false,
            "service unavailable during initialization failure".into(),
        ),
    };
    let mut forced = false;
    let mut reaped = false;
    let mut code = None;
    let mut wait_error = None;
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(status)) => {
            reaped = true;
            code = status.code();
        }
        other => {
            forced = true;
            wait_error = Some(format!("graceful wait: {other:?}"));
            match tokio::time::timeout(Duration::from_secs(5), child.kill()).await {
                Ok(Ok(())) => {
                    reaped = true;
                    wait_error
                        .as_mut()
                        .unwrap()
                        .push_str("; owned Child.kill awaited exit/reap");
                }
                other => wait_error
                    .as_mut()
                    .unwrap()
                    .push_str(&format!("; kill/reap unproven: {other:?}")),
            }
        }
    }
    json!({"pid":pid,"cancel_result":cancel_result,"cancel_ok":cancel_ok,"forced":forced,"reaped":reaped,"exit_code":code,"wait_error":wait_error,
        "lifecycle_passed":cancel_ok&&reaped&&!forced&&code==Some(0),"scope":"owned tokio Child;bounded cancel5s/wait5s/kill5s;nonzero/forced/error never green"})
}

fn resource_summary(samples: &[Value], pid: Option<u32>) -> Value {
    let server: Vec<_> = samples
        .iter()
        .filter_map(|r| {
            r.get("server")
                .filter(|v| pid.is_some() && v["pid"].as_u64() == pid.map(u64::from))
        })
        .collect();
    let max_of = |field: &str| server.iter().filter_map(|v| v[field].as_u64()).max();
    let first = server.first();
    let last = server.last();
    let cpu_delta=first.zip(last).and_then(|(a,b)|{
        if a["pid"]!=b["pid"]||a["cpu_time_unit"]!=b["cpu_time_unit"]||a["kernel_release"]!=b["kernel_release"]||a["timebase_numer"]!=b["timebase_numer"]||a["timebase_denom"]!=b["timebase_denom"]{return None;}
        let user=b["cpu_user_ns"].as_u64()?.checked_sub(a["cpu_user_ns"].as_u64()?)?;
        let system=b["cpu_system_ns"].as_u64()?.checked_sub(a["cpu_system_ns"].as_u64()?)?;
        Some(json!({"user_ns":user,"system_ns":system,"pid":pid,"cpu_time_unit":a["cpu_time_unit"],"kernel_release":a["kernel_release"],"scope":"first-last sampled counters,not exact workload start/end"}))
    });
    json!({"sample_count":samples.len(),"server_available":server.len(),"server_unavailable":samples.len()-server.len(),"observed_max_server_resident_bytes":max_of("resident_bytes"),"observed_max_server_threads":max_of("threads"),"calibrated_cpu_delta":cpu_delta,"claim":"actual child process only;observed sampled maxima not tree or absolute peak;missing null not zero"})
}

/// Re-evaluate every read's locked-source facets/spans plus all timing arithmetic
/// and deterministic distributions. No product request or label rewrite occurs.
pub fn replay(plan_path: &Path, observed: &Path, output: &Path) -> i32 {
    assert!(!output.exists());
    let plan: MixedPlan = serde_json::from_slice(&std::fs::read(plan_path).unwrap()).unwrap();
    let root = tempfile::tempdir().unwrap();
    for (p, s) in &plan.files {
        cc_eval::benchmark::validation::relative_path(p).unwrap();
        let path = root.path().join(p);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, s).unwrap();
    }
    let rows: Vec<Value> =
        serde_json::from_slice(&std::fs::read(observed.join("observations.json")).unwrap())
            .unwrap();
    let recorded: Value =
        serde_json::from_slice(&std::fs::read(observed.join("summary.json")).unwrap()).unwrap();
    let mut mismatches = Vec::new();
    let mut read_times = Vec::new();
    let mut build_times = Vec::new();
    let mut failed = 0;
    for (i, r) in rows.iter().enumerate() {
        let check = if let Some(raw) = r.get("raw").filter(|v| !v.is_null()) {
            if r["kind"] == "read" {
                let q = plan.queries.iter().find(|q| q.id == r["query_id"]).unwrap();
                super::evaluate_facets(&serde_json::to_value(q).unwrap(), raw, root.path())
            } else {
                json!({"status":"build","passed":valid_build(raw,plan.files.len())})
            }
        } else {
            json!({"status":if r["error"].as_str().is_some_and(|e|e.starts_with("deadline_censored")){"Timeout"}else{"error"},"passed":false})
        };
        if check != r["check"] {
            mismatches
                .push(json!({"row":i,"field":"check","recorded":r["check"],"recomputed":check}));
        }
        if check["passed"] != true {
            failed += 1;
        }
        let (off, start, end) = (
            r["offered_us"].as_u64().unwrap(),
            r["started_us"].as_u64().unwrap(),
            r["finished_us"].as_u64().unwrap(),
        );
        if start < off
            || end < start
            || r["client_queue_us"] != start.saturating_sub(off)
            || r["service_and_transport_us"] != end.saturating_sub(start)
            || r["end_to_end_us"] != end.saturating_sub(off)
        {
            mismatches.push(json!({"row":i,"field":"clock_decomposition"}));
        }
        if r["kind"] == "read" {
            read_times.push(end.saturating_sub(off));
        } else {
            build_times.push(end.saturating_sub(off));
        }
    }
    for (field, expected) in [
        ("completed_jobs", json!(rows.len())),
        (
            "read_latency",
            serde_json::to_value(statistics::distribution(&read_times)).unwrap(),
        ),
        (
            "build_latency",
            serde_json::to_value(statistics::distribution(&build_times)).unwrap(),
        ),
    ] {
        if recorded[field] != expected {
            mismatches
                .push(json!({"field":field,"recorded":recorded[field],"recomputed":expected}));
        }
    }
    let termination: Value =
        serde_json::from_slice(&std::fs::read(observed.join("termination.json")).unwrap()).unwrap();
    let lifecycle_ok = termination["cancel_ok"] == true
        && termination["reaped"] == true
        && termination["forced"] == false
        && termination["exit_code"] == 0;
    if termination["lifecycle_passed"] != lifecycle_ok
        || recorded["lifecycle"] != termination
        || (recorded["exit_code"] == 0 && !lifecycle_ok)
    {
        mismatches.push(json!({"field":"lifecycle_and_final_summary","termination":termination,"summary":recorded}));
    }
    let resource_samples: Value =
        serde_json::from_slice(&std::fs::read(observed.join("resource-samples.json")).unwrap())
            .unwrap();
    let pid = resource_samples["server_pid"].as_u64().map(|p| p as u32);
    let recomputed_resources =
        resource_summary(resource_samples["samples"].as_array().unwrap(), pid);
    let recorded_resources: Value =
        serde_json::from_slice(&std::fs::read(observed.join("resource-summary.json")).unwrap())
            .unwrap();
    if recorded_resources != recomputed_resources {
        mismatches.push(json!({"field":"resource_summary","recorded":recorded_resources,"recomputed":recomputed_resources}));
    }
    super::write(
        output,
        &json!({"status":if mismatches.is_empty(){"replay_consistent"}else{"invalid_measurement"},"rows":rows.len(),"failed_read_or_build_checks":failed,"mismatches":mismatches,"limits":["recomputes raw body/span/facet/source and recorded timing/resource arithmetic","clock wall elapsed and native PID sample authenticity require immutable harness provenance","replay consistency does not turn red quality or small-N timing into acceptance"]}),
    );
    if mismatches.is_empty() {
        0
    } else {
        2
    }
}
