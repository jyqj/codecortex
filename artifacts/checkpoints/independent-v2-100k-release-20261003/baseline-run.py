#!/usr/bin/env python3
"""One finite synthetic 100k release run bound to PR101; preserve every failure."""
import concurrent.futures,gzip,hashlib,importlib.util,json,os,pathlib,shutil,subprocess,sys,threading,time,traceback
sys.dont_write_bytecode=True
OUT=pathlib.Path(__file__).resolve().parent
ROOT=OUT.parents[2]
SHA="574f7598662334c63e020da136c87f4f7281554d"
SEED=0
COUNT=100000
spec=importlib.util.spec_from_file_location("preparation",ROOT/"scripts/p7_release_resource_preparation.py")
driver=importlib.util.module_from_spec(spec);spec.loader.exec_module(driver)
write=driver.write
build=json.loads((OUT/"build-receipt.json").read_text())
assert build["source_sha"]==SHA and build["build_exit_code"]==0 and build["guard_stop"] is None
assert subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip()==SHA
assert sorted(build["compiler_artifact"]["features"])==["semantic","semantic-http"]
binary=pathlib.Path(build["compiler_artifact"]["executable"])
assert driver.digest(binary)==build["binary_sha256"]
assert build["compiler_artifact"]["profile"]["opt_level"]=="3" and not build["compiler_artifact"]["profile"]["debug_assertions"]
case=OUT/"live/n100000";case.mkdir(parents=True)
repo=case/"repo";(repo/"src").mkdir(parents=True)
cache=case/"cache"
subprocess.run(["git","-c","init.templateDir=","init","--quiet","--initial-branch=synthetic",str(repo)],check=True)
assert subprocess.check_output(["git","rev-parse","--show-toplevel"],cwd=repo,text=True).strip()==str(repo)
summary={"source_sha":SHA,"source_tree":build["source_tree"],"binary_sha256":build["binary_sha256"],"profile":"release","features":["semantic","semantic-http"],"build_receipt_sha256":driver.digest(OUT/"build-receipt.json"),"status":"running","full_V20":False,"valid_for_other_source_trees":False,"seed":SEED,"files":COUNT,"checks":[],"failures":[],"not_run":[],"timeouts":{"index_rpc":300,"ready_drain":300,"search_rpc":300,"normal_exit":15,"overall":900},"resource_limits":{"disk_reserve_bytes":4*1024**3,"cgroup_memory_guard_bytes":12*1024**3},"resource_sampling":{"interval_ms":20,"method":"proc stat/status root+thread children recursive discovery, cgroup current; non-atomic sampled RSS, not exact lifetime peak or PSS; unreadable tree coverage reported"}}
write(OUT/"preregistration.json",summary)
write(OUT/"environment.json",{"platform":list(os.uname()),"affinity":sorted(os.sched_getaffinity(0)),"cpu_max":pathlib.Path("/sys/fs/cgroup/cpu.max").read_text(),"memory_max":pathlib.Path("/sys/fs/cgroup/memory.max").read_text(),"memory_current":pathlib.Path("/sys/fs/cgroup/memory.current").read_text(),"memory_events":pathlib.Path("/sys/fs/cgroup/memory.events").read_text(),"disk_free_bytes":shutil.disk_usage(OUT).free,"source_generator":"scripts/p7_release_resource_preparation.py:source(i,value=0)","source_generator_sha256":driver.digest(ROOT/"scripts/p7_release_resource_preparation.py"),"rng":"none; seed=0 is generator value parameter, files i=0..99999","git_isolation":"empty synthetic repository; no inherited parent Git history"})
start=time.monotonic();product=None;model=None;stop=threading.Event();current_phase="preflight"
def persist():write(OUT/"summary.json",summary)
def check(name,value):
    summary["checks"].append({"name":name,"result":value});persist()
def guarded():
    while not stop.wait(.1):
        reason="overall_timeout_900s" if time.monotonic()-start>900 else "memory_above_12GiB" if int(pathlib.Path("/sys/fs/cgroup/memory.current").read_text())>12*1024**3 else "disk_below_4GiB" if shutil.disk_usage(OUT).free<4*1024**3 else None
        if reason:
            summary["guard_stop"]=reason;summary["failures"].append({"phase":current_phase,"reason":reason});persist()
            if product and product.p.poll() is None:product.p.terminate()
            if model and model.poll() is None:model.terminate()
            return
