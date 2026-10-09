"""Author offline replay of retained PR173 recovery evidence; never runs a product."""
from pathlib import Path
from contextlib import closing
import collections
import hashlib
import json
import re
import sqlite3
import subprocess
import sys

HERE = Path(__file__).resolve().parent
RAW = HERE.parent / "evidence" / "recovery" / "11597639035" / "p8-full-recovery"
REPO = HERE.parent / "evidence" / "source"
G = "275e8799d4947d297329073eaa3ca675d3fd0777"
OLD = "277f2490fad3fa30f2812b5547bad033867c9ea5"
sys.path.insert(0, str(REPO / "scripts"))
import p7_build_identity as identity
import p7_fault_lifecycle_stdio
import p8_recovery
import p8_rollback
import p8_cold_build

def load(path):
    return json.loads(Path(path).read_text())

def sha(path):
    return identity.file_sha256(path)

def git(*args, data=None):
    return subprocess.check_output(["git", "-C", str(REPO), *args], input=data)

MEMBERS = load(HERE / "member-hashes.json")["members"]
HASHES = {name.removeprefix("p8-full-recovery/"): row["sha256"]
          for name, row in MEMBERS.items() if name.startswith("p8-full-recovery/")}
FULL = load(RAW / "full-recovery.json")
REPORT = {"source_commit": G, "run_id": 37890757129, "job_id": 113690916709,
          "artifact_id": load(HERE / "github-metadata.json")["artifact"]["id"], "reviewer": "scale_engineering", "review_role": "author_reception", "checks": {}}
CHECKS = REPORT["checks"]

def same(condition, message):
    if not condition:
        raise AssertionError(message)

def blob_manifest(commit):
    rows = git("ls-tree", "-rz", commit, "--", "Cargo.toml", "Cargo.lock", "crates").split(b"\0")
    blobs = {}
    for row in rows:
        if not row: continue
        meta, name = row.split(b"\t", 1)
        mode, kind, oid = meta.decode().split()
        same(kind == "blob" and mode in ("100644", "100755"), "nonregular source")
        blobs[name.decode()] = oid
    oids = list(dict.fromkeys(blobs.values()))
    raw = git("cat-file", "--batch", data=("\n".join(oids) + "\n").encode())
    at = 0
    hashes = {}
    for oid in oids:
        end = raw.index(b"\n", at)
        actual, kind, size = raw[at:end].decode().split()
        same((actual, kind) == (oid, "blob"), "batch identity")
        at = end + 1
        hashes[oid] = hashlib.sha256(raw[at:at + int(size)]).hexdigest()
        at += int(size)
        same(raw[at:at + 1] == b"\n", "batch boundary")
        at += 1
    same(at == len(raw), "batch trailing bytes")
    return {name: hashes[oid] for name, oid in sorted(blobs.items())}

def sealed_record(path):
    record = load(RAW / path)
    prefix = Path(path).parent
    for name, expected in record.get("evidence_files_sha256", {}).items():
        same(HASHES[(prefix / name).as_posix()] == expected, "nested seal: " + path + ":" + name)
    return record

def command(home, subdir, expected):
    record = load(home / subdir / "command.json")
    same(record == expected and record["status"] == "passed" and record["exit_code"] == 0,
         "original command does not pass")
    for name, expected_sha in record["logs_sha256"].items():
        same(sha(home / subdir / name) == expected_sha, "command log bytes")
    return [json.loads(line) for line in (home / subdir / "stdout.log").read_text().splitlines() if line.strip()] if subdir == "build" else record

same(git("rev-parse", "HEAD").decode().strip() == G, "review checkout changed")
SOURCE_BEFORE = identity.source_snapshot(REPO)
source_manifests = {commit: blob_manifest(commit) for commit in (G, OLD)}
same(SOURCE_BEFORE["inputs"] == source_manifests[G] == FULL["source"]["inputs"], "full product source manifest")
same(SOURCE_BEFORE == FULL["source"], "source receipt differs from exact G")
same(FULL["status"] == "passed_declared_fault_matrix" and FULL["schema_version"] == 2,
     "full matrix did not pass formal wrapper")
same(FULL["evidence_files_sha256"] == {name: value for name, value in HASHES.items() if name != "full-recovery.json"},
     "full exact evidence inventory/seal differs")
