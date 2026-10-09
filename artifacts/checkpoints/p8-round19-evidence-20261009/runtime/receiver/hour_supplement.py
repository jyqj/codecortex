#!/usr/bin/env python3
"""Supplement original 275e receipts with full wire-ID/clock/endpoint binding.
No product, Cargo, oracle, provider or new workload is executed.
"""
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
from cache_wire import transport, tool_value, bind_compound_reads_with_endpoint

if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
ROOT = Path(sys.argv[1]).resolve(strict=True)
SOURCE = Path(sys.argv[2]).resolve(strict=True)
AID = 11600492680
G = "275e8799d4947d297329073eaa3ca675d3fd0777"
BASE = ROOT / "raw" / str(AID) / "extracted"
M = BASE / "p8-runtime"
B = BASE / "p8-build"
OUT = ROOT / "wire-supplement"

def require(value, message):
    if not value:
        raise ValueError(message)

def read(path):
    return json.loads(path.read_bytes())

def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def write(path, value):
    with path.open("x") as stream:
        json.dump(value, stream, sort_keys=True, indent=2)
        stream.write("\n")

def canonical(value):
    # Full canonical JSON values, not digest-only equality.
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

def run():
    OUT.mkdir(exist_ok=False)
    replay = read(ROOT / "review-runtime/runtime-11600492680-review-v2.json")
    require(replay["review_status"] == "raw_and_receipt_review_passed"
            and replay["errors"] == [] and replay["artifact_id"] == AID
            and replay["source_sha"] == G and replay["run_id"] == 37890757030,
            "same original 275e base checker failed")
    sys.path.insert(0, str(SOURCE / "scripts"))
    from p7_build_identity import source_snapshot
    from p8_runtime_build import observer_snapshot, verify_output, command_for, TARGETS
    source = source_snapshot(SOURCE)
    observer = observer_snapshot(SOURCE)
    require(source["source_commit"] == G and source["input_count"] == 1089
            and source["manifest_sha256"] == "593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83",
            "fixed original source")
    require(len(observer["files"]) == 9
            and observer["manifest_sha256"] == "ca48939e0647dba2f12e9f26ab0bc88860219ddb304574b6386e5d47b8945064",
            "fixed original nine observers")
    build, plan, report = read(B/"build-receipt.json"), read(M/"plan.json"), read(M/"report.json")
    require(build["source_before"] == build["source_after"] == source
            and build["observer_before"] == build["observer_after"] == observer,
            "retained source/observer receipts")
    require(plan["build_identity"] == report["final_build_verification"],
            "original full before/after execution proof")
    require(build["build_command"] == command_for(Path(build["target_dir"]))
            and build["target_initially_absent"] is True
            and build["build_dir"] == build["target_dir"]
            and build["toolchain_before"] == build["toolchain_after"]
            and plan["build_identity"]["toolchain"] == build["toolchain_before"],
            "original private target and actual toolchain records")
    for name, value in build["toolchain_before"].items():
        require(name in ("cargo", "rustc")
                and Path(value["invocation"]).is_absolute()
                and value["command"] == [value["invocation"], "--version", "--verbose"]
                and Path(value["executable"]).is_absolute()
                and len(value["executable_sha256"]) == 64 and bool(value["version"]),
                "recorded compiler identity fields")
    require(build["compiler_environment"] ==
            {"RUSTC":build["toolchain_before"]["rustc"]["invocation"],
             "RUSTC_WRAPPER":"", "RUSTC_WORKSPACE_WRAPPER":""},
            "recorded compiler invocation overrides")
    messages = [json.loads(line) for line in (B/"product-build.jsonl").open() if line.strip()]
    require(set(build["artifacts"]) == set(TARGETS), "all three actual release roles")
    for name, stored in build["artifacts"].items():
        a = stored["cargo_artifact"]
        package, relative, _, _ = TARGETS[name]
        exact_root = "/home/runner/work/codecortex/codecortex/crates/" + package + "/"
        require(a["manifest_path"] == exact_root+"Cargo.toml"
                and a["target"]["src_path"] == exact_root+relative
                and a["executable"] == build["target_dir"]+"/release/"+name
                and a["executable"] == stored["copy_source"]["path"]
                and a["fresh"] is False and a["target"]["kind"] == ["bin"],
                "original exact Cargo producer and executable-to-copy paths")
        matches = [x for x in messages if x.get("reason") == "compiler-artifact"
                   and x.get("target",{}).get("name") == name
                   and x.get("target",{}).get("kind") == ["bin"]]
        require(matches == [a] and sha(B/name) == stored["binary_sha256"] == stored["copy_source"]["sha256"]
                and (B/name).stat().st_size == stored["binary_bytes"] == stored["copy_source"]["bytes"],
                "unique actual artifact and retained byte copy")
    records = [json.loads(line) for line in (M/"raw.jsonl").open() if line.strip()]
    rows = [row for row in records if row["kind"] == "operation"]
    resources = [row for row in records if row["kind"] == "resources"]
    require(len(rows) == 3601 and sorted(row["id"] for row in rows) == list(range(3601))
            and Counter(row["operation"] for row in rows) == {"read":2400, "build":1201}
            and all(row["status"] == "success" for row in rows), "whole original population")
    sessions, terminal = {}, {}
    for label in ("product", "full-product"):
        with (M/label/"rpc.jsonl").open() as stream:
            requests, responses, times, counts = transport(json.loads(line) for line in stream)
        sessions[label] = (requests, responses, times)
        process = read(M/label/"process.json")
        require(process["exit_code"] == process["expected_exit_code"] == 0
                and process["cleanup"] == "completed"
                and process["binary_sha256"] == build["binary_sha256"], "owned original process ended")
        terminal[label] = {"event_counts":counts, "process":process}
    requests, responses, times = sessions["product"]
    bound = bind_compound_reads_with_endpoint(rows, requests, responses, times, records,
                                              *sessions["full-product"])
    require(bound["compound_reads"] == 2400 and bound["bound_RPCs"] == 9600
            and bound["role_counts"] == {r:2400 for r in ("before_status","symbol","hybrid","after_status")},
            "exact four-role per-read wire binding")
    require(all(row["cache_probe"]["server_pid"] == terminal["product"]["process"]["pid"]
                for row in rows if row["operation"] == "read"), "actual native owner")
    used = {row["rpc_id"] for row in bound["mapping"]}
    status_ids = sorted((i for i,p in requests.items() if p.get("method") == "tools/call"
                         and p.get("params",{}).get("name") == "status"),
                        key=lambda i:times[i]["request"])
    require(len(status_ids) == len(resources)+4801 and len(used.intersection(status_ids)) == 4800
            and all(requests[i]["params"]["arguments"] == {"aspect":"index"} for i in status_ids),
            "all original status RPCs")
    endpoint_rows = [row for row in records if row["kind"] == "endpoint_status"]
    require(len(endpoint_rows) == 1, "one endpoint diagnostics record")
    endpoint_id = status_ids[-1]
    endpoint = tool_value(responses[endpoint_id])
    require(endpoint_id not in used and endpoint.get("diagnostics",endpoint) == endpoint_rows[0]["response"],
            "separate final endpoint diagnostics full value")
    origin_high = bound["common_monotonic_origin_interval_ns"][1]
    require(times[endpoint_id]["request"] > origin_high + max(row["finished_ns"] for row in rows),
            "endpoint diagnostics after all original work")
    leftovers = [i for i in status_ids if i not in used and i != endpoint_id]
    require(len(leftovers) == len(resources), "no unaccounted or reused sampler status")
    project = lambda value:{"server":value.get("process_resources",{}),
                           "query_execution":value.get("query_execution"),
                           "search_cache":value.get("search_cache")}
    for i, raw in zip(leftovers, resources):
        value = tool_value(responses[i])
        value = value.get("diagnostics",value)
        require(project(value) == {"server":raw.get("server",{}),
                "query_execution":raw.get("query_execution"),"search_cache":raw.get("search_cache")},
                "ordered original sampler full projection")
    require(report["owned_cleanup"] == {"unfinished_work":0,"sampler_stopped":True,
            "product_stopped":True,"comparison_product_stopped":True,
            "product_construction_pending":False,"comparison_product_construction_pending":False},
            "original work and all writers drained")
    stats = OUT/"original-p8-runtime-statistics"
    with stats.open("xb") as target:
        target.write((B/"p8-runtime-statistics").read_bytes())
    stats.chmod(0o555)
    require(sha(stats) == build["statistics_sha256"], "actual retained statistics executable")
    stats_inputs = {name:sha(M/name) for name in ("plan.json","raw.jsonl")}
    argv = [str(stats),"--plan",str(M/"plan.json"),"--raw",str(M/"raw.jsonl"),
            "--output",str(OUT/"statistics.json")]
    write(OUT/"statistics-command.json",{"argv":argv,"scope":"offline original retained statistics only"})
    started = time.monotonic()
    with (OUT/"statistics.stdout").open("xb") as stdout, (OUT/"statistics.stderr").open("xb") as stderr:
        done = subprocess.run(argv, stdout=stdout, stderr=stderr, timeout=120,
             env={k:v for k,v in os.environ.items() if k not in ("GH_TOKEN","GITHUB_TOKEN")})
    write(OUT/"statistics-execution.json",{"exit_code":done.returncode,
                                         "wall_seconds":time.monotonic()-started})
    require(done.returncode == 0 and (OUT/"statistics.json").read_bytes() == (M/"statistics.json").read_bytes()
            == (M/"statistics-replay.json").read_bytes(), "retained Rust statistics byte-identical replay")
    require(sha(stats) == build["statistics_sha256"]
            and stats_inputs == {name:sha(M/name) for name in stats_inputs}, "replay inputs unchanged")
    verify_output(B); verify_output(M)
    require(source_snapshot(SOURCE) == source and observer_snapshot(SOURCE) == observer,
            "source/observer after offline replay")
    write(OUT/"cache-wire-binding.json",bound)
    write(OUT/"inspection.json",{"status":"accepted_scoped_275e_soak_supplement",
          "source":G,"artifact_id":AID,"terminal":terminal,"compound_reads":2400,"bound_RPCs":9600,
          "bound_status_probes":4800,"sampler_status_responses":len(leftovers),
          "endpoint_status_rpc_id":endpoint_id,"cache_wire_sha256":sha(OUT/"cache-wire-binding.json"),
          "same_original_275e_base_checker_passed":True,
          "statistics_replay_byte_identical":True,
          "scope":"Full original wire/value/clock and endpoint supplement; no new product workload.",
          "TODO_closed":0,"TODO_remaining":29})
    print(json.dumps({"status":"accepted_scoped_275e_soak_supplement",
                      "inspection_sha256":sha(OUT/"inspection.json")},sort_keys=True))

if __name__ == "__main__":
    run()
