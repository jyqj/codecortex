"""Independent audit of fixed G4 safe-report delivery; no executable replay."""
import collections, datetime, hashlib, json, re, stat, sys, time, zipfile, zlib
from pathlib import Path, PurePosixPath
BASE=Path("artifacts/checkpoints/p8-round17-evidence-intake-20261009/root-safe-g4")
OUT=BASE/"independent-review"
G4="260f596582f2d82b8d7c707b61a6b8b6a43b069f"
CONTROL="9aacfbea2771209134b4f2dd8889cbb4e2a3d099"
INTAKE="c87fd64c9e543ffd75244b17f8b283791052cb0b"
REPORT_SHA="fb0b82515cabb32fd79543e104bf85c481141876e48c09475cfed4c147eae48d"
ZIP_SHA="092aeba0160d2cc598360cc2380c1e170a5650c244f23660811257d0626c640c"
PINS={"G2-platform-registration.json":{"bytes":7163,"sha256":"d45b3e45b2b817a81274fcadb1b7726370d0ddda8df2eed2282b80cf62e66124","git_blob":"85a6238c80683f3ce96a0f5bf743ccedbc99d48a"},"controller.py":{"bytes":15329,"sha256":"bf315f5de2766ecb34562559f201b858c9e491738f23b8e269ceaa024820433a","git_blob":"8d79df11d35cd4719a2920e2862301cda7b1e84c"},"disk_zip.py":{"bytes":4899,"sha256":"eb7cb8012cfd4db62c77d0e3abdcd11265fa06f12ccb5f8a1b3062ffb2f079d8","git_blob":"19c1a550c76470668e2a8a2c8b8071de7bccbeaa"},"platform_intake.py":{"bytes":11173,"sha256":"ad8f8c5f1c41cb5d500d8bc54017d8f226349ee29f3e35dc24d6d54d0778c2be","git_blob":"6810776bf8ae20e6001e87ea2db2e644b10dd8a5"},"runtime_intake.py":{"bytes":22534,"sha256":"c1d7d1bc2f3d0c529c042db9181a946f4bc77eb1eac03c1331b2203a4c2ea16c","git_blob":"7f114b9aa06a7fecf2f48ee6dd256773daea35a7"}}
SPEC=[
 ("mixed/C1","mixed/C1-artifact-11590168318.zip",11590168318,16588933,"d793ce76d6da48c3cc96b28afad80c8eeab5576c4cb4d7ad364e5c823eb35355"),
 ("mixed/C4","mixed/C4-artifact-11590202791.zip",11590202791,16959276,"0adef902e90e21f9d32349d330a4f0d48041ae336878e826ab5b6beca67eec24"),
 ("mixed/C8","mixed/C8-artifact-11590639057.zip",11590639057,16621243,"befead56e71df69ab5c88d0483de67b9c2009b99ee82df0e683f99a25844daa7"),
 ("mixed/C16","mixed/C16-artifact-11588372461.zip",11588372461,16388340,"ad788f605deb6e55e7ad3d944394e079a5983ea774560869d2a9b09624e22e2a"),
 ("soak","soak/artifact-11590202308.zip",11590202308,50994513,"46a72baaecbd0e5ea3197d05e639f618cd3a570b9cc65f46fa51a5f0a4916286")]
checks=collections.Counter()
def ck(ok,label):
    if not ok: raise AssertionError(label)
    checks[label]+=1