same(FULL["observer_before"] == FULL["observer_after"], "observer changed")
observed = p8_cold_build.observer_snapshot(REPO, "full")
same(dict(FULL["observer_before"], source_root=str(REPO)) == observed, "observer blobs or loaded source differ")
p8_cold_build.verify_observer_archive(RAW, FULL["observer_before"])
for name, value in FULL["runner_files_sha256"].items():
    same(sha(RAW / "runner-source" / name) == value == sha(REPO / "scripts" / name), "runner source differs")
CHECKS["binding"] = dict(full_sealed_members=len(FULL["evidence_files_sha256"]), source_inputs=len(source_manifests[G]),
                          historical_inputs=len(source_manifests[OLD]), observer_inputs=len(observed["inputs"]),
                          source_manifest_sha256=SOURCE_BEFORE["manifest_sha256"], observer_manifest_sha256=observed["manifest_sha256"])

products = []
for package, folder, commit, features in [("default", "product-default", G, []),
        ("semantic", "product-semantic", G, ["semantic"]),
        ("semantic-http", "product-semantic-http", G, ["semantic", "semantic-http"]),
        ("default", "product-previous", OLD, [])]:
    home = RAW / folder
    receipt = load(home / "build-receipt.json")
    source = receipt["source_before"]
    same(source == receipt["source_after"] and source["source_commit"] == commit, "product source before/after")
    same(load(home / "source-inputs.json") == source_manifests[commit], "complete source map")
    same(sha(home / "source-inputs.json") == source["manifest_sha256"], "source manifest digest")
    same(source["input_count"] == len(source_manifests[commit]), "source count")
    same(source["source_tree"] == git("rev-parse", commit + "^{tree}").decode().strip(), "source tree")
    rows = command(home, "build", receipt["build_observation"])
    artifact = receipt["cargo_artifact"]
    chosen = [row for row in rows if row.get("reason") == "compiler-artifact" and row.get("target", {}).get("name") == "codecortex" and row.get("executable")]
    same(chosen == [artifact] and rows[-1] == {"reason": "build-finished", "success": True}, "Cargo exact product and finish")
    same(receipt["status"] == "passed" and receipt["build_exit_code"] == 0 and receipt["stop_reason"] is None, "product build failure")
    expected = ["/home/runner/.cargo/bin/cargo", "build", "-p", "cc-server", "--bin", "codecortex", "--locked", "--offline", "--no-default-features", "--message-format=json-render-diagnostics"]
    if package != "default": expected += ["--features", package]
    same(receipt["build_command"] == receipt["build_observation"]["command"] == expected, "Cargo invocation")
    same(artifact["manifest_path"] == source["source_root"] + "/crates/cc-server/Cargo.toml", "Cargo manifest origin")
    same(artifact["target"]["src_path"] == source["source_root"] + "/crates/cc-server/src/main.rs", "Cargo source origin")
    same(artifact["target"]["kind"] == ["bin"] and sorted(artifact["features"]) == features, "target/features")
    same(artifact["executable"] == receipt["target_directory"] + "/debug/codecortex" == receipt["copy_source"]["path"], "original executable copy path")
    member = MEMBERS["p8-full-recovery/" + folder + "/codecortex"]
    same(member["sha256"] == receipt["copy_source"]["sha256"] == receipt["binary_sha256"] and member["bytes"] == receipt["copy_source"]["bytes"] == receipt["binary_bytes"], "retained binary copy bytes")
    same(receipt["build_profile"] == "dev" and artifact["profile"]["opt_level"] == "0" and artifact["profile"]["debug_assertions"] is True and artifact["profile"]["test"] is False, "actual dev profile")
    products.append(dict(folder=folder, source_commit=commit, bytes=member["bytes"], sha256=member["sha256"], profile=artifact["profile"], wall_seconds=receipt["build_observation"]["wall_seconds"]))
CHECKS["product_builds"] = products

