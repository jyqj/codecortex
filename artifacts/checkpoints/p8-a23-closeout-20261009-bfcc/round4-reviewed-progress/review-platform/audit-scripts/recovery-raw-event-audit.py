import sys
sys.dont_write_bytecode=True
import pathlib,json,hashlib,collections
base=pathlib.Path.cwd()
source=(base/"source").resolve()
sys.path.insert(0,str(source/"scripts"))
from p8_cold_build import digest
from p8_rollback import source_manifest,verify_fixture_search
from p8_recovery import verify_symbol
home=base/"raw"/"11592146562"/"extracted"/"p8-full-recovery"
j=json.loads((home/"full-recovery.json").read_text())
def value(response):
    if "error" in response:
        return {"rpc_error":response["error"]}
    r=response["result"]
    if "structuredContent" in r:
        assert not r.get("isError"), "tool error"
        v=r["structuredContent"]
        return v.get("result",v)
    return r
native=[]
streams={}
binary_by_path={p["binary_path"]:p["binary_sha256"] for p in j["products"].values()}
for process_file in sorted(home.rglob("process.json")):
    out=process_file.parent
    rel=out.relative_to(home).as_posix()
    p=json.loads(process_file.read_text())
    assert p["binary_sha256"]==binary_by_path[p["command"][0]], rel
    assert p["cleanup"]=="completed" and p["initialized"] is True and p["tool_count"]==14,rel
    assert p["exit_code"]==p["expected_exit_code"]==(-9 if rel.endswith("before-kill") else 0),rel
    events=[json.loads(line) for line in (out/"rpc.jsonl").read_text().splitlines()]
    requests=[e["payload"] for e in events if e.get("event")=="request"]
    responses=[e["payload"] for e in events if e.get("event")=="response"]
    wire=[]
    for e in events:
        if "wire_sha256" in e:
            b=e["text"].encode()
            assert len(b)==e["wire_bytes"] and hashlib.sha256(b).hexdigest()==e["wire_sha256"],rel
            wire.append(json.loads(e["text"]))
    assert responses==wire,rel+" wire differs from decoded responses"
    req={r["id"]:r for r in requests}
    res={r["id"]:r for r in responses}
    assert len(req)==len(requests) and len(res)==len(responses),rel+" duplicate id"
    assert set(res)<=set(req),rel
    missing=set(req)-set(res)
    assert missing==({j["local_recovery"]["cases"][0]["result"]["kill"]["pending_request_id"]} if rel.endswith("before-kill") else set()),rel+" unexpected missing response"
    terminal=[e for e in events if e.get("event")=="terminal"]
    assert len([e for e in terminal if e.get("kind")=="stdout_eof"])==1,rel
    exits=[e for e in terminal if e.get("kind")=="process_exit"]
    assert len(exits)==1 and exits[0]["exit_code"]==p["exit_code"],rel
    parsed=[(req[r["id"]],value(r)) for r in responses]
    streams[rel]={"process":p,"events":events,"requests":requests,"responses":responses,"parsed":parsed}
    errors=[r["error"] for r in responses if "error" in r]
    if errors:
        assert rel=="local-recovery/database_busy/lock-window",rel+" unexpected RPC error"
        assert all("locked" in e.get("message","") for e in errors)
    local_file=out/"local.json"
    if local_file.exists() and rel.startswith(("rollback/","actual-version-pair/")):
        local=json.loads(local_file.read_text())
        values={request["params"]["name"]:v for request,v in parsed if request["method"]=="tools/call"}
        assert all(local[k]==values[n] for k,n in [("index","index"),("capabilities","status"),("search","search")]),rel+" local summary not from raw"
        assert values["status"]["capabilities"]["search"] is True
        assert values["status"]["retrieval"]["semantic_state"]=="not_configured" and values["status"]["retrieval"]["dense_state"]=="disabled"
        assert not values["index"]["parse_errors"] and values["index"]["files_scanned"]>0
        check=verify_fixture_search(home/rel.split("/")[0]/"fixture",values["search"])
        if "public_source" in local:
            assert local["public_source"]==check
    native.append({"session":rel,"pid":p["pid"],"exit_code":p["exit_code"],"requests":len(req),"responses":len(res),"expected_pending":sorted(missing),"rpc_errors":len(errors),"wire_rows":len(wire),"rpc_sha256":digest(out/"rpc.jsonl")})
