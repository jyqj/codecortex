import collections,datetime,hashlib,json,pathlib,re,sys,zipfile,zlib
BASE=pathlib.Path("artifacts/checkpoints/p8-round17-evidence-intake-20261009/root-safe-g4")
OUT=BASE/"author-review"/"v2"
OUT.mkdir(exist_ok=False)
(OUT/"verify_author_reception.py").write_bytes(pathlib.Path(__file__).read_bytes())
checks=[]
def check(ok,label):
    if not ok: raise ValueError(label)
    checks.append(label)
def sha(b): return hashlib.sha256(b).hexdigest()
def gitblob(b): return hashlib.sha1(b"blob "+str(len(b)).encode()+b"\0"+b).hexdigest()
def no_dups(pairs):
    d={}
    for k,v in pairs:
        if k in d: raise ValueError("duplicate JSON key")
        d[k]=v
    return d
def parse(b): return json.loads(b,object_pairs_hook=no_dups)
def canonical(v): return (json.dumps(v,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
G4="260f596582f2d82b8d7c707b61a6b8b6a43b069f"
CONTROL="9aacfbea2771209134b4f2dd8889cbb4e2a3d099"
INTAKE="c87fd64c9e543ffd75244b17f8b283791052cb0b"
SPEC=[
 ("mixed/C1","mixed/C1-artifact-11590168318.zip",11590168318,16588933,"d793ce76d6da48c3cc96b28afad80c8eeab5576c4cb4d7ad364e5c823eb35355"),
 ("mixed/C4","mixed/C4-artifact-11590202791.zip",11590202791,16959276,"0adef902e90e21f9d32349d330a4f0d48041ae336878e826ab5b6beca67eec24"),
 ("mixed/C8","mixed/C8-artifact-11590639057.zip",11590639057,16621243,"befead56e71df69ab5c88d0483de67b9c2009b99ee82df0e683f99a25844daa7"),
 ("mixed/C16","mixed/C16-artifact-11588372461.zip",11588372461,16388340,"ad788f605deb6e55e7ad3d944394e079a5983ea774560869d2a9b09624e22e2a"),
 ("soak","soak/artifact-11590202308.zip",11590202308,50994513,"46a72baaecbd0e5ea3197d05e639f618cd3a570b9cc65f46fa51a5f0a4916286")]
outcome={"schema":"p8-G4-complete-safe-report-author-reception-v1","author":"scale_engineering","role":"author reception; not non-author independent approval","status":"incomplete_not_certified","started_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"new_product_Cargo_statistics_execution":False,"TODO_closed":0,"TODO_remaining":29}
try:
    raw=(BASE/"complete-safe-report.json").read_bytes()
    check(len(raw)==10051680 and sha(raw)=="fb0b82515cabb32fd79543e104bf85c481141876e48c09475cfed4c147eae48d","complete report original byte length/SHA")
    zraw=(BASE/"g4-safe-report-11596847982.zip").read_bytes()
    check(len(zraw)==2241291 and sha(zraw)=="092aeba0160d2cc598360cc2380c1e170a5650c244f23660811257d0626c640c","complete safe-report ZIP fixed bytes/SHA")
    with zipfile.ZipFile(BASE/"g4-safe-report-11596847982.zip") as z:
        infos=z.infolist()
        check(len(infos)==1 and infos[0].filename=="complete-safe-report.json" and not infos[0].is_dir(),"safe-report ZIP exact one-file denominator")
        check(z.read(infos[0])==raw and (zlib.crc32(raw)&0xffffffff)==infos[0].CRC,"safe-report ZIP entire member actual CRC and exact preserved file")
    r=parse(raw); e=r["evidence"]
    check(r["status"]=="accepted_scoped_full_original_delivery_intake" and r["exit_code"]==0 and r["profile"]=="G4-runtime","actual full-intake result")
    check(r["controller_commit"]==INTAKE and r["original_receiver"]==CONTROL and r["measured_source"]==G4 and r["original_run"]==37879784342 and r["original_artifact"]==11593443086,"three distinct exact source/controller identities")
    check(r["native_statistics_or_product_or_Cargo_executed"] is False and r["original_measurements_rerun"] is False and r["raw_originals_relabelled"] is False,"intake no new native execution/relabel")
    pins={"G2-platform-registration.json":"85a6238c80683f3ce96a0f5bf743ccedbc99d48a","controller.py":"8d79df11d35cd4719a2920e2862301cda7b1e84c","disk_zip.py":"19c1a550c76470668e2a8a2c8b8071de7bccbeaa","platform_intake.py":"6810776bf8ae20e6001e87ea2db2e644b10dd8a5","runtime_intake.py":"7f114b9aa06a7fecf2f48ee6dd256773daea35a7"}
    check(r["controller_inputs_before"]==r["controller_inputs_after"] and {k:v["git_blob"] for k,v in r["controller_inputs_before"].items()}==pins,"all five published observer input pins unchanged")
    check(r["capacity_before"]["reserve_bytes"]==r["capacity_after"]["reserve_bytes"]==536870912 and r["capacity_before"]["free_bytes"]>=r["capacity_before"]["required_bytes"] and r["capacity_after"]["free_bytes"]>=536870912,"original 512MiB reserve maintained")
    api=r["original_api"]
    check(api["run"]["id"]==37879784342 and api["run"]["head_sha"]==CONTROL and api["run"]["run_attempt"]==1 and api["run"]["conclusion"]=="success","original receiver exact run API")
    check(api["jobs"]["total_count"]==len(api["jobs"]["jobs"])==1 and api["jobs"]["jobs"][0]["id"]==113656573097 and api["jobs"]["jobs"][0]["conclusion"]=="success","complete original job API")
    check(api["artifacts"]["total_count"]==len(api["artifacts"]["artifacts"])==1 and api["artifacts"]["artifacts"][0]["id"]==11593443086 and api["artifacts"]["artifacts"][0]["size_in_bytes"]==233905529 and api["artifacts"]["artifacts"][0]["digest"]=="sha256:7bc22b041108af5bf8f29b571fe094c35ede0f92cb56c93ad29bcd4c8f9ff1b6","complete original artifact API")
    check(len(r["transfers"])==5 and all(t["status"]=="downloaded" and t["curl_exit"]==0 and t["http_status"]=="200" and t["bytes"]<=t["maximum_bytes"] for t in r["transfers"]),"all fixed API/archive/log transfers complete")
    check(e["status"]=="accepted_scoped_original_receiver_delivery" and e["controller_commit"]==CONTROL and e["source_commit"]==G4 and e["receiver_run"]==37879784342 and e["artifact_id"]==11593443086 and e["zip_bytes"]==233905529 and e["zip_sha256"]=="7bc22b041108af5bf8f29b571fe094c35ede0f92cb56c93ad29bcd4c8f9ff1b6","complete original outer identity")
    members=e["actual_outer_members"]; selected={k:v.encode() for k,v in e["selected_original_outputs"].items()}
    get=lambda n:parse(selected[n])
    check(len(members)==10743 and len(selected)==53,"actual complete outer/selected populations")
    for n,v in members.items():
        p=pathlib.PurePosixPath(n)
        check(not p.is_absolute() and ".." not in p.parts and str(p)==n and "\\" not in n and type(v["bytes"]) is int and v["bytes"]>=0 and re.fullmatch("[0-9a-f]{64}",v["sha256"]) and re.fullmatch("[0-9a-f]{8}",v["crc32"]),"outer metadata "+n)
    for n,b in selected.items():
        check(n in members and len(b)==members[n]["bytes"] and sha(b)==members[n]["sha256"] and "%08x"%(zlib.crc32(b)&0xffffffff)==members[n]["crc32"],"selected exact bytes/CRC/SHA "+n)
    inv=get("file-inventory.json"); withheld=get("withheld-original-job-logs.json"); receipt=get("receipt.json")
    included={n:v for n,v in inv.items() if v["included_in_upload"]}; excluded={n:v for n,v in inv.items() if not v["included_in_upload"]}
    fixed_logs={"mixed/C1/job-113576447129.log","mixed/C4/job-113576447195.log","mixed/C8/job-113576447334.log","mixed/C16/job-113576446998.log","soak/job-113576447137.log"}
    check(set(members)==set(included)|{"file-inventory.json","receipt.json"} and set(excluded)==set(withheld["files"])==fixed_logs,"two-way complete outer inventory and exactly five declared withheld logs")
    for n,v in included.items(): check(all(members[n][k]==v[k] for k in ("bytes","sha256")),"outer sealed "+n)
    for n,v in excluded.items(): check(all(withheld["files"][n][k]==v[k] for k in ("bytes","sha256")),"withheld sealed "+n)
    check(sha(selected["file-inventory.json"])==receipt["file_inventory_sha256"] and sha(selected["withheld-original-job-logs.json"])==receipt["withheld_original_job_logs_sha256"] and receipt==e["receipt"] and withheld==e["original_job_logs_withheld"],"receipt exact inventory seals")
    check(receipt["controller_commit"]==CONTROL and receipt["source_commit"]==G4 and receipt["original_run"]==37854847820 and receipt["status"]=="accepted_scoped_original_G4_offline_replay" and receipt["exit_code"]==0 and receipt["original_artifact_ids"]==[x[2] for x in SPEC],"original receiver receipt and exact five original studies")
    check(receipt["new_product_workload"] is False and receipt["original_sources_relabelled"] is False and withheld["complete_independent_replay_requires_original_actions_logs"] is True,"preserved original failure/exclusion boundary")
    source=get("source-before.json"); observer=get("observer-before.json")
    check(selected["source-before.json"]==selected["source-after.json"] and selected["observer-before.json"]==selected["observer-after.json"] and source==e["source"] and observer==e["observer"],"full original source/observer bytes before/after")
    check(source["source_commit"]==G4 and source["source_tree"]=="645404431ca15770d6e729785a3ada68b096c593" and source["input_count"]==len(source["inputs"])==1087 and sha(canonical(source["inputs"]))==source["manifest_sha256"]=="517ff2f445357534232a56aadd8bef93f30987ce6127bd55b0c01cb9a0b15268","all 1087 source entries canonical manifest")
    check(observer["source_commit"]==G4 and len(observer["files"])==7 and sha(canonical(observer["files"]))==observer["manifest_sha256"]=="7874353103743d0cf187a77fa204a44c65cfb957989c46ee3bff849d0a87ab05","all seven observer entries canonical manifest")
    first=get("commands/seal-11590168318/command.json"); remote_root=first["cwd"]
    check(isinstance(remote_root,str) and pathlib.PurePosixPath(remote_root).is_absolute() and str(pathlib.PurePosixPath(remote_root))==remote_root and ".." not in pathlib.PurePosixPath(remote_root).parts,"actual runtime schema cwd root")
    soakcmd=get("commands/original-soak-review/command.json"); python=soakcmd["argv"][0];remote_output=soakcmd["argv"][2][:-len("/soak/inspect.py")]
    check(pathlib.PurePosixPath(python).name in ("python","python3","python3.12"),"original checker Python role")
    for name in ["seal-%d"%x[2] for x in SPEC]+["original-mixed-review","original-soak-review"]:
        cmd=get("commands/"+name+"/command.json"); result=get("commands/"+name+"/result.json")
        check(result["exit_code"]==0 and cmd["source_commit"]==G4 and cmd["scope"]=="offline replay only","actual checker result "+name)
        if name.startswith("seal-"):
            home=next(x[0] for x in SPEC if name=="seal-%d"%x[2]); path=remote_output+"/"+home+"/originals"
            argv=[python,"-B",remote_root+"/scripts/p8_runtime.py","verify","--output",path+"/p8-runtime","--build-output",path+"/p8-build"];cwd=remote_root
        else:
            argv=[python,"-B",remote_output+("/mixed/review_mixed.py" if name=="original-mixed-review" else "/soak/inspect.py")];cwd=None
        check(cmd["argv"]==argv and cmd["cwd"]==cwd,"exact original checker argv/cwd "+name)
    mapping=get("receiver-mapping.json")
    expected={"mixed/review_mixed.py":"e0e6002810aa7722ed070483532a7853e9971f7f45f38ea5355caf9e5920825b","soak/inspect.py":"4fa1e18e355cd303b0ed8df986438f5965fb186ee413249b0ac62a434a6705c1","soak/cache_wire.py":"7ddec02ac351832562a7c0ff367cbccead2291c0fb447b91600c248ded57a5ea"}
    for n,h in expected.items():
        body=selected[n].decode()
        if n=="mixed/review_mixed.py": before="REPO=Path("+repr(remote_root)+")";after="REPO=Path('/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source')"
        elif n=="soak/inspect.py": before="REPO = "+repr(remote_root);after="REPO = '/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source'"
        else: before=after=None
        if before is not None:
            check(body.count(before)==1,"single approved source-root adaptation "+n);body=body.replace(before,after)
        check(sha(body.encode())==h==mapping[n]["selected_receiver_sha256"] and mapping[n]["mapped_sha256"]==members[n]["sha256"],"original/explicitly derived receiver complete body "+n)
    nested=e["actual_original_ZIPs"];check(len(nested)==5 and len({x["artifact_id"] for x in nested})==5,"all five unique nested originals")
    total_inner=0; stats=[]
    for home,zipname,aid,size,h in SPEC:
        x=next(x for x in nested if x["artifact_id"]==aid); im=x["members"];total_inner+=len(im)
        check(x["bytes"]==members[zipname]["bytes"]==size and x["sha256"]==members[zipname]["sha256"]==h and len(im)==2125,"fixed nested original identity "+str(aid))
        prefix=home+"/originals/"
        expanded={n[len(prefix):]:v for n,v in members.items() if n.startswith(prefix)}
        check(im==expanded,"all nested-to-expanded rows exact both directions "+str(aid))
        build=get(prefix+"p8-build/build-receipt.json")
        check(build["status"]=="passed" and build["build_exit_code"]==0 and build["source_before"]==build["source_after"]==source and build["observer_before"]==build["observer_after"]==observer,"full build before/after provenance "+str(aid))
        for leaf,key in [("codecortex","binary_sha256"),("p8-oracle","oracle_sha256"),("p8-runtime-statistics","statistics_sha256")]:
            check(members[prefix+"p8-build/"+leaf]["sha256"]==build[key],"actual retained binary "+str(aid)+"/"+leaf)
        for path,v in observer["files"].items():
            check(all(im["p8-build/observer-source/"+path][k]==v[k] for k in ("bytes","sha256")),"retained observer "+str(aid)+"/"+path)
        original=prefix+"p8-runtime/statistics.json"; replay=home+("/independent-statistics-replay/statistics.json" if home.startswith("mixed/") else "/independent-review-01/statistics.json")
        check(all(members[original][k]==members[replay][k] for k in ("bytes","sha256")),"full original statistics exact replay bytes "+str(aid))
        if home.startswith("mixed/"):
            sr=get(home+"/independent-statistics-replay/result.json");check(sr["exit_code"]==0 and sr["byte_identical"] is True,"actual retained mixed statistics exit "+str(aid))
        stats.append({"artifact_id":aid,"statistics_sha256":members[original]["sha256"],"statistics_bytes":members[original]["bytes"]})
    check(total_inner==10625 and e["budgets"]["member_count"]==len(members)+total_inner==21368 and e["budgets"]["metadata_bytes"]<=8*1024**2 and e["budgets"]["selected_bytes"]==sum(map(len,selected.values()))+sum(x["members"][n]["bytes"] for x in nested for n in ("p8-build/seal.json","p8-runtime/seal.json"))<=16*1024**2 and all(len(x)<=4*1024**2 for x in selected.values()),"full registered metadata/member/selected budgets")
    mixed=get("mixed/independent-review.json"); soak=get("soak/inspection.json")
    check(mixed==e["original_mixed_replay"] and mixed["status"]=="accepted_scoped_mixed_raw" and sorted(c["configured_concurrency"] for c in mixed["cells"])==[1,4,8,16],"all four exact mixed cells")
    for c in mixed["cells"]:
        check(c["source"]==G4 and c["offered"]==900 and c["outcomes"]=={"success":900} and c["primary_N"]=={"build":300,"read":600} and len(c["original_oracle_tables"])==15 and c["statistics_replay"]["exit_code"]==0 and c["statistics_replay"]["byte_identical"] is True,"mixed original population/statistics C"+str(c["configured_concurrency"]))
    check(soak==e["original_soak_replay"] and soak["decision"]=="accepted_scoped_original_G4_soak_observation" and soak["source_commit"]==G4 and soak["artifact_id"]==11590202308 and soak["outcomes"]=={"success":3601} and soak["operations"]=={"build":1201,"read":2400} and soak["bound_cache_RPCs"]==9600 and soak["parity_tables"]==15 and soak["statistics_replay"]["exit_code"]==0 and soak["statistics_replay"]["byte_identical"] is True,"complete original soak population/full parity/statistics")
    binding=get("soak/independent-review-01/cache-wire-binding.json")
    mapping_bytes=canonical(binding["mapping"])
    check(len(binding["mapping"])==9600 and gitblob(mapping_bytes)=="95d8e14c6473638d4c4063b31b52864eb85b274f"==e["exact_9600_mapping_git_blob"],"all 9600 original RPC mapping exact byte/Git identity")
    check(binding["compound_reads"]==2400 and binding["bound_RPCs"]==9600 and binding["role_counts"]==dict(before_status=2400,symbol=2400,hybrid=2400,after_status=2400),"unchanged compound workload denominator")
    ops=collections.defaultdict(set);ids=set()
    for row in binding["mapping"]:
        check(set(row)=={"operation_id","role","rpc_id"} and row["rpc_id"] not in ids and row["role"] not in ops[row["operation_id"]],"unique original operation/role/RPC binding")
        ids.add(row["rpc_id"]);ops[row["operation_id"]].add(row["role"])
    check(len(ops)==2400 and all(v=={"before_status","symbol","hybrid","after_status"} for v in ops.values()),"four exact roles for every original compound read")
    endpoint=binding["endpoint_binding"];check(endpoint==e["original_endpoint_binding"],"original endpoint selected/evidence exact")
    check(endpoint["incremental_rpc_id"]==14400 and endpoint["full_rpc_id"]==4 and endpoint["endpoint_public_record_index"]==7198 and endpoint["exact_parameters"]==dict(name="search",arguments=dict(query="p8_runtime_stable_signal",mode="symbol",top_k=5)),"unique separate original endpoints exact IDs/arguments")
    check(endpoint["incremental_request_ns"]==3936917768366 and endpoint["incremental_response_ns"]==3936918642621 and endpoint["full_request_ns"]==3936918715122 and endpoint["full_response_ns"]==3936919744572 and endpoint["latest_raw_operation_finished_ns"]==3600873316382,"original endpoint actual retained times")
    check(endpoint["incremental_request_ns"]>binding["common_monotonic_origin_interval_ns"][1]+endpoint["latest_raw_operation_finished_ns"] and endpoint["incremental_request_ns"]>endpoint["latest_bound_read_response_ns"] and endpoint["full_request_ns"]>=endpoint["incremental_response_ns"] and 14400 not in ids,"endpoint strictly after original work/9600 bound calls")
    check(e["original_workload_samples"]==3601 and e["mapping_entries"]==9600 and e["original_statistics_executed_here"] is False and e["product_or_Cargo_executed"] is False,"offline-intake execution scope")
    check((BASE/"complete-safe-report.json").read_bytes()==raw and (BASE/"g4-safe-report-11596847982.zip").read_bytes()==zraw,"preserved original report/archive unchanged")
    outcome.update(status="accepted_scoped_author_complete_report_reception",report_bytes=len(raw),report_sha256=sha(raw),archive_bytes=len(zraw),archive_sha256=sha(zraw),controller_commit=INTAKE,original_receiver=CONTROL,measured_source=G4,original_outer_members=len(members),original_nested_members=total_inner,original_total_members=len(members)+total_inner,selected_members=len(selected),outer_selected_bytes=sum(map(len,selected.values())),nested_selected_seal_bytes=sum(x["members"][n]["bytes"] for x in nested for n in ("p8-build/seal.json","p8-runtime/seal.json")),total_selected_bytes=e["budgets"]["selected_bytes"],source_inputs=1087,observer_inputs=7,offline_checker_commands=7,original_mixed_cells=4,original_soak_operations=3601,original_compound_reads=2400,original_bound_RPCs=9600,mapping_git_blob=gitblob(mapping_bytes),mapping_sha256=sha(mapping_bytes),statistics=stats,original_intake_wall_seconds=r["wall_seconds"],scope="This author re-read the complete safe report and actual safe-report ZIP CRC/SHA, verified every recorded inventory/selected byte/identity and all available original command, source, build, statistics and endpoint predicates. The large 233,905,529-byte original outer and five raw ZIPs were fully SHA/CRC/stream-checked by actual published c87 intake; they were not downloaded or rerun here. Raw endpoint response Values and full seal bodies remain original-archive evidence, verified by that fixed intake; this report does not pretend to regenerate unavailable raw members. Independent acceptance is separate. Exactly five signed-URL-bearing original logs remain explicitly withheld and required for a standalone full checker rerun.",failure_history_preserved=["author-review v1 selected budget denominator error; original review SHA 4aad7a606376da690cb78d08b64188b6f9b969be3be0a3143157df16fc45a9c6 retained","29ed original endpoint receiver failure","93ce schema-adapter KeyError","three prior complete job-log Transport closed observations"])
except Exception as error:
    outcome.update(error_type=type(error).__name__,safe_error=str(error) if isinstance(error,ValueError) else "typed author reception failure")
    raise
finally:
    outcome["completed_checks"]=len(checks)
    outcome["check_labels_sha256"]=sha(canonical(checks))
    outcome["finished_at"]=datetime.datetime.now(datetime.timezone.utc).isoformat()
    (OUT/"check-labels.json").write_bytes(canonical(checks))
    (OUT/"review.json").write_bytes(canonical(outcome))
    print(json.dumps({"status":outcome["status"],"report_path":str(OUT/"review.json"),"report_sha256":sha(canonical(outcome)),"report_bytes":len(canonical(outcome)),"script_sha256":sha((OUT/"verify_author_reception.py").read_bytes()),"completed_checks":len(checks)},sort_keys=True))