faults = []
same(len(FULL["executions"]) == 7, "original seven fault suites")
for number, original in enumerate(FULL["executions"]):
    home = RAW / f"fault-test-{number:02}"
    receipt = load(home / "test-receipt.json")
    same(receipt == original and receipt["source"] == SOURCE_BEFORE, "fault receipt/source")
    rows = command(home, "build", receipt["build"])
    artifact = receipt["cargo_artifact"]
    same(artifact in rows and rows[-1] == {"reason": "build-finished", "success": True}, "fault Cargo artifact")
    same(artifact["profile"]["test"] is True and artifact["manifest_path"] == "/home/runner/work/codecortex/codecortex/crates/" + receipt["package"] + "/Cargo.toml", "fault manifest")
    same(artifact["executable"].startswith("/home/runner/work/_temp/p8-full-recovery/cargo-target/debug/deps/"), "fault target")
    same(HASHES[f"fault-test-{number:02}/fault-test"] == receipt["executable_sha256"], "fault executable hash")
    execution = command(home, "execution", receipt["execution"])
    same(execution["command"] == [receipt["retained_executable"], "--exact", receipt["test_name"], "--nocapture", "--test-threads=1"], "original exact test command")
    text = (home / "execution/stdout.log").read_text()
    same(re.search(r"test result: ok\. 1 passed; 0 failed; 0 ignored;", text) is not None and receipt["test_name"] in text, "actual original test result")
    faults.append(dict(test=receipt["test_name"], executable_sha256=receipt["executable_sha256"], exit=execution["exit_code"]))
CHECKS["original_fault_tests"] = faults

for name, seeds, points in [("p7-crash-raw", [17, 29, 43], ["uncommitted", "claimed", "artifact", "published"]),
                           ("independent-crash-raw", [101, 211, 307], ["uncommitted", "claimed", "response", "artifact", "published"])]:
    rows = load(RAW / name / "results.json")
    same({(x["seed"], x["point"]) for x in rows} == {(s, p) for s in seeds for p in points} and len(rows) == len(seeds) * len(points), "complete crash matrix")
    for row in rows:
        same(row["signal"] == 9 and row["integrity"] == "ok", "actual SIGKILL/integrity")
        if row["point"] == "uncommitted": same(row["transaction_rollback"] is True, "transaction rollback")
        else:
            same(row["no_op_replay_rounds"] == 3 and row["final_epoch"] == 1, "no-op replay result")
            same(row["reopen_provider_calls"] == (1 if row["point"] in ("claimed", "response") else 0), "bounded replay calls")
    CHECKS[name] = dict(cases=len(rows), seeds=seeds, points=points, results_sha256=sha(RAW / name / "results.json"))

for path in ("local-recovery/recovery.json", "rollback/rollback.json", "actual-version-pair/version-pair.json"):
    sealed_record(path)
CHECKS["nested_receipt_seals"] = True

def rpc(path):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    requests = [r["payload"] for r in rows if r["event"] == "request"]
    responses = [r["payload"] for r in rows if r["event"] == "response"]
    same(len({r["id"] for r in requests}) == len(requests), "duplicate request")
    same(len({r["id"] for r in responses}) == len(responses), "duplicate response")
    same({r["id"] for r in responses} <= {r["id"] for r in requests}, "unmatched response")
    return requests, {r["id"]: r for r in responses}

pair = sealed_record("actual-version-pair/version-pair.json")
same(pair == FULL["actual_source_version_pair"] and pair["production_schema_versions"] == {"current": 25, "previous": 24}, "actual schema pair")
same(pair["schema_fault_injection"] is False and pair["source_unchanged"] is True and pair["release_certified"] is False, "pair classification")
same([c["id"] for c in pair["cases"]] == ["actual_previous_binary_rebuilds_newer_schema", "current_binary_restored_after_actual_downgrade", "actual_current_database_backup_restore"] and all(c["status"] == "passed" for c in pair["cases"]), "pair cases")
same(pair["cases"][0]["before"]["database"]["schema_version"] == 25 and pair["cases"][0]["after"]["database"]["schema_version"] == 24, "old actual downgrade")
for label, folder, version in [("current", "current-product", 25), ("previous", "previous-product", 24)]:
    product = pair["products"][label]
    same(product["source"]["source_commit"] == (G if label == "current" else OLD), "pair source revision")
    same(HASHES[f"actual-version-pair/{folder}/codecortex"] == product["binary_sha256"], "pair retained exact binary")
    schema_source = git("show", product["source"]["source_commit"] + ":crates/cc-db/src/index_migrate.rs").decode()
    same(re.search(r"SCHEMA_VERSION[^=]*=\s*" + str(version) + r"\s*;", schema_source) is not None, "original source schema")