assert len(native)==13 and len({x["pid"] for x in native})==13
local=j["local_recovery"]
assert local==json.loads((home/"local-recovery"/"recovery.json").read_text())
passed=[c for c in local["cases"] if c["status"]=="passed"]
assert [c["id"] for c in passed]==["kill_restart","database_busy","deleted_source"]
local_checks=[]
for c in passed:
    rel="local-recovery/"+c["id"]
    project=home/rel/"fixture"
    result=c["result"]
    assert source_manifest(project)==result["expected_source_after"]
    values=[v for name,s in streams.items() if name.startswith(rel+"/") for q,v in s["parsed"]]
    if c["id"] in ("kill_restart","database_busy"):
        query=result["query"]
        assert query["response"] in values and result["capabilities"] in values
        name="p8_recovery_after_restart" if c["id"]=="kill_restart" else "p8_recovery_after_unlock"
        assert verify_symbol(project,query["response"],name,present=True)==query["check"]
    if c["id"]=="kill_restart":
        k=result["kill"]
        assert k["pid"]==streams[rel+"/before-kill"]["process"]["pid"]
        assert k["exit_code"]==-9 and k["stop_observed"] is True
        assert k["stop_observation_method"]=="waitpid_owned_child_WUNTRACED_SIGSTOP"
        assert k["request"]["event"] in streams[rel+"/before-kill"]["events"]
    elif c["id"]=="database_busy":
        b=result["busy"]
        assert b["lock_probe"]["blocked"] is True and b["lock_probe"]["sqlite_errorcode"]==5
        assert 0<b["held_seconds"]<=b["hold_budget_seconds"]==6 and b["new_symbol_rows_while_locked"]==0
        assert result["incremental"] in values
        assert b["request_while_locked"]["event"]["reason"] in [r["error"] for r in streams[rel+"/lock-window"]["responses"] if "error" in r]
    else:
        assert result["deleted_source_recreated"] is False and not (project/"src/deleted.rs").exists()
        assert result["after_reopen"]["explicit_index_called"] is False
        assert not any(q.get("params",{}).get("name")=="index" for q in streams[rel+"/reopen-without-index"]["requests"])
        for phase in ("after_incremental","after_reopen"):
            v=result[phase]
            assert v["deleted_query"]["response"]==[] and v["deleted_query"]["response"] in values
            assert v["retained_query"]["response"] in values and v["capabilities"] in values
    local_checks.append({"case":c["id"],"status":"accepted_raw_RPC_and_source","started_ns":c["started_ns"],"finished_ns":c["finished_ns"]})
