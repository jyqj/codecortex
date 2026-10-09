import ast, base64, datetime, hashlib, json, pathlib, re, stat, subprocess, sys, zipfile
if sys.flags.optimize:
    raise RuntimeError("optimized Python cannot perform receipt checks")
data=sys.stdin.buffer.read()
def unique(pairs):
    result={}
    for key,value in pairs:
        if key in result: raise ValueError("duplicate JSON key: "+key)
        result[key]=value
    return result
def parse(raw):
    return json.loads(raw,object_pairs_hook=unique,parse_constant=lambda value:(_ for _ in ()).throw(ValueError("nonfinite JSON: "+value)))
def sha(raw): return hashlib.sha256(raw).hexdigest()
p=parse(data)
out=pathlib.Path("artifacts/checkpoints/p8-round17-evidence-intake-20261009/surface-window-controls-root")
out.mkdir(exist_ok=False)
(out/"audit-script.py").write_bytes(pathlib.Path(__file__).read_bytes())
(out/"audit-input.json").write_bytes(data)
checks=[]
def require(ok,name):
    if not ok: raise ValueError(name)
    checks.append(name)
report={"schema":"independent-original-surface-window-controls-reception-v1","reviewer":"/root","observed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"status":"failed","scope":"Independent complete original artifact and recorded execution review only; no Cargo, product, benchmark or original test rerun."}
try:
    response=subprocess.run(["gh","api","repos/jyqj/codecortex/git/blobs/"+p["approved_registry"]["blob"]],capture_output=True,check=True,timeout=40)
    registry_api=parse(response.stdout)
    require(registry_api["encoding"]=="base64" and registry_api["sha"]==p["approved_registry"]["blob"],"fixed registry original Git object identity")
    registry_bytes=base64.b64decode(registry_api["content"],validate=False)
    require(len(registry_bytes)==registry_api["size"] and sha(registry_bytes)==p["approved_registry"]["sha256"],"complete original approved registry SHA256 and size")
    require(hashlib.sha1(b"blob "+str(len(registry_bytes)).encode()+b"\0"+registry_bytes).hexdigest()==p["approved_registry"]["blob"],"approved registry raw Git blob SHA1")
    expected_registry=parse(registry_bytes)
    (out/"approved-registry.json").write_bytes(registry_bytes)
    archive=pathlib.Path(p["archive"]["path"])
    require(archive.is_file() and not archive.is_symlink(),"original regular ZIP")
    before=archive.stat()
    raw=archive.read_bytes()
    require(len(raw)==p["archive"]["bytes"] and sha(raw)==p["archive"]["sha256"],"complete original ZIP byte size and SHA256")
    api=p["APIs"]
    run=api["run"]; jobs=api["jobs"]; arts=api["artifacts"]
    require(run["id"]==37890734575 and run["head_sha"]==p["source"] and run["run_attempt"]==1 and run["event"]=="push" and run["status"]=="completed" and run["conclusion"]=="success","original first run terminal identity")
    require(jobs["total_count"]==len(jobs["jobs"])==1 and jobs["jobs"][0]["id"]==113690845958 and jobs["jobs"][0]["conclusion"]=="success" and all(s["status"]=="completed" and s["conclusion"]=="success" for s in jobs["jobs"][0]["steps"]),"complete sole job and steps success")
    require(arts["total_count"]==len(arts["artifacts"])==1,"complete unique original artifact API page")
    art=arts["artifacts"][0]
    require(art["id"]==11597533828 and art["name"]=="p8-surface-window-controls-37890734575" and art["size_in_bytes"]==len(raw) and art["digest"]=="sha256:"+sha(raw) and art["expired"] is False and art["workflow_run"]["head_sha"]==p["source"],"API artifact bytes bind full received ZIP")
    contents={}; inv=[]
    with zipfile.ZipFile(archive) as z:
        require(z.testzip() is None,"complete original ZIP CRC population")
        names=z.namelist()
        require(len(names)==len(set(names))==21,"exact 21 unique original members")
        for info in z.infolist():
            name=info.filename
            require(not info.is_dir() and not name.startswith("/") and ".." not in pathlib.PurePosixPath(name).parts and not (info.flag_bits&1) and not stat.S_ISLNK(info.external_attr>>16),"safe regular member "+name)
            b=z.read(info)
            require(len(b)==info.file_size,"original member full size "+name)
            contents[name]=b
            inv.append({"path":name,"bytes":len(b),"sha256":sha(b),"crc32":info.CRC})
    require(sum(len(v) for v in contents.values())==370401,"complete expanded original bytes")
    expected_names={f"{i:02}.{stream}" for i in range(6) for stream in ("stdout","stderr")}|{"plan.json","progress.json","receipt.json","source-before.json","source-after.json","observer-before.json","observer-after.json","observations/p8-prepare-surface-window-work.json","observations/p8-prepare-dependency-window-work.json"}
    require(set(contents)==expected_names,"complete original member names")
    receipt=parse(contents["receipt.json"]); plan=parse(contents["plan.json"]); progress=parse(contents["progress.json"])
    inventory={n:{"bytes":len(b),"sha256":sha(b)} for n,b in sorted(contents.items()) if n!="receipt.json"}
    require(receipt["files"]==inventory,"receipt exactly seals all 20 other original members")
    workflow=p["workflow"]["body"]
    require(len(workflow.encode())==p["workflow"]["bytes"] and sha(workflow.encode())==p["workflow"]["sha256"]=="f27250c1c66238aa2b60b7ff53c013611198e8653227851c76311f2efa56ed0a","fixed committed workflow bytes")
    def literal(name):
        match=re.search(r"^\s+"+name+r" = (.+)$",workflow,re.M)
        require(match is not None,"fixed workflow literal "+name)
        return ast.literal_eval(match.group(1))
    commands=literal("commands")
    tests={str(k):v for k,v in literal("expected_tests").items()}
    observers=literal("expected_observers")
    require(len(commands)==6 and set(tests)=={"3","4","5"} and [len(tests[str(k)]) for k in (3,4,5)]==[6,3,1],"original six commands and exact registered 6+3+1 population")
    for record,label in ((plan,"plan"),(progress,"progress"),(receipt,"receipt")):
        require(record["commands"]==commands and record["expected_tests"]==tests and record["expected_test_count"]==10,label+" exact registered argv and test identities")
        require(record["expected_source"]==p["source"] and record["expected_product_input_count"]==1089 and record["expected_product_manifest_sha256"]==p["product_manifest"],label+" exact source and input identity")
        require(record["cwd"]=="/home/runner/work/codecortex/codecortex",label+" original command cwd")
    require(plan["status"]=="running" and plan["results"]==[],"original pre-execution plan retained")
    require(progress["status"]=="running" and progress["results"]==receipt["results"],"last original progress retains complete original command rows")
    env={"CARGO_BUILD_JOBS":"2","CARGO_INCREMENTAL":"0","CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","RUSTFLAGS":"","RUSTC_WRAPPER":"","RUSTC_WORKSPACE_WRAPPER":"","CARGO_TARGET_DIR":"/home/runner/work/_temp/surface-window-controls-cargo","CODECORTEX_BENCH_OBSERVATIONS":"/home/runner/work/_temp/surface-window-controls/observations"}
    require(plan["environment"]==progress["environment"]==receipt["environment"]==env,"original isolated target and unwrapped environment")
    require(receipt["schema_version"]==2 and receipt["status"]=="controls_completed" and receipt["exit_code"]==0 and receipt["not_run_command_indices"]==[] and receipt["source_unchanged"] is True and receipt["observers_unchanged"] is True and len(receipt["results"])==6,"complete successful recorded execution boundary")
    require(contents["source-before.json"]==contents["source-after.json"],"complete source before/after bytes equal")
    source=parse(contents["source-before.json"])
    require(source["source_commit"]==p["source"] and source["source_tree"]==p["tree"] and source["input_count"]==len(source["inputs"])==1089,"actual fixed Git source and tree identity")
    require(source["inputs"]==expected_registry["complete_inputs"],"all 1089 source inputs exactly match independently approved actual registry")
    manifest=(json.dumps(source["inputs"],ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
    require(sha(manifest)==source["manifest_sha256"]==p["product_manifest"],"independently recomputed full product manifest")
    require(contents["observer-before.json"]==contents["observer-after.json"],"complete observer before/after bytes equal")
    actual_observers=parse(contents["observer-before.json"])
    require(set(actual_observers)==set(observers) and {k:v["sha256"] for k,v in actual_observers.items()}==observers,"all three original observer identities")
    require({k:v["bytes"] for k,v in actual_observers.items()}=={"scripts/p7_build_identity.py":5940,"scripts/p8_scale_matrix.py":65092,"scripts/p8_runner_capacity.py":11164},"all original observer byte lengths")
    guard=parse(contents["00.stdout"])
    require(guard=={"schema_version":10,"source_version":"p8-completion-source-20261009-v15","previous_source_version":"p8-oracle-compat-source-20261008-v14","base_source":"7354db236c9d9850a75f31672697ae9eab44565e","product_source":"a402e88d460179afb12e3e2dc066d62cf168c690","review_source":"f66daf6088f6f6a8d990d736ee0c1213032ca175","scope":"independently_reviewed_source_and_validation_inputs","runtime_and_quality_claims":"require_separate_execution_evidence","status":"passed","complete_inputs":1089,"executed_previous_proof":"p8-oracle-compat-source-20261008-v14"},"complete original v15 stdout includes actual P/R and executed previous proof")
    populations=[]
    for index,row in enumerate(receipt["results"]):
        require(row["argv"]==commands[index] and row["stdout"]==f"{index:02}.stdout" and row["stderr"]==f"{index:02}.stderr" and row["exit_code"]==0 and row["accepted"] is True,"original accepted command identity "+str(index))
        require(type(row["elapsed_ns"]) is int and row["elapsed_ns"]>0 and datetime.datetime.fromisoformat(row["started_utc"]).utcoffset()==datetime.timedelta(),"positive original elapsed and UTC start "+str(index))
        if str(index) in tests:
            expected=tests[str(index)]
            text=contents[row["stdout"]].decode()
            require(re.findall(r"^running (\d+) tests?\s*$",text,re.M)==[str(len(expected))],"single correct libtest running count "+str(index))
            method_lines=[line for line in text.splitlines() if line.startswith("test ") and not line.startswith("test result:")]
            found=[]
            for line in method_lines:
                match=re.fullmatch(r"test (.+) \.\.\. ok",line)
                require(match is not None,"actual successful libtest method "+str(index))
                found.append(match.group(1))
            require(len(found)==len(set(found))==len(expected) and sorted(found)==sorted(expected),"complete exact unique libtest method population "+str(index))
            summaries=[line for line in text.splitlines() if line.startswith("test result:")]
            require(len(summaries)==1,"single original libtest terminal summary "+str(index))
            match=re.fullmatch(r"test result: ok\. (\d+) passed; 0 failed; 0 ignored; 0 measured; (\d+) filtered out; finished in ([0-9.]+)s",summaries[0])
            require(match is not None and int(match.group(1))==len(expected),"original zero-failed/ignored/measured summary "+str(index))
            expected_population={"actual_names":sorted(found),"expected_count":len(expected),"failed":0,"ignored":0,"measured":0,"passed":len(expected)}
            require(row["test_population"]==expected_population,"recorded population equals independently parsed original stdout "+str(index))
            populations.append({"command_index":index,**expected_population,"filtered_out":int(match.group(2)),"libtest_elapsed_seconds":float(match.group(3))})
    observed=[]
    metrics=("statements","rows","vm_steps","sorts","fullscan_steps")
    budgets=[0,1,2,8,40,2**64-1]
    for kind,selection_count,expected_count,stmt_count in (("surface",3,54,9),("dependency",2,36,4)):
        name=f"observations/p8-prepare-{kind}-window-work.json"
        rows=parse(contents[name])
        require(len(rows)==expected_count,"complete original "+kind+" observation population")
        sums={scope:{m:0 for m in metrics} for scope in ("original_two_lookup_work","retained_single_lookup_work")}
        for i,row in enumerate(rows):
            require(row["budget"]==budgets[i//(3*selection_count)] and row["excluded"]==[0,3,39][(i//selection_count)%3],"original ordered "+kind+" budget/exclusion stratum "+str(i))
            require(type(row["admitted"]) is int and 0<=row["admitted"]<=row["budget"] and type(row["pending"]) is bool,"valid actual "+kind+" admission and pending "+str(i))
            require(row["scope"]=="actual SQL statement counters; no whole-build latency claim","unchanged observation scope "+kind+str(i))
            for scope in sums:
                work=row[scope]
                require(set(work)==set(metrics) and all(type(work[m]) is int and work[m]>=0 for m in metrics),"complete actual SQL counters "+kind+str(i)+scope)
                for m in metrics:sums[scope][m]+=work[m]
            old=row["original_two_lookup_work"]; retained=row["retained_single_lookup_work"]
            require(old["statements"]==2*stmt_count and retained["statements"]==stmt_count and old["fullscan_steps"]==retained["fullscan_steps"]==0 and all(old[m]>=retained[m] for m in metrics),"actual retained query work is a strict statement subset "+kind+str(i))
        observed.append({"kind":kind,"original_member":name,"bytes":len(contents[name]),"sha256":sha(contents[name]),"cases":len(rows),"per_case_statements":{"original":2*stmt_count,"retained":stmt_count},"aggregate_actual_counters":sums,"scope":"Only these bounded correctness fixtures; no 100k or whole-build wall-clock extrapolation."})
    after=archive.stat()
    require(before.st_ino==after.st_ino and before.st_size==after.st_size and before.st_mtime_ns==after.st_mtime_ns and sha(archive.read_bytes())==p["archive"]["sha256"],"original ZIP unchanged through independent full audit")
    report.update(status="accepted_scoped_original_surface_window_controls",original_archive=p["archive"],complete_original_members=sorted(inv,key=lambda r:r["path"]),expanded_original_bytes=370401,source={"commit":source["source_commit"],"tree":source["source_tree"],"input_count":source["input_count"],"manifest_sha256":source["manifest_sha256"],"registry_blob":p["approved_registry"]["blob"]},original_guard_output=guard,observers=actual_observers,original_command_results=receipt["results"],independently_parsed_test_populations=populations,actual_SQL_observations=observed,original_API_binding={"run":run["id"],"job":jobs["jobs"][0]["id"],"artifact":art["id"],"attempt":1,"all_terminal_success":True},audit_input={"bytes":len(data),"sha256":sha(data)},limits={"no_original_test_rerun":True,"no_product_or_Cargo_execution":True,"ordinary_CI_not_substituted":True,"no_latency_gain_claim":True,"no_N150_started_or_altered":True,"no_TODO_status_change":True},TODO={"newly_closed":0,"remaining":29})
except Exception as error:
    report["error"]=type(error).__name__+": "+str(error)
report["checks"]=checks
report["check_count"]=len(checks)
result=(json.dumps(report,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
(out/"review.json").write_bytes(result)
print(json.dumps({"status":report["status"],"checks":len(checks),"report_path":str(out/"review.json"),"report_bytes":len(result),"report_sha256":sha(result),"error":report.get("error"),"actual_SQL_observations":report.get("actual_SQL_observations")},sort_keys=True))
if report["status"]!="accepted_scoped_original_surface_window_controls":raise SystemExit(1)