pair_sessions=[]
for number, label, version in [(1,"01-current-build",25),(2,"02-previous-opens-current",24),(3,"03-current-restored",25),(4,"04-original-backup-restored",25)]:
    home = RAW / "actual-version-pair" / label
    process = load(home / "process.json")
    same(process["exit_code"] == process["expected_exit_code"] == 0 and process["cleanup"] == "completed" and process["tool_count"] == 14, "pair product EOF/tools")
    requests,responses = rpc(home / "rpc.jsonl")
    same(set(responses) == {r["id"] for r in requests}, "pair RPC complete")
    local=load(home/"local.json")
    for request in requests:
        response=responses[request["id"]]
        same("error" not in response,"pair public RPC error")
        if request["method"]=="tools/call":
            name=request["params"]["name"]
            value=response["result"]["structuredContent"]
            value=value.get("result",value)
            same(value==local[{"index":"index","status":"capabilities","search":"search"}[name]],"derived local differs from raw RPC")
    if number in (2,3):same("index schema version mismatch, rebuild required" in (home/"product-stderr.log").read_text(),"mismatch explicit")
    pair_sessions.append(dict(session=label,expected_schema=version,rpc_requests=len(requests),exit=0))
CHECKS["actual_version_pair"] = dict(sessions=pair_sessions, schema_sequence=[25,24,25,25], original_backup_restore=True, source_unchanged=True, released_version_pair=False)

database_checks=[]
for relative, expected in [("actual-version-pair/backups/current-original.sqlite3",25),("actual-version-pair/backups/previous-rebuilt.sqlite3",24),("actual-version-pair/backups/current-rebuilt.sqlite3",25),("actual-version-pair/fixture/.codecortex/index.sqlite3",25),("rollback/backups/index-old.sqlite3",25),("rollback/backups/index-future.sqlite3",1025),("rollback/backups/index-rebuilt.sqlite3",25),("rollback/fixture/.codecortex/index.sqlite3",25)]:
    path=RAW/relative
    wal=path.with_name(path.name+"-wal")
    same(not wal.exists() or wal.stat().st_size==0,"cannot immutable-inspect uncheckpointed WAL")
    before=sha(path)
    with closing(sqlite3.connect(path.as_uri()+"?mode=ro&immutable=1",uri=True)) as db:
        schema=db.execute("PRAGMA user_version").fetchone()[0]
        integrity=db.execute("PRAGMA integrity_check").fetchone()[0]
        fk=db.execute("PRAGMA foreign_key_check").fetchall()
        files=db.execute("SELECT COUNT(*) FROM files").fetchone()[0]
    same(schema==expected and integrity=="ok" and not fk and files==2 and sha(path)==before,"retained SQLite schema/integrity")
    database_checks.append(dict(path=relative,schema=schema,integrity=integrity,indexed_files=files,sha256=before))
CHECKS["retained_databases"] = database_checks