active_checks=[]
assert j["executed_active_seeds"]==[223,227,229] and [a["seed"] for a in j["active_stdio"]]==[223,227,229]
expected_cases=["active_provider_network_disconnect","active_semantic_cache_corruption","active_cache_reader_rejects_unknown_format","concurrent_database_replacement"]
for a in j["active_stdio"]:
    seed=a["seed"]
    out=home/f"active-stdio-{seed}"
    assert a==json.loads((out/"active-faults.json").read_text())
    assert a["status"]=="passed" and a["binary_sha256"]==a["binary_sha256_after"]==j["products"]["semantic-http"]["binary_sha256"]
    assert [c["id"] for c in a["cases"]]==expected_cases and all(c["status"]=="passed" for c in a["cases"])
    events=[json.loads(line) for line in (out/"events.jsonl").read_text().splitlines()]
    assert [e["sequence"] for e in events]==list(range(len(events)))
    requests=[e for e in events if e["kind"]=="mcp_request"]
    responses=[e for e in events if e["kind"]=="mcp_response"]
    req={(e["session"],e["request"]["id"]):e for e in requests}
    res={(e["session"],e["response"]["id"]):e for e in responses}
    assert len(req)==len(requests) and len(res)==len(responses) and set(req)==set(res)
    pairs=[]
    for key,e in res.items():
        request=req[key]
        assert request["sequence"]<e["sequence"] and "error" not in e["response"]
        pairs.append((request["request"],value(e["response"]),e["sequence"]))
    statuses=[v["retrieval"] for q,v,n in pairs if q.get("params",{}).get("name")=="status"]
    searches=[(q,v,n) for q,v,n in pairs if q.get("params",{}).get("name")=="search"]
    sem=[(v,n) for q,v,n in searches if q["params"].get("arguments",{}).get("retrieval_strategy")=="semantic"]
    assert len(sem)==5
    lanes=[]
    for v,n in sem:
        selected=[lane for lane in v["evidence_summary"]["retrieval"]["lanes"] if lane["lane_id"]=="semantic"]
        assert len(selected)==1
        lanes.append(selected[0])
    assert [lane["status"] for lane in lanes]==["complete","partial","complete","partial","complete"]
    assert [lane["coverage"]["complete"] for lane in lanes]==[True,False,True,False,True]
    c0,c1,c2,c3=a["cases"]
    assert c0["before"] in statuses and c0["after"] in statuses
    assert c0["before"]["query_pins"]==0 and c0["before"]["semantic_pending"]>0
    assert c0["before"]["semantic_state"]=="backfilling" and c0["before"]["dense_state"]=="partial"
    assert c0["before"]["dense_published"]<c0["before"]["dense_desired"]
    assert c0["after"]["semantic_state"]=="ready" and c0["after"]["dense_published"]==1 and c0["after"]["query_pins"]==0
    assert c1["corrupt_query"]==sem[1][0] and c1["restored_lane"]==lanes[2]
    assert lanes[1]["truncation_reason"]=="semantic_artifact_unavailable"
    assert c1["original_sha256"]==digest(out/"original-vector.bin") and c1["additional_provider_requests"]==0
    assert c2["rejected_lane"]==lanes[3] and c2["restored_lane"]==lanes[4]
    assert c2["injected_format"]==999 and c2["original_metadata_sha256"]==digest(out/"original-vector.meta.json")
    assert all(c3[k] in statuses for k in ("held","swapped","settled"))
    assert c3["held"]["query_pins"]>0 and c3["held"]["generation"]["incarnation"]!=c3["swapped"]["generation"]["incarnation"]
    assert c3["swapped"]["semantic_state"]!="ready" and c3["settled"]["semantic_state"]=="ready"
    assert c3["settled"]["query_pins"]==0 and c3["settled"]["dense_published"]==1
    snapshots=[e for e in events if e["kind"]=="database_read_only"]
    pending=[e["snapshot"] for e in snapshots if e["phase"]=="disconnect_pending"]
    settled=[e["snapshot"] for e in snapshots if e["phase"]=="replacement_settled"]
    assert len(pending)==len(settled)==1
    assert any(x["state"]=="pending" and x["attempt_count"]>0 for x in pending[0]["outbox"])
    assert c3["persisted_manifest"]==settled[0]["manifest"] and len(c3["persisted_manifest"])==1
    assert c3["current_source_sha256"]==digest(out/"project"/"keep.rs")
    assert (out/"project"/"keep.rs").read_text()==f"pub fn survivor_{seed}() -> u32 {{ {seed+3000} }}\n"
    assert c3["deleted_source_absent"] is True and not (out/"project"/"remove.rs").exists()
    http=[e for e in events if e["kind"]=="http_request"]
    ends=[e for e in events if e["kind"] in ("http_response","http_disconnect")]
    assert len(http)==len(ends)==a["observed_provider_requests"]==len(a["provider_calls"])==6
    assert [e["request_number"] for e in http]==list(range(1,7))
    for call in a["provider_calls"]:
        found=[e for e in ends if e["request_number"]==call["request_number"]]
        assert len(found)==1
        for k,v in call.items():
            if k in found[0]:
                assert found[0][k]==v,(seed,k)
    assert sum(e["kind"]=="http_disconnect" for e in ends)==1
    assert not any(sem[0][1]<e["sequence"]<sem[-1][1] for e in http),"cache replay caused unrecorded network"
    spawns=[e for e in events if e["kind"]=="spawn"]
    exits=[e for e in events if e["kind"]=="product_exit"]
    assert len(spawns)==len(exits)==len(a["product_exits"])==2
    for p in spawns:
        assert p["command"][0]==j["products"]["semantic-http"]["binary_path"]
        session=p["session"]
        stdout=out/(session+".stdout.log")
        raw=[json.loads(line) for line in stdout.read_text().splitlines() if line.strip()]
        assert raw==[e["response"] for e in responses if e["session"]==session]
        exitrow=[x for x in a["product_exits"] if x["session"]==session]
        assert len(exitrow)==1
        x=exitrow[0]
        assert x["exit_code"]==0 and x["cleanup"] is False and x["requested_sigkill"] is False
        assert x["stdout_reader_joined"] is True and x["stdout_errors"]==[]
        assert digest(stdout)==x["stdout_sha256"] and digest(out/(session+".stderr.log"))==x["stderr_sha256"]
        ev=[e for e in exits if e["session"]==session]
        assert len(ev)==1 and all(ev[0][k]==v for k,v in x.items())
    active_checks.append({"seed":seed,"status":"accepted_original_raw_MCP_HTTP_and_lifecycle","mcp_requests":len(req),"mcp_responses":len(res),"provider_requests":len(http),"disconnects":1,"native_clean_exits":2,"semantic_lane_sequence":[lane["status"] for lane in lanes],"event_sha256":digest(out/"events.jsonl")})
result={"schema_version":1,"status":"raw_RPC_HTTP_lifecycle_accepted_database_backup_replay_pending","source_commit":j["source"]["source_commit"],"artifact_id":11592146562,"native_sessions":native,"local_cases":local_checks,"active_seeds":active_checks,"todo_status_change":False}
(base/"review-platform"/"recovery-raw-event-audit.json").write_text(json.dumps(result,indent=2)+"\n")
print(json.dumps({"status":result["status"],"native_sessions":len(native),"native_requests":sum(x["requests"] for x in native),"native_responses":sum(x["responses"] for x in native),"local_cases":local_checks,"active_seeds":active_checks},indent=2))