# Replace only measurement hooks, retain real MCP product protocol and source generator.
def tree_snapshot(pid):
    todo=[pid];seen=set();rows=[];errors=[]
    while todo:
        current=todo.pop()
        if current in seen:continue
        seen.add(current);row=driver.snapshot(current)
        if row:rows.append(row)
        else:errors.append({"pid":current,"reason":"stat/status/io unreadable or process exited"})
        try:
            tasks=list(pathlib.Path(f"/proc/{current}/task").iterdir())
            for task in tasks:
                try:todo.extend(int(x) for x in (task/"children").read_text().split())
                except OSError as exc:errors.append({"pid":current,"task":task.name,"reason":str(exc)})
        except OSError as exc:errors.append({"pid":current,"reason":str(exc)})
    return {"root_pid":pid,"observed_pids":sorted(seen),"snapshots":rows,"sampled_sum_rss_bytes":sum(x["rss_bytes"] for x in rows) if rows else None,"coverage_errors":errors,"method":"recursive every-thread children; non-atomic, transient descendants may be missed; shared pages counted per process"}
driver.tree_snapshot=tree_snapshot
def normal_close(p):
    p.phase="normal-eof-exit";t=time.monotonic();p.p.stdin.close()
    try:code=p.p.wait(timeout=15);forced=False
    except subprocess.TimeoutExpired:
        forced=True;p.p.terminate()
        try:code=p.p.wait(timeout=15)
        except subprocess.TimeoutExpired:p.p.kill();code=p.p.wait()
    p.stop.set();p.sampler.join();p.raw.close();p.resources.close();p.stderr.close()
    result={"exit_code":code,"forced":forced,"wall_seconds":time.monotonic()-t}
    assert code==0 and not forced,result
    return result