active=[]
for seed in (223,227,229):
    home=RAW/f"active-stdio-{seed}";r=load(home/"active-faults.json")
    same(r in FULL["active_stdio"] and r["status"]=="passed" and r["seed"]==seed,"active receipt")
    same(r["binary_sha256"]==r["binary_sha256_after"]==HASHES["product-semantic-http/codecortex"],"active exact product")
    cases={c["id"]:c for c in r["cases"]};same(len(cases)==4 and all(c["status"]=="passed" for c in cases.values()),"active cases")
    disconnect=cases["active_provider_network_disconnect"]
    same(disconnect["before"]["semantic_state"]=="backfilling" and disconnect["before"]["dense_state"]=="partial" and disconnect["after"]["semantic_state"]=="ready","no false ready")
    corrupt=cases["active_semantic_cache_corruption"]
    lanes=corrupt["corrupt_query"]["evidence_summary"]["retrieval"]["lanes"]
    lane=next(x for x in lanes if x["lane_id"]=="semantic")
    same(lane["status"]=="partial" and lane["coverage"]["complete"] is False and lane["truncation_reason"]=="semantic_artifact_unavailable" and corrupt["restored_lane"]["status"]=="complete" and corrupt["additional_provider_requests"]==0,"corruption truthful query scope")
    unknown=cases["active_cache_reader_rejects_unknown_format"]
    same(unknown["injected_format"]==999 and unknown["rejected_lane"]["status"]=="partial" and unknown["rejected_lane"]["coverage"]["complete"] is False and unknown["restored_lane"]["status"]=="complete","unknown format rejected")
    swap=cases["concurrent_database_replacement"]
    same(swap["held"]["query_pins"]>0 and swap["held"]["generation"]["incarnation"]!=swap["swapped"]["generation"]["incarnation"] and swap["swapped"]["semantic_state"]!="ready" and swap["settled"]["semantic_state"]=="ready" and len(swap["persisted_manifest"])==1 and swap["deleted_source_absent"] is True,"held swap fencing/deletion")
    events=[json.loads(line) for line in (home/"events.jsonl").read_text().splitlines()]
    same([e["sequence"] for e in events]==list(range(len(events))),"active sequence")
    counts=collections.Counter(e["kind"] for e in events)
    same(counts["http_request"]==r["observed_provider_requests"]==len(r["provider_calls"])==6 and counts["http_disconnect"]==1,"actual HTTP denominator")
    same(all(call["actual_paid_cost"] is None for call in r["provider_calls"]) and r["paid_cost"] is None,"fake provider costs must remain unknown")
    for exit in r["product_exits"]:
        same(exit["exit_code"]==0 and exit["stdout_reader_joined"] is True and not exit["stdout_errors"],"active EOF")
        session=exit["session"]
        same(sha(home/(session+".stdout.log"))==exit["stdout_sha256"] and sha(home/(session+".stderr.log"))==exit["stderr_sha256"],"active wire logs")
        stdout=[json.loads(line) for line in (home/(session+".stdout.log")).read_text().splitlines()]
        recorded=[e["response"] for e in events if e["kind"]=="mcp_response" and e["session"]==session]
        same(stdout==recorded,"active entire stdout equals recorded responses")
    active.append(dict(seed=seed,cases=list(cases),events=len(events),event_counts=dict(counts),provider_requests=6,paid_cost=None))
CHECKS["active_faults"] = active

local=sealed_record("local-recovery/recovery.json")
same(local==FULL["local_recovery"] and local["counts"]==dict(cancelled=0,failed=0,not_run=4,passed=3),"local explicit scope")
for case in local["cases"][:3]:
    same(case==load(RAW/"local-recovery"/case["id"]/"receipt.json") and case["status"]=="passed","local raw receipt")
delete=next(c for c in local["cases"] if c["id"]=="deleted_source")["result"]
same(delete["deleted_source_recreated"] is False and delete["after_reopen"]["explicit_index_called"] is False and delete["after_reopen"]["deleted_query"]["response"]==[],"deleted no repair/recreation")
CHECKS["local_scope"]={"passed":[c["id"] for c in local["cases"] if c["status"]=="passed"],"not_run_in_local_profile":[c["id"] for c in local["cases"] if c["status"]=="not_run"],"full_matrix_supplements_local_not_run":True}
CHECKS["source_after_equal"] = identity.source_snapshot(REPO)==SOURCE_BEFORE
same(CHECKS["source_after_equal"],"review source changed")
same({name:sha(RAW.parent/name) for name,row in MEMBERS.items() if row["extracted_nonbinary"]}=={name:row["sha256"] for name,row in MEMBERS.items() if row["extracted_nonbinary"]},"extracted originals changed during review")
REPORT.update(status="accepted_scoped_raw_evidence", task_closure=False, TODO_closed=0,
    limitations=["Actual retained products use dev profile; cannot replace G8 full release-profile validation.",
                 "Actual unmodified public source revision 277/schema24 is not a tagged release; original task text does not require a tagged release pair.",
                 "The actual pair sequence is G25 -> old24 -> G25 -> original G25 backup; 24 is established from the old binary source and actual rebuild, not a fabricated preceding run.",
                 "Paid-provider cost remains null; all enabled network faults use owned loopback fake provider. No paid/live or physical-device/power-loss claim.",
                 "P8-011 dependencies P7-020/P8-007/P8-010 and P8-016 dependencies P7-020/P8-012/P8-013 remain separate; old-functional full CI and G8 are not certified by this artifact.",
                 "No product or Cargo was executed by this author reception; large binaries were streamed from retained ZIP for exact SHA and size, not omitted." ])
(HERE/"author-review.json").write_text(json.dumps(REPORT,sort_keys=True,indent=2)+"\n")
print(json.dumps({"status":REPORT["status"],"checks":list(CHECKS),"report_sha256":sha(HERE/"author-review.json")},indent=2))