def sha(b):return hashlib.sha256(b).hexdigest()
def jb(x):return (json.dumps(x,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
def oid(b):return hashlib.sha1(b"blob "+str(len(b)).encode()+b"\0"+b).hexdigest()
def stable(p):
    ck(not p.is_symlink() and stat.S_ISREG(p.stat().st_mode),"regular owned fixed input")
    b=p.read_bytes()
    return b,dict(bytes=len(b),sha256=sha(b),inode=p.stat().st_ino)
def meta(row):
    ck(set(row)=={"bytes","sha256","crc32"} and type(row["bytes"]) is int and row["bytes"]>=0
       and re.fullmatch("[0-9a-f]{64}",row["sha256"]) and re.fullmatch("[0-9a-f]{8}",row["crc32"]),"complete member metadata types")
def pathok(n):
    p=PurePosixPath(n)
    ck(not p.is_absolute() and ".." not in p.parts and str(p)==n and "\\" not in n and "\0" not in n and len(n.encode())<=1024,"safe inventory path")
def content(raw,row,label):
    ck(len(raw)==row["bytes"] and sha(raw)==row["sha256"] and ("%08x"%(zlib.crc32(raw)&0xffffffff))==row["crc32"],label)
def main():
    started=datetime.datetime.now(datetime.timezone.utc).isoformat();mono=time.monotonic()
    raw,rid=stable(BASE/"complete-safe-report.json");zr,zid=stable(BASE/"g4-safe-report-11596847982.zip")
    ck(rid["bytes"]==10051680 and rid["sha256"]==REPORT_SHA,"fixed complete safe report")
    ck(zid["bytes"]==2241291 and zid["sha256"]==ZIP_SHA,"fixed downloaded safe ZIP")
    import io
    with zipfile.ZipFile(io.BytesIO(zr)) as z:
        ck(z.namelist()==["complete-safe-report.json"],"safe ZIP exact sole member")
        zi=z.infolist()[0];member=z.read(zi)
        ck(member==raw and zi.file_size==len(raw) and z.testzip() is None,"safe ZIP full CRC size and byte identity")
    d=json.loads(raw);e=d["evidence"];s=e["selected_original_outputs"];get=lambda n:json.loads(s[n]);outer=e["actual_outer_members"]
    ck(d["schema"]=="p8-fixed-outer-full-intake-v1" and d["status"]=="accepted_scoped_full_original_delivery_intake" and d["exit_code"]==0,"actual intake status")
    ck(d["controller_commit"]==INTAKE and d["measured_source"]==G4 and d["original_receiver"]==CONTROL and d["original_run"]==37879784342 and d["original_artifact"]==11593443086,"three source/controller identities")
    for k in ("native_statistics_or_product_or_Cargo_executed","original_measurements_rerun","raw_originals_relabelled"):ck(d[k] is False,"no new measurement or relabel")
    ck(d["TODO_closed"]==e["TODO_closed"]==0 and d["TODO_remaining"]==e["TODO_remaining"]==29,"unchanged TODO scope")
    ck(d["controller_inputs_before"]==d["controller_inputs_after"]==PINS,"five exact reviewed controller input blobs")
    cap=d["capacity_before"]
    ck(cap["reserve_bytes"]==536870912 and cap["allowance_bytes"]==134217728 and cap["registered_nested_bytes"]==117552305 and cap["registered_outer_bytes"]==233905529,"unchanged capacity components")
    ck(cap["required_bytes"]==sum(cap[k] for k in ("reserve_bytes","allowance_bytes","registered_nested_bytes","registered_outer_bytes")) and cap["free_bytes"]>=cap["required_bytes"] and d["capacity_after"]["free_bytes"]>=536870912,"recorded capacity admission")
    api=d["original_api"];run=api["run"]
    ck(run["id"]==37879784342 and run["head_sha"]==CONTROL and run["run_attempt"]==1 and run["status"]=="completed" and run["conclusion"]=="success","original receiver first-attempt API")
    jobs=api["jobs"]["jobs"];arts=api["artifacts"]["artifacts"]
    ck(api["jobs"]["total_count"]==len(jobs)==1 and jobs[0]["id"]==113656573097 and jobs[0]["name"]=="offline_replay" and jobs[0]["head_sha"]==CONTROL and jobs[0]["conclusion"]=="success","complete original job population")
    ck(api["artifacts"]["total_count"]==len(arts)==1,"complete original artifact population")
    a=arts[0]
    ck(a["id"]==11593443086 and a["size_in_bytes"]==233905529 and a["digest"]=="sha256:7bc22b041108af5bf8f29b571fe094c35ede0f92cb56c93ad29bcd4c8f9ff1b6" and a["workflow_run"]["id"]==37879784342 and a["workflow_run"]["head_sha"]==CONTROL,"original fixed artifact API")
    ck(len(d["transfers"])==5 and all(t["curl_exit"]==0 and t["http_status"]=="200" and t["status"]=="downloaded" and 0<t["bytes"]<=t["maximum_bytes"] for t in d["transfers"]),"five actual successful bounded transfers")
    ck(e["schema"]=="p8-G4-external-disk-intake-v1" and e["status"]=="accepted_scoped_original_receiver_delivery" and e["controller_commit"]==CONTROL and e["source_commit"]==G4,"original intake profile")
    ck(e["artifact_id"]==a["id"] and e["zip_bytes"]==a["size_in_bytes"] and "sha256:"+e["zip_sha256"]==a["digest"],"original archive identity cross-binding")
    ck(len(outer)==10743 and len(s)==53,"complete safe output populations")
    for n,row in outer.items():pathok(n);meta(row)
    for n,body in s.items():ck(type(body) is str and n in outer,"selected originals present");content(body.encode(),outer[n],"selected original full bytes CRC SHA")
    inventory=get("file-inventory.json");receipt=get("receipt.json");withheld=get("withheld-original-job-logs.json")
    ck(receipt==e["receipt"]==d["original_job_log"]["safe_original_controller_receipt"],"original receipt exact mirrored values")
    ck(receipt["status"]=="accepted_scoped_original_G4_offline_replay" and receipt["exit_code"]==0 and receipt["controller_commit"]==CONTROL and receipt["source_commit"]==G4 and receipt["original_run"]==37854847820,"original receiver receipt identity")
    ck(receipt["original_artifact_ids"]==[x[2] for x in SPEC] and receipt["new_product_workload"] is False and receipt["original_sources_relabelled"] is False,"exact original five-artifact cohort")
    ck(sha(s["file-inventory.json"].encode())==receipt["file_inventory_sha256"] and sha(s["withheld-original-job-logs.json"].encode())==receipt["withheld_original_job_logs_sha256"],"receipt seals its inventories")
    incl={n:r for n,r in inventory.items() if r["included_in_upload"]};excl={n:r for n,r in inventory.items() if not r["included_in_upload"]}
    ck(set(outer)==set(incl)|{"file-inventory.json","receipt.json"},"bidirectional full outer inventory")
    for n,r in incl.items():ck(all(r[k]==outer[n][k] for k in ("bytes","sha256")),"outer seal entry exact")
    fixedlogs={"mixed/C1/job-113576447129.log","mixed/C4/job-113576447195.log","mixed/C8/job-113576447334.log","mixed/C16/job-113576446998.log","soak/job-113576447137.log"}
    ck(set(excl)==set(withheld["files"])==fixedlogs and withheld==e["original_job_logs_withheld"],"exact documented withheld log scope")
    for n,r in excl.items():ck(all(r[k]==withheld["files"][n][k] for k in ("bytes","sha256")),"withheld log identity")
    ck(d["original_job_log"]["included_in_public_outputs"] is False and set(d["original_job_log"]["safe_checkout_commits"])=={CONTROL,G4},"safe log provenance")
    source=get("source-before.json");obs=get("observer-before.json")
    ck(s["source-before.json"]==s["source-after.json"] and s["observer-before.json"]==s["observer-after.json"],"raw before-after source bytes identical")
    ck(source==e["source"] and obs==e["observer"] and "source_root" not in source and "source_root" not in obs,"actual runtime schema preserved")
    ck(source["source_commit"]==G4 and source["source_tree"]=="645404431ca15770d6e729785a3ada68b096c593" and source["input_count"]==len(source["inputs"])==1087,"complete G4 production population")
    ck(sha(jb(source["inputs"]))==source["manifest_sha256"]=="517ff2f445357534232a56aadd8bef93f30987ce6127bd55b0c01cb9a0b15268","independently recomputed production manifest")
    ck(obs["source_commit"]==G4 and len(obs["files"])==7 and sha(jb(obs["files"]))==obs["manifest_sha256"]=="7874353103743d0cf187a77fa204a44c65cfb957989c46ee3bff849d0a87ab05","independently recomputed seven-observer manifest")
    remote_root=get("commands/seal-11590168318/command.json")["cwd"]
    py,flag,soakpath=get("commands/original-soak-review/command.json")["argv"];outroot=soakpath[:-len("/soak/inspect.py")]
    ck(flag=="-B" and Path(py).name in ("python3","python3.12","python") and Path(remote_root).is_absolute() and ".." not in Path(remote_root).parts,"exact original offline role and root")
    cmdresults=[]
    for name,home in [(f"seal-{x[2]}",x[0]) for x in SPEC]+[("original-mixed-review",None),("original-soak-review",None)]:
        c=get("commands/"+name+"/command.json");res=get("commands/"+name+"/result.json")
        if home:
            orig=outroot+"/"+home+"/originals"
            argv=[py,"-B",remote_root+"/scripts/p8_runtime.py","verify","--output",orig+"/p8-runtime","--build-output",orig+"/p8-build"];cwd=remote_root
        else:
            argv=[py,"-B",outroot+("/mixed/review_mixed.py" if name=="original-mixed-review" else "/soak/inspect.py")];cwd=None
        ck(c==dict(argv=argv,cwd=cwd,source_commit=G4,scope="offline replay only") and res["exit_code"]==0,"seven original exact argv cwd and exit")
        if home:
            v=json.loads(s["commands/"+name+"/stdout.log"])
            ck(v["status"]=="sealed_artifacts_verified" and v["files"]==2108 and v["observation_status"]=="passed_observation" and v["observation_exit_code"]==0,"actual original paired seal verifier result")
        cmdresults.append({"name":name,"exit_code":res["exit_code"],"wall_seconds":res["wall_seconds"]})
    bodies={"mixed/review_mixed.py":"e0e6002810aa7722ed070483532a7853e9971f7f45f38ea5355caf9e5920825b","soak/inspect.py":"4fa1e18e355cd303b0ed8df986438f5965fb186ee413249b0ac62a434a6705c1","soak/cache_wire.py":"7ddec02ac351832562a7c0ff367cbccead2291c0fb447b91600c248ded57a5ea"}
    mapped=get("receiver-mapping.json")
    for n,h in bodies.items():
        body=s[n]
        if n=="mixed/review_mixed.py":
            old="REPO=Path("+repr(remote_root)+")";new="REPO=Path('/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source')"
        elif n=="soak/inspect.py":
            old="REPO = "+repr(remote_root);new="REPO = '/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source'"
        else:old=None
        if old:ck(body.count(old)==1,"single allowed receiver root mapping");body=body.replace(old,new)
        ck(sha(body.encode())==h==mapped[n]["selected_receiver_sha256"] and sha(s[n].encode())==mapped[n]["mapped_sha256"],"original reviewed receiver bodies")
    nested={z["artifact_id"]:z for z in e["actual_original_ZIPs"]};ck(set(nested)=={x[2] for x in SPEC},"five unique nested archives")
    totalmeta=sum(len(json.dumps([n,r],ensure_ascii=True).encode()) for n,r in outer.items());totalmembers=len(outer);sealbytes=0
    for home,zipname,aid,size,h in SPEC:
        z=nested[aid];ck(z["bytes"]==size and z["sha256"]==h and outer[zipname]["bytes"]==size and outer[zipname]["sha256"]==h,"immutable nested ZIP identity")
        inner=z["members"];pre=home+"/originals/"
        ck(len(inner)==2125 and {n for n in outer if n.startswith(pre)}=={pre+n for n in inner},"bidirectional nested complete population")
        for n,row in inner.items():
            pathok(n);meta(row);ck(row==outer[pre+n],"every nested member identical to expanded original")
            totalmeta+=len(json.dumps([n,row],ensure_ascii=True).encode())
        totalmembers+=len(inner)
        sealbytes+=sum(inner[n]["bytes"] for n in ("p8-build/seal.json","p8-runtime/seal.json"))
        build=get(pre+"p8-build/build-receipt.json")
        ck(build["source_before"]==build["source_after"]==source and build["observer_before"]==build["observer_after"]==obs and build["status"]=="passed" and build["build_exit_code"]==0,"all original build provenance unchanged")
        buildroot="/home/runner/work/codecortex/codecortex";target=build["target_dir"]
        expect=["cargo","build","--release","--locked","--offline","--no-default-features","-p","cc-server","--bin","codecortex","-p","cc-eval","--bin","p8-oracle","--bin","p8-runtime-statistics","--message-format=json-render-diagnostics","--target-dir",target]
        ck(build["build_command"]==expect and build["schema_version"]==2 and build["cold_build_claim"] is False,"original release Cargo command not cold claim")
        for name,ar in build["artifacts"].items():
            car=ar["cargo_artifact"];cp=ar["copy_source"];pr=outer[pre+"p8-build/"+name]
            crate="cc-server" if name=="codecortex" else "cc-eval";src="src/main.rs" if name=="codecortex" else "src/bin/"+name+".rs"
            ck(car["reason"]=="compiler-artifact" and car["manifest_path"]==buildroot+"/crates/"+crate+"/Cargo.toml" and car["target"]["src_path"]==buildroot+"/crates/"+crate+"/"+src and car["target"]["name"]==name and car["target"]["kind"]==["bin"],"exact original Cargo target manifest source")
            ck(car["executable"]==cp["path"]==target+"/release/"+name and cp["bytes"]==ar["binary_bytes"]==pr["bytes"] and cp["sha256"]==ar["binary_sha256"]==pr["sha256"],"exact Cargo executable copy chain")
            ck(car["profile"]["opt_level"]=="3" and not car["profile"]["debug_assertions"] and not car["profile"]["test"],"actual release profile")
        ck(len(build["artifacts"])==3,"three original binary artifacts")
        for hkey in ("cargo_log_sha256","stderr_sha256"):
            ck(any(r["sha256"]==build[hkey] for n,r in inner.items() if n.startswith("p8-build/")),"sealed original Cargo log hash")
        sp=pre+"p8-runtime/statistics.json";rp=home+("/independent-statistics-replay/statistics.json" if home.startswith("mixed") else "/independent-review-01/statistics.json")
        ck(outer[sp]==outer[rp],"five original statistics replay bytes CRC SHA")
        ck(outer[pre+"p8-build/p8-runtime-statistics"]["sha256"]==build["statistics_sha256"],"retained statistics executable source binding")
        if home.startswith("mixed"):
            rr=get(home+"/independent-statistics-replay/result.json")
            ck(rr["exit_code"]==0 and rr["byte_identical"] and rr["binary_sha256"]==build["statistics_sha256"] and rr["source_commit"]==G4,"actual mixed statistics replay result")
            ck(rr["input_sha256"]=={"plan":outer[pre+"p8-runtime/plan.json"]["sha256"],"raw":outer[pre+"p8-runtime/raw.jsonl"]["sha256"]} and rr["output_sha256"]==outer[rp]["sha256"],"mixed replay exact original inputs and output")
    selectedbytes=sum(len(x.encode()) for x in s.values())
    ck(e["budgets"]==dict(member_count=totalmembers,metadata_bytes=totalmeta,selected_bytes=selectedbytes+sealbytes),"recomputed complete receiver accounting including ten seals")
    ck(totalmembers==21368 and totalmeta<=8*1024**2 and selectedbytes+sealbytes<=16*1024**2 and all(len(x.encode())<=4*1024**2 for x in s.values()),"original metadata selected-byte ceilings")
    mixed=get("mixed/independent-review.json");soak=get("soak/inspection.json")
    ck(mixed==e["original_mixed_replay"] and soak==e["original_soak_replay"],"selected original review exact embedded values")
    cells={x["configured_concurrency"]:x for x in mixed["cells"]};ck(set(cells)=={1,4,8,16} and mixed["status"]=="accepted_scoped_mixed_raw","four distinct mixed strata")
    cellsummary=[]
    for c,x in sorted(cells.items()):
        ck(x["source"]==G4 and x["offered"]==900 and x["outcomes"]=={"success":900} and x["primary_N"]=={"read":600,"build":300},"mixed original full denominators")
        ck(len(x["mutation_N"])==6 and set(x["mutation_N"].values())=={50},"mutation strata not pooled")
        ck(x["initial_full_index_calls"]==1 and x["incremental_index_calls"]==300 and x["repair_index_calls"]==0 and len(x["original_oracle_tables"])==15,"mixed no-repair original endpoint parity")
        ck(x["read_build_overlap"] is (c>1) and x["observed_driver_peak"]==x["wire_peak"] and 1<=x["wire_peak"]<=c,"actual concurrency not configured-peak fiction")
        ck(x["native_RSS_positive"]==x["resource_samples"] and x["resource_samples"]>=448 and x["cpu_and_io_monotonic"] and x["runner_pid"]!=x["server_pid"],"mixed native resource role attribution")
        cellsummary.append({k:x[k] for k in ("configured_concurrency","offered","primary_N","observed_driver_peak","read_build_overlap","resource_samples","catalog_compactions")})
    ck(soak["decision"]=="accepted_scoped_original_G4_soak_observation" and soak["source_commit"]==G4 and soak["operations"]=={"build":1201,"read":2400} and soak["outcomes"]=={"success":3601},"soak original full offered denominator")
    ck(soak["observed_work_ns"]>=3600*10**9 and soak["original_parity_exit_code"]==0 and soak["parity_tables"]==15 and soak["branch_switches"]==200 and soak["catalog_compactions"]==25,"original one-hour branch catalog parity scope")
    rc=soak["resource_coverage"];rss=soak["rss"]
    ck(rc["passed"] and rc["allowed_maximum_gap_ns"]==5*10**9 and rc["maximum_gap_ns"]<=rc["allowed_maximum_gap_ns"] and rc["observed_samples"]==3594 and rc["work_end_ns"]-rc["work_start_ns"]==soak["observed_work_ns"],"one-hour resource coverage and original gap")
    ck(rss["passed"] and rss["observed"]==3594 and rss["allowed_bytes"]==rss["warmed_median_bytes"]*1.25+32*1024**2 and rss["tail_median_bytes"]<=rss["allowed_bytes"],"original warmed RSS bound")
    cache=soak["cache_reuse"];ck(cache["passed"] and cache["errors"]==[] and cache["observed"]=={"hit":1400,"miss":1000,"invalidations":999},"actual nonzero cache hit miss invalidation")
    ck(cache["validated_reads"]==cache["recorded_offered_reads"]==cache["expected_offered_reads"]==2400 and cache["status_request_counts"]=={"success":4800} and all(cache["request_counts"][r]=={"success":2400} for r in ("before_status","symbol","hybrid","after_status")),"all four RPC-role denominators")
    ck(len(cache["time_quarters"])==4 and sum(q["reads"] for q in cache["time_quarters"])==2400 and all(q["hits"]>0 and q["misses"]>0 and q["invalidations"]>0 and set(q["mutations"])==set(cells[1]["mutation_N"]) and min(q["mutations"].values())>0 for q in cache["time_quarters"]),"actual completion-time quarter coverage")
    ck(sum(sum(q["mutations"].values()) for q in cache["time_quarters"])==1201 and "never offered time" in cache["quarter_time_basis"],"mutation quarter denominator and time basis")
    binding=get("soak/independent-review-01/cache-wire-binding.json");mp=binding["mapping"]
    mappingraw=jb(mp);ck(len(mp)==9600 and oid(mappingraw)==e["exact_9600_mapping_git_blob"]=="95d8e14c6473638d4c4063b31b52864eb85b274f","all mapping raw bytes exactly prior independent original diagnosis")
    ck(binding["compound_reads"]==2400 and binding["bound_RPCs"]==9600 and collections.Counter(x["role"] for x in mp)==binding["role_counts"]==dict(before_status=2400,symbol=2400,hybrid=2400,after_status=2400),"all 9600 unique role claims")
    ids=collections.defaultdict(dict)
    for x in mp:ck(set(x)=={"operation_id","role","rpc_id"} and x["role"] not in ids[x["operation_id"]],"unique operation role");ids[x["operation_id"]][x["role"]]=x["rpc_id"]
    ck(set(ids)=={i for i in range(3601) if i%3} and len({x["rpc_id"] for x in mp})==9600,"exact original read population and one-to-one wire IDs")
    ck(all(v["before_status"]<v["symbol"]<v["hybrid"]<v["after_status"] for v in ids.values()),"four role causal request order")
    ep=binding["endpoint_binding"];ck(ep==e["original_endpoint_binding"],"endpoint exact embedded binding")
    ck(ep["incremental_rpc_id"]==14400 and ep["full_rpc_id"]==4 and ep["endpoint_public_record_index"]==7198 and ep["exact_parameters"]==dict(name="search",arguments=dict(mode="symbol",query="p8_runtime_stable_signal",top_k=5)),"unique separately emitted endpoint identity")
    lo,hi=binding["common_monotonic_origin_interval_ns"]
    ck(lo<=hi and ep["incremental_request_ns"]>hi+ep["latest_raw_operation_finished_ns"] and ep["incremental_request_ns"]>ep["latest_bound_read_response_ns"] and ep["incremental_request_ns"]<ep["incremental_response_ns"]<=ep["full_request_ns"]<ep["full_response_ns"],"endpoint after workload and complete bound reads")
    ck(14400 not in {x["rpc_id"] for x in mp} and binding["unbound_status_RPCs"]==3595,"endpoint not hidden in workload counts")
    for side,n in (("product",14400),("full-product",4)):
        t=soak["transport"][side];ck(t["stdout_eof_observed"] and t["process_exit_0_observed"] and all(t["kinds"][k]==n for k in ("request","response","stdout_wire","rpc_send")),"original complete transport populations and stop")
    stats=get("soak/independent-review-01/statistics.json")
    ck(stats["exit_code"]==0 and stats["expected_samples"]==stats["recorded_samples"]==3601 and stats["missing_samples"]==stats["unexpected_samples"]==0 and stats["outcomes"]=={"success":3601},"original Rust all-attempt statistics")
    ck(stats["plan_sha256"]==outer["soak/originals/p8-runtime/plan.json"]["sha256"] and stats["raw_sha256"]==outer["soak/originals/p8-runtime/raw.jsonl"]["sha256"] and not stats["new_performance_gate"] and not stats["release_certified"],"statistics original input and scope")
    ck({x["operation"]:x["expected_samples_in_group"] for x in stats["by_operation"]}=={"read":2400,"build":1201} and sum(x["expected_samples_in_group"] for x in stats["by_build_mutation"])==1201,"statistics overlapping views are not pooled")
    ck(soak["statistics_replay"]["exit_code"]==0 and soak["statistics_replay"]["byte_identical"] and e["original_statistics_executed_here"] is False and e["product_or_Cargo_executed"] is False,"prior actual statistics replay not repeated here")
    afterraw,afterrid=stable(BASE/"complete-safe-report.json");afterzip,afterzid=stable(BASE/"g4-safe-report-11596847982.zip")
    ck(afterraw==raw and afterzip==zr and afterrid==rid and afterzid==zid,"fixed evidence unchanged after audit")
    report=dict(schema="p8-G4-safe-delivery-independent-review-v1",status="accepted_scoped_original_G4_safe_delivery",reviewer="/root/acceptance_audit",started_at=started,finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),wall_seconds=time.monotonic()-mono,measured_source=G4,original_receiver=CONTROL,intake_controller=INTAKE,safe_artifact_id=11596847982,safe_ZIP=zid,safe_report=rid,actual_native_audit=True,new_product_Cargo_statistics_or_workload_executed=False,checks=dict(checks),observations=dict(outer_members=len(outer),nested_members=totalmembers-len(outer),total_member_records=totalmembers,selected_original_files=len(s),selected_original_bytes=selectedbytes,ten_nested_seal_bytes=sealbytes,source_inputs=1087,observer_files=7,original_commands=cmdresults,mixed=cellsummary,soak=dict(offered=3601,operations=soak["operations"],work_ns=soak["observed_work_ns"],branches=200,catalog_compactions=25,resource=rc,rss=rss,cache=cache["observed"],mapping_entries=len(mp),mapping_git_blob=oid(mappingraw),endpoint=ep)),limits=[
      "This native audit fully rehashed the 2,241,291-byte safe ZIP and 10,051,680-byte report; its sole member CRC/bytes exactly match.",
      "The original 233,905,529-byte delivery and five nested ZIPs were fully CRC/SHA/parsed by the fixed c87 intake on its fresh VM, not re-downloaded or replayed here. Complete 21,368 metadata records and 53 original selected bodies are independently checked here.",
      "The ten inner seal bodies and original raw/RPC/SQLite contents are not embedded in this safe report. Their underlying content predicates rely on the identified reviewed validator's actual execution and seven preserved command receipts; metadata consistency is not a claim to re-execute those content checks locally.",
      "The source and observer map hashes equal the previously fixed G4 manifests. No current peer checkout is read, modified or substituted.",
      "No product/oracle/Cargo or new Rust statistics execution. Prior mixed/soak replay receipts and exact output-byte identities are independently cross-checked.",
      "Configured concurrency is not observed peak. IID confidence intervals, mixed mutation heterogeneity, process-tree unavailability and compound-read scope stay unchanged.",
      "Three exploratory shape outputs were truncated by the tool; they are diagnostic only. This bounded actual audit does not depend on their truncated content.",
      "This G4 result does not relabel G2/P3/a23 evidence or add a prerequisite to a23 closure."
    ],TODO_closed=0,TODO_remaining=29)
    return report
if __name__=="__main__":
    try:
        review=main()
    except BaseException as exc:
        fail=dict(status="independent_audit_failed",error_type=type(exc).__name__,error=str(exc),completed_checks=dict(checks),measurement_failure_claimed=False)
        with (OUT/"attempt-01-failure.json").open("x") as f:json.dump(fail,f,sort_keys=True,indent=2);f.write("\n")
        print(json.dumps({"status":fail["status"],"error_type":fail["error_type"],"error":fail["error"],"check_categories":len(checks)}))
        raise
    rb=jb(review)
    with (OUT/"review.json").open("xb") as f:f.write(rb)
    print(json.dumps({"status":review["status"],"review_bytes":len(rb),"review_sha256":sha(rb),"review_git_blob":oid(rb),"checks":len(checks),"member_records":review["observations"]["total_member_records"],"mapping_entries":review["observations"]["soak"]["mapping_entries"],"TODO_closed":0,"TODO_remaining":29},sort_keys=True))
