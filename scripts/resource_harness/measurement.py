"""Explicit single-variant measurement. Imports perform no builds or product starts."""
import concurrent.futures,gzip,hashlib,json,os,pathlib,shutil,sqlite3,subprocess,sys,threading,time,traceback
from . import protocol as driver
from .product import Product
from .runtime import Journal, Observer, memory_snapshot
from .identity import driver_identity
write=driver.write


class Gate:
    def __init__(self,summary,out,observer):
        self.summary,self.out,self.observer=summary,out,observer
        self.lock=threading.RLock()

    def check(self,product=None):
        with self.lock:
            if self.summary.get('guard_stop'):
                raise RuntimeError(self.summary['guard_stop'])
            if product:product.transport.pending.raise_if_terminal()

    def fail(self,reason,product,model,**details):
        with self.lock:
            self.summary['guard_stop']=reason
            self.summary['status']='failed'
            self.summary['failures'].append(dict(phase=self.observer.phase,reason=reason))
            try:
                write(self.out/'summary.json',self.summary)
            finally:
                # Even a failed persistence attempt must wake unresolved RPCs.
                if product:product.transport.notify_guard(reason,group_path=str(self.observer.group),**details)
        self.observer.sample()
        if product and product.p.poll() is None:product.p.terminate()
        if model and model.poll() is None:model.terminate()