completed=set()
try:
    assert shutil.disk_usage(OUT).free>=4*1024**3
    assert int(pathlib.Path("/sys/fs/cgroup/memory.current").read_text())<12*1024**3
    threading.Thread(target=guarded,daemon=True).start()
    current_phase="generation";t=time.monotonic();total=0;aggregate=hashlib.sha256()
    with gzip.open(case/"source-inputs.jsonl.gz","wt") as manifest:
        for i in range(COUNT):
            path=repo/"src"/f"file_{i:05}.rs";data=driver.source(i,value=SEED).encode();path.write_bytes(data);total+=len(data)
            row={"path":str(path.relative_to(repo)),"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
            line=json.dumps(row,sort_keys=True)+"\n";manifest.write(line);aggregate.update(line.encode())
    assert sum(1 for _ in (repo/"src").iterdir())==COUNT
    check("generation",{"files":COUNT,"logical_bytes":total,"wall_seconds":time.monotonic()-t,"ordered_manifest_sha256":aggregate.hexdigest()});completed.add("generation")
    model_log=(case/"model.log").open("wb")
    model=subprocess.Popen([sys.executable,str(ROOT/"scripts/p7_release_resource_preparation.py"),"--mock",str(case)],stdout=model_log,stderr=model_log)
    driver.wait_file(case/"http-port.json")
    port=json.loads((case/"http-port.json").read_text())["port"]
    config={"auto_index":{"enabled":False},"indexing":{"max_concurrent_parse":4},"query":{"strategy":"local","deadline_ms":30000,"lane_timeout_ms":20000,"semantic_timeout_ms":5000,"semantic_top_k":24},"semantic":{"enabled":True,"network_opt_in":True,"allow_query_network":False,"model_id":"fake/resource-preparation","dimensions":128,"max_input_tokens":8192,"max_batch_items":16,"endpoint":f"http://127.0.0.1:{port}/v1","allow_http":True,"api_key_ref":"env:P7_RESOURCE_DUMMY","max_concurrent":4,"max_concurrent_per_project":2,"retry_max_attempts":1,"retry_total_deadline_ms":120000,"breaker_failure_threshold":100}}
    write(repo/".codecortex.json",config);write(case/"config.json",config)
    def open_product(directory):
        directory.mkdir(exist_ok=True)
        p=driver.Product(binary.resolve(),repo,directory,cache);p.model_pid=model.pid;return p
    assert not list(repo.rglob("index.sqlite3")),"cold requires no index database"
    product=open_product(case);current_phase=product.phase="cold-empty-index-build";t=time.monotonic()
    result=product.tool("index",{"path":str(repo)})
    cold={"wall_seconds":time.monotonic()-t,"rpc_timing":dict(product.last_timing),"result":result}
    write(case/"cold-result.json",cold)
    assert result["files_scanned"]==result["files_parsed"]==result["files_added"]==COUNT
    assert result["files_skipped"]==0 and not result["parse_errors"]
    check("cold_index",cold);completed.add("cold_index")
    driver.wait_file(case/"http-entered")
    held=driver.db_report(repo);write(case/"held-db.json",held)
    assert held["counts"]["files"]==COUNT and held["integrity"]=="ok" and held["foreign_key_errors"]==0
    assert held["counts"]["edge_tables"]["co_change_edges"]==0 and held["counts"]["edge_tables"]["test_edges"]==0,held
    (case/"http-release").touch();current_phase=product.phase="backfill-drain";t=time.monotonic();deadline=t+300
    while True:
        status=product.tool("status",{"aspect":"capabilities"})["retrieval"]
        if status["semantic_state"]=="ready":break
        if time.monotonic()>=deadline:raise TimeoutError("backfill did not become ready in 300s")
        time.sleep(.2)
    ready_wall=time.monotonic()-t
    final=driver.db_report(repo);write(case/"final-db.json",final);write(case/"final-status.json",status)
    assert final["counts"]["files"]==final["counts"]["document_manifest"]==final["counts"]["semantic_manifest"]==COUNT
    assert final["integrity"]=="ok" and final["foreign_key_errors"]==0
    check("ready_manifest_integrity_fk",{"drain_wall_seconds":ready_wall,"db":final,"status":status});completed.add("ready_manifest_integrity_fk")
    queries=["resource_00000","resource_00001","resource_50000","resource_99999"]
    current_phase=product.phase="serial-reference"
    def query(p,q):
        t=time.monotonic();value=p.tool("search",{"query":q,"retrieval_strategy":"local","top_k":5});hits=value["machine_pack"]["hits"]
        assert hits,(q,value)
        assert q in json.dumps(hits),(q,hits)
        return {"query":q,"wall_seconds":time.monotonic()-t,"hits":hits,"response":value}
    reference={q:query(product,q) for q in queries};write(case/"serial-reference.json",reference)
    for mode,offered in [("repeated",[queries[0]]*4),("distinct",queries)]:
        current_phase=product.phase="concurrent-"+mode;product.peak=0;t=time.monotonic()
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:rows=list(pool.map(lambda q:query(product,q),offered))
        write(case/f"concurrent-{mode}.json",rows)
        for row in rows:assert row["hits"]==reference[row["query"]]["hits"],"query contamination or unstable hits"
        check("concurrent_"+mode,{"requests":4,"observed_peak_inflight":product.peak,"burst_wall_seconds":time.monotonic()-t,"stable_hits":True})
    completed.add("bounded_concurrency")
    exit_result=normal_close(product);product=None;check("normal_exit",exit_result);completed.add("normal_exit")
    current_phase="reopen";product=open_product(case/"reopen");product.phase="reopen-status-and-query"
    reopened_status=product.tool("status",{"aspect":"capabilities"})["retrieval"];write(case/"reopen-status.json",reopened_status)
    assert reopened_status["semantic_state"]=="ready",reopened_status
    reopened=driver.db_report(repo);write(case/"reopen-db.json",reopened)
    assert reopened["counts"]==final["counts"] and reopened["integrity"]=="ok" and reopened["foreign_key_errors"]==0
    rows=[query(product,q) for q in queries];write(case/"reopen-queries.json",rows)
    for row in rows:assert row["hits"]==reference[row["query"]]["hits"],"reopen changed hits"
    check("reopen_stability",{"stable_counts":True,"stable_hits":True,"status":reopened_status});completed.add("reopen_stability")
    exit_result=normal_close(product);product=None;check("reopen_normal_exit",exit_result)
    summary["status"]="passed_declared_100k_local_scope"
except BaseException as exc:
    summary["status"]="failed";summary["failures"].append({"phase":current_phase,"type":type(exc).__name__,"error":str(exc),"traceback":traceback.format_exc()})
    (OUT/"run-failure.log").write_text(traceback.format_exc());print(traceback.format_exc(),file=sys.stderr)
finally:
    stop.set();(case/"http-release").touch()
    if product:
        try:summary["failure_cleanup_product_exit"]=product.close()
        except BaseException as exc:summary["failures"].append({"phase":"cleanup","error":repr(exc)})
    if model:
        if model.poll() is None:model.terminate()
        try:model.wait(timeout=10)
        except subprocess.TimeoutExpired:model.kill();model.wait()
        summary["mock_shutdown_exit_code"]=model.returncode;model_log.close()
    summary["not_run"]=[name for name in ["generation","cold_index","ready_manifest_integrity_fk","bounded_concurrency","normal_exit","reopen_stability"] if name not in completed]
    summary["intentionally_not_run"]=["50k repeat","heldout","real providers","full V20","other source trees","large unbounded pressure","workspace test/clippy/format checks (evidence-only change; could include heldout, no production changes)"]
    summary["wall_seconds"]=time.monotonic()-start;summary["memory_events_after"]=pathlib.Path("/sys/fs/cgroup/memory.events").read_text();persist()
    for path in case.rglob("*.jsonl"):
        with path.open("rb") as src,gzip.open(str(path)+".gz","wb") as dst:shutil.copyfileobj(src,dst)
        path.unlink()
print(json.dumps({"status":summary["status"],"wall_seconds":summary["wall_seconds"],"failures":summary["failures"],"not_run":summary["not_run"]},ensure_ascii=False))
raise SystemExit(0 if summary["status"]=="passed_declared_100k_local_scope" else 1)