def measure(ROOT,OUT,SHA,receipt,receipt_digest,group,scope,*,preflight=False):
    COUNT=32 if preflight else 100000
    SEED=0
    build=receipt
    binary=pathlib.Path(build["compiler_artifact"]["executable"])
    assert build["source_sha"]==SHA and build["build_exit_code"]==0 and build["guard_stop"] is None
    assert sorted(build["compiler_artifact"]["features"])==["semantic","semantic-http"]
    binary=pathlib.Path(build["compiler_artifact"]["executable"])
    assert driver.digest(binary)==build["binary_sha256"]
    assert build["compiler_artifact"]["profile"]["opt_level"]=="3" and not build["compiler_artifact"]["profile"]["debug_assertions"]
    OUT.mkdir()
    case=OUT/f"live/n{COUNT}";case.mkdir(parents=True)
    repo=case/"repo";(repo/"src").mkdir(parents=True)
    cache=case/"cache"
    subprocess.run(["git","-c","init.templateDir=","init","--quiet","--initial-branch=synthetic",str(repo)],check=True)
    assert subprocess.check_output(["git","rev-parse","--show-toplevel"],cwd=repo,text=True).strip()==str(repo)
    summary={"production_sha":SHA,"source_sha":SHA,"source_tree":build["source_tree"],"binary_sha256":build["binary_sha256"],"profile":"release","features":["semantic","semantic-http"],"build_receipt_sha256":receipt_digest,"status":"running","full_V20":False,"valid_for_other_source_trees":False,"seed":SEED,"files":COUNT,"checks":[],"failures":[],"not_run":[],"timeouts":{"index_rpc":300,"ready_drain":300,"search_rpc":300,"normal_exit":15,"overall":900},"resource_limits":{"disk_reserve_bytes":4*1024**3,"cgroup_memory_guard_bytes":12*1024**3},"resource_sampling":{"interval_ms":20,"method":"proc stat/status root+thread children recursive discovery, cgroup current; non-atomic sampled RSS, not exact lifetime peak or PSS; unreadable tree coverage reported"}}
    write(OUT/"preregistration.json",summary)
    write(OUT/"environment.json",{"platform":list(os.uname()),"affinity":sorted(os.sched_getaffinity(0)),"cpu_max":(group / "cpu.max").read_text(),"memory_max":(group / "memory.max").read_text(),"memory_current":(group / "memory.current").read_text(),"memory_events":(group / "memory.events").read_text(),"disk_free_bytes":shutil.disk_usage(OUT).free,"source_generator":"resource_harness.protocol:source(i,value=0)","source_generator_module_sha256":driver.digest(pathlib.Path(driver.__file__)),"original_preparation_sha256":"a76865a466252f5863c8e0ad278c4c6814093d4a4a629a9409acf2e6b916901b","rng":f"none; seed=0 is generator value parameter, files i=0..{COUNT-1}","git_isolation":"empty synthetic repository; no inherited parent Git history"})
    start=time.monotonic();product=None;model=None;stop=threading.Event();current_phase="preflight"
    drain_start=None;boundary_thread=None
    obstream=(OUT/"composition.jsonl").open("w")
    observer=Observer(Journal(obstream),group,scope=scope,processes={"harness_root":os.getpid()})
    gate=Gate(summary,OUT,observer)
    def persist():
        with gate.lock: write(OUT/"summary.json",summary)
    def check(name,value):
        summary["checks"].append({"name":name,"result":value});persist()
    def guarded():
        while not stop.wait(.1):
            try:
                current=int((group / 'memory.current').read_text())
                reason='overall_timeout_900s' if time.monotonic()-start>900 else 'memory_above_12GiB' if current>12*1024**3 else 'disk_below_4GiB' if shutil.disk_usage(OUT).free<4*1024**3 else None
            except (OSError,ValueError) as exc:
                reason='guard_observation_error: '+repr(exc);current=None
            if reason:
                gate.fail(reason,product,model,observed_bytes=current)
                return
    def v2_status(p):
        value=p.tool("status",{"aspect":"capabilities"});status=value["retrieval"]
        expected={"spec":"retrieval-capabilities-v2","consistency":"point_in_time","generation_scope":"observed_database_snapshot","service_state_scope":"process_observed_separately"}
        assert all(status.get(k)==v for k,v in expected.items()),status
        assert status.get("identity_validation") in ("checked_at_observation_boundary","not_observed"),status
        if status.get("semantic_state")=="ready":
            assert status["identity_validation"]=="checked_at_observation_boundary" and status.get("generation") and status.get("semantic_active_space"),status
        summary.setdefault("status_observations",[]).append({"time_ns":time.monotonic_ns(),"elapsed_drain_seconds":time.monotonic()-drain_start if drain_start is not None else None,"semantic_state":status.get("semantic_state"),"index_state":status.get("index_state"),"error":status.get("error"),"rpc_timing":dict(p.last_timing)})
        write(case/"last-observed-status.json",value)
        return status

    def deadline_counts():
        observed={"label":"300s boundary read-only observation; actual time recorded; never cleanup counts","read_start_ns":time.monotonic_ns(),"elapsed_drain_seconds":time.monotonic()-drain_start}
        try:
            paths=[pathlib.Path(held["path"])]
            c=sqlite3.connect(f"file:{paths[0]}?mode=ro",uri=True,timeout=0)
            try:
                c.execute("BEGIN")
                observed["snapshot_query_start_ns"]=time.monotonic_ns();observed["snapshot_elapsed_drain_seconds"]=time.monotonic()-drain_start
                observed["counts"]={t:c.execute(f"SELECT COUNT(*) FROM {t}").fetchone()[0] for t in ["files","symbols","chunks","document_manifest","semantic_manifest"]}
                observed["outbox_states"]={k:v for k,v in c.execute("SELECT state,COUNT(*) FROM semantic_outbox GROUP BY state")}
            finally:c.close()
        except BaseException as exc:observed["error"]=repr(exc)
        observed["read_end_ns"]=time.monotonic_ns();write(case/"deadline-300s-db.json",observed)
        summary["deadline_300s_db"]=observed;persist()

    def boundary_observer():
        if not boundary_stop.wait(max(0,deadline-time.monotonic())):deadline_counts()

    def normal_close(p, cleanup=False):
        if not cleanup:gate.check(p)
        p.phase="normal-eof-exit";t=time.monotonic();p.p.stdin.close()
        try:code=p.p.wait(timeout=15);forced=False
        except subprocess.TimeoutExpired:
            forced=True;p.p.terminate()
            try:code=p.p.wait(timeout=15)
            except subprocess.TimeoutExpired:raise RuntimeError("normal EOF and controlled termination did not complete; no kill experiment")
        p.close_logs()
        result={"exit_code":code,"forced":forced,"wall_seconds":time.monotonic()-t}
        summary.setdefault("product_exits",[]).append(result);persist()
        assert code==0 and not forced,result
        return result
    completed=set()
    try:
        assert shutil.disk_usage(OUT).free>=4*1024**3
        assert int((group / "memory.current").read_text())<12*1024**3
        guard_thread=threading.Thread(target=guarded,name="measurement-guard");guard_thread.start()
        current_phase="generation";observer.begin_phase("generation-before");t=time.monotonic();total=0;aggregate=hashlib.sha256()
        with gzip.open(case/"source-inputs.jsonl.gz","wt") as manifest:
            for i in range(COUNT):
                path=repo/"src"/f"file_{i:05}.rs";data=driver.source(i,value=SEED).encode();path.write_bytes(data);total+=len(data)
                row={"path":str(path.relative_to(repo)),"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
                line=json.dumps(row,sort_keys=True)+"\n";manifest.write(line);aggregate.update(line.encode())
        assert sum(1 for _ in (repo/"src").iterdir())==COUNT
        check("generation",{"files":COUNT,"logical_bytes":total,"wall_seconds":time.monotonic()-t,"ordered_manifest_sha256":aggregate.hexdigest()});completed.add("generation")
        observer.begin_phase("generation-after")
        gate.check()
        model_log=(case/"model.log").open("wb")
        model=subprocess.Popen([sys.executable,str(pathlib.Path(driver.__file__).resolve()),"--mock",str(case)],stdout=model_log,stderr=model_log)
        driver.wait_file(case/"http-port.json")
        port=json.loads((case/"http-port.json").read_text())["port"]
        config={"auto_index":{"enabled":False},"indexing":{"max_concurrent_parse":4},"query":{"strategy":"local","deadline_ms":30000,"lane_timeout_ms":20000,"semantic_timeout_ms":5000,"semantic_top_k":24},"semantic":{"enabled":True,"network_opt_in":True,"allow_query_network":False,"model_id":"fake/resource-preparation","dimensions":128,"max_input_tokens":8192,"max_batch_items":16,"endpoint":f"http://127.0.0.1:{port}/v1","allow_http":True,"api_key_ref":"env:P7_RESOURCE_DUMMY","max_concurrent":4,"max_concurrent_per_project":2,"retry_max_attempts":1,"retry_total_deadline_ms":120000,"breaker_failure_threshold":100}}
        write(repo/".codecortex.json",config);write(case/"config.json",config)
        summary["config_sha256"]=driver.digest(case/"config.json");persist()
        def open_product(directory):
            nonlocal product
            directory.mkdir(exist_ok=True)
            with gate.lock:
                gate.check()
                product=Product(binary.resolve(),repo,directory,cache,observer,model.pid)
            product.initialize(directory)
            gate.check(product)
            return product
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
        if preflight:
            hold_started=time.monotonic_ns()
            time.sleep(.5)
            held_events=[json.loads(line) for line in (case/'http.jsonl').read_text().splitlines()]
            enters=[e for e in held_events if e['event']=='entered']
            returns=[e for e in held_events if e['event']=='returned']
            held_status=v2_status(product)
            observation=dict(time_ns=time.monotonic_ns(),hold_started_ns=hold_started,
                             actual_http_held=len(enters),actual_returned=len(returns),
                             actual_batch_sizes=[e['input_count'] for e in enters],
                             configured_caps=dict(global_cap=4,per_project=2),held_status=held_status,
                             runtime_effective_caps='unknown: status does not expose semantic gate caps',
                             independent_acceptance=False)
            write(OUT/'sharedgate-preflight.json',observation)
            assert len(enters)==2 and not returns,observation
            assert held_status['semantic_state']=='backfilling',held_status
            check('controlled_http_hold',observation)
        current_phase=product.phase="backfill-drain";t=time.monotonic();drain_start=t;deadline=t+300
        (case/"http-release").touch()
        summary["drain_start_ns"]=time.monotonic_ns();summary["ready_deadline_monotonic"]=deadline;persist()
        boundary_stop=threading.Event();boundary_thread=threading.Thread(target=boundary_observer,daemon=True);boundary_thread.start()
        while True:
            if time.monotonic()>=deadline:raise TimeoutError("backfill did not become ready in 300s")
            status=v2_status(product)
            observed_at=time.monotonic()
            if observed_at>=deadline:raise TimeoutError("backfill did not become ready in 300s (late responses are not accepted)")
            gate.check(product)
            if status["semantic_state"]=="ready":break
            time.sleep(.2)
        ready_wall=time.monotonic()-t
        boundary_stop.set();boundary_thread.join()
        final=driver.db_report(repo);write(case/"final-db.json",final);write(case/"final-status.json",status)
        assert final["counts"]["files"]==final["counts"]["document_manifest"]==final["counts"]["semantic_manifest"]==COUNT
        assert final["integrity"]=="ok" and final["foreign_key_errors"]==0
        check("ready_manifest_integrity_fk",{"drain_wall_seconds":ready_wall,"db":final,"status":status});completed.add("ready_manifest_integrity_fk")
        queries=[f"resource_{i:05}" for i in (0,1,COUNT//2,COUNT-1)]
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
        reopened_status=v2_status(product);write(case/"reopen-status.json",reopened_status)
        gate.check(product)
        assert reopened_status["semantic_state"]=="ready",reopened_status
        reopened=driver.db_report(repo);write(case/"reopen-db.json",reopened)
        assert reopened["counts"]==final["counts"] and reopened["integrity"]=="ok" and reopened["foreign_key_errors"]==0
        rows=[query(product,q) for q in queries];write(case/"reopen-queries.json",rows)
        for row in rows:assert row["hits"]==reference[row["query"]]["hits"],"reopen changed hits"
        check("reopen_stability",{"stable_counts":True,"stable_hits":True,"status":reopened_status});completed.add("reopen_stability")
        exit_result=normal_close(product);product=None;check("reopen_normal_exit",exit_result)
        with gate.lock:
            gate.check()
            summary["status"]="passed_preflight_32" if preflight else "passed_declared_100k_local_scope"
    except BaseException as exc:
        summary["status"]="failed";summary["failures"].append({"phase":current_phase,"type":type(exc).__name__,"error":str(exc),"traceback":traceback.format_exc()})
        (OUT/"run-failure.log").write_text(traceback.format_exc());print(traceback.format_exc(),file=sys.stderr)
    finally:
        if boundary_thread is not None:
            if time.monotonic()<deadline:boundary_stop.set()
            boundary_thread.join()
        summary["ready_window_end_elapsed_seconds"]=time.monotonic()-drain_start if drain_start is not None else None
        stop.set()
        if "guard_thread" in locals():guard_thread.join()
        observer.begin_phase("cleanup")
        (case/"http-release").touch()
        if product:
            try:summary["failure_cleanup_product_exit"]=normal_close(product,cleanup=True)
            except BaseException as exc:summary["failures"].append({"phase":"cleanup","error":repr(exc)})
        if model:
            if model.poll() is None:model.terminate()
            try:model.wait(timeout=10)
            except subprocess.TimeoutExpired:summary["failures"].append({"phase":"mock_cleanup","error":"controlled terminate did not complete; no kill"})
            summary["mock_shutdown_exit_code"]=model.returncode;model_log.close()
        if drain_start is not None:
            tail=driver.db_report(repo);tail["inspection_phase"]="after cleanup; not within 300s ready deadline";tail["elapsed_drain_seconds"]=time.monotonic()-drain_start
            write(case/"cleanup-tail-db.json",tail);summary["cleanup_tail_db"]=tail
        summary["not_run"]=[name for name in ["generation","cold_index","ready_manifest_integrity_fk","bounded_concurrency","normal_exit","reopen_stability"] if name not in completed]
        summary["intentionally_not_run"]=["50k repeat","heldout","real providers","full V20","other source trees","large unbounded pressure","workspace test/clippy/format checks (evidence-only change; could include heldout, no production changes)"]
        summary["wall_seconds"]=time.monotonic()-start
        summary["memory_events_after"]=memory_snapshot(group,scope=scope)['memory_events']
        observer.begin_phase('cleanup-after')
        if summary['failures'] or summary.get('guard_stop'):summary['status']='failed'
        persist()
        write(OUT/'identity.json',dict(source_sha=SHA,source_tree=build['source_tree'],
              binary_sha256=build['binary_sha256'],build_receipt_sha256=receipt_digest,
              config_sha256=summary.get('config_sha256'),driver_files=driver_identity(),
              strict_paired_causal_speedup=False,group_independence='unknown',scope=scope))
        obstream.close()
        for path in case.rglob("*.jsonl"):
            with path.open("rb") as src,gzip.open(str(path)+".gz","wb") as dst:shutil.copyfileobj(src,dst)
            path.unlink()

    return summary
