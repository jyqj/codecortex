#!/usr/bin/env python3
"""P8 mixed/soak observations through one actual product stdio process.

Owned fixtures only. The fixed offered schedule, all terminal outcomes, native
self measurements, real Git switches and exact no-repair oracle are retained.
This driver never changes roadmap status or grants release approval.
"""
import argparse
from collections import Counter
from concurrent.futures import CancelledError, ThreadPoolExecutor, wait
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import resource
import shutil
import subprocess
import sys
import threading
import time

from p8_cold_build import digest, new_directory, write_json
from p8_rollback import Product, require
from p8_runtime_build import OBSERVER_FILES, artifact_inventory, seal_output, verify_output, verify_receipt


MARKER = "p8-runtime-owned-fixture-v1\n"
QUERY = "p8_runtime_stable_signal"
COMPACTION_EVENT = "resolver catalog dropped for tombstone compaction"
MAX_RAW_BYTES = 512 * 1024 * 1024
CACHE_COUNTERS = ("graph_hits", "graph_misses", "result_hits", "result_misses")
SOAK_READ_PROTOCOL = "symbol_plus_local_hybrid_cache_v1"
SOAK_READ_ROLES = ("before_status", "symbol", "hybrid", "after_status")


class Raw:
    def __init__(self, path):
        self.stream = Path(path).open("xb")
        self.lock = threading.Lock()
        self.size = 0

    def emit(self, kind, **data):
        row = dict(kind=kind, **data)
        encoded = (json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n").encode()
        with self.lock:
            if self.size + len(encoded) > MAX_RAW_BYTES:
                raise RuntimeError("runtime raw evidence budget exhausted")
            self.stream.write(encoded)
            self.stream.flush()
            self.size += len(encoded)

    def close(self):
        self.stream.close()


def owned(root):
    require(root.is_dir() and not root.is_symlink()
            and (root / ".p8-owned").read_text() == MARKER,
            "operation requires the exact owned runtime fixture")


def git(root, *args):
    owned(root)
    command = ["git", "-c", "core.hooksPath=/dev/null", "-C", str(root), *args]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    require(result.returncode == 0, "owned git failed: " + result.stderr)
    return dict(argv=command, stdout=result.stdout, stderr=result.stderr, exit_code=0)


def hot_source(generation):
    # Constant source size / symbol count; no ever-growing comment accumulation.
    return "".join(f"def churn_{generation % 1000:03}_{i:03}():\n    return {i}\n" for i in range(128))


def make_fixture(root, files):
    root.mkdir()
    (root / ".p8-owned").write_text(MARKER)
    (root / ".gitignore").write_text(".codecortex/\n.codecortex.json\n.p8-owned\n")
    (root / ".codecortex.json").write_text(json.dumps({
        "auto_index": {"enabled": False}, "semantic": {"enabled": False},
        "indexing": {"max_concurrent_parse": 2},
    }) + "\n")
    (root / "stable.py").write_text(f"def {QUERY}():\n    return 7\n")
    (root / "branch.py").write_text("def branch_signal():\n    return 1\n")
    (root / "churn.py").write_text(hot_source(0))
    for i in range(files - 3):
        (root / f"module_{i:05}.py").write_text(f"def stable_{i:05}():\n    return {i}\n")
    git(root, "init", "-b", "p8-a")
    git(root, "add", "--", ".gitignore", "*.py")
    git(root, "-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid",
        "commit", "-m", "fixed branch a")
    git(root, "switch", "-c", "p8-b")
    (root / "branch.py").write_text("def branch_signal():\n    return 2\n")
    git(root, "add", "--", "branch.py")
    git(root, "-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid",
        "commit", "-m", "fixed branch b")
    git(root, "switch", "p8-a")


def mutate(root, number):
    owned(root)
    action = number % 6
    if action == 0:
        (root / "churn.py").write_text(hot_source(number + 1))
        return dict(action="bounded_symbol_churn", generation=number + 1, symbols=128)
    if action == 1:
        (root / "temporary.py").write_text("def p8_temporary():\n    return 3\n")
        return dict(action="add", path="temporary.py")
    if action == 2:
        (root / "temporary.py").rename(root / "renamed.py")
        return dict(action="rename", before="temporary.py", after="renamed.py")
    if action == 3:
        (root / "renamed.py").unlink()
        return dict(action="delete", path="renamed.py")
    if action == 4:
        # Force only this task's tracked fixture files back to a frozen branch.
        # Neither user source nor another working tree can reach this path.
        branch = "p8-b" if (number // 6) % 2 == 0 else "p8-a"
        result = git(root, "switch", "--discard-changes", branch)
        result.update(action="real_git_branch_switch", branch=branch,
                      commit=git(root, "rev-parse", "HEAD")["stdout"].strip())
        return result
    (root / "churn.py").write_text(hot_source(0))
    return dict(action="restore_api", path="churn.py")


def diagnostics(product):
    value = product.tool("status", {"aspect": "index"}, timeout=30)
    return value.get("diagnostics", value)


def execution_environment():
    cpu_model = None
    try:
        cpu_model = next((line.partition(":")[2].strip()
                          for line in Path("/proc/cpuinfo").read_text().splitlines()
                          if line.startswith("model name")), None)
    except OSError:
        pass
    try:
        affinity = len(os.sched_getaffinity(0))
    except (AttributeError, OSError):
        affinity = None
    return dict(system=platform.system(), release=platform.release(), machine=platform.machine(),
                cpu_model=cpu_model, reported_logical_cpus=os.cpu_count(),
                runner_affinity_cpus=affinity, cgroup_cpu_quota=None,
                cgroup_cpu_quota_status="not_observed; logical or affinity counts are not a quota claim")


def require_stable_symbol(response):
    """A query echo or arbitrary diagnostic text is not a public symbol hit."""
    require(isinstance(response, list) and any(
        isinstance(hit, dict) and hit.get("name") == QUERY
        and hit.get("file_path") == "stable.py" for hit in response),
        "exact stable.py public symbol missing")


def cache_identity(state):
    """Require native process and complete public generation, not an epoch guess."""
    diagnostics = state.get("diagnostics", {})
    process = diagnostics.get("process_resources", {})
    retrieval = diagnostics.get("retrieval", {})
    generation = retrieval.get("generation")
    freshness = state.get("resolution_freshness", {})
    require(type(process.get("pid")) is int and process["pid"] > 0,
            "cache observation lacks native server PID")
    require(isinstance(generation, dict)
            and type(generation.get("index_epoch")) is int
            and type(generation.get("evidence_epoch")) is int
            and isinstance(generation.get("incarnation"), list)
            and len(generation["incarnation"]) == 16
            and all(type(n) is int and 0 <= n <= 255 for n in generation["incarnation"])
            and freshness.get("complete") is True
            and freshness.get("status") == "ready"
            and freshness == retrieval.get("resolution_freshness")
            and freshness.get("index_epoch") == generation["index_epoch"],
            "cache observation lacks complete generation")
    return dict(pid=process["pid"], generation=generation)


def cache_lookup(before, after):
    require(cache_identity(before) == cache_identity(after),
            "server or generation changed inside cache read")
    old = before.get("diagnostics", {}).get("search_cache", {})
    new = after.get("diagnostics", {}).get("search_cache", {})
    delta = {}
    for key in CACHE_COUNTERS:
        require(type(old.get(key)) is int and type(new.get(key)) is int
                and 0 <= old[key] <= new[key], "cache counter missing/reset")
        delta[key] = new[key] - old[key]
    graph = delta["graph_hits"], delta["graph_misses"]
    result = delta["result_hits"], delta["result_misses"]
    if graph == (1, 0) and result == (0, 0):
        return dict(state="hit", owner="graph_result_cache", delta=delta)
    if graph == (0, 1) and result in ((0, 0), (0, 1), (1, 0)):
        return dict(state="miss", owner="graph_result_cache", delta=delta)
    if graph == (0, 0) and result in ((1, 0), (0, 1)):
        return dict(state="hit" if result[0] else "miss", owner="result_cache", delta=delta)
    raise ValueError("cache counters do not isolate exactly one hybrid lookup")


def require_stable_hybrid(response, project):
    """Verify the retained hit against the current owned fixture's exact bytes."""
    owned(project)
    source = (project / "stable.py").read_bytes()
    expected = f"def {QUERY}():\n    return 7\n".encode()
    require(source == expected, "owned stable.py changed")
    require(isinstance(response, dict) and response.get("_truncated") is not True,
            "hybrid response missing/truncated")
    require(response.get("evidence_summary", {}).get("packing", {}).get("partial") is False,
            "hybrid packing completeness missing")
    hits = response.get("machine_pack", {}).get("hits")
    require(isinstance(hits, list) and len(hits) == 1, "hybrid top_k=1 needs one machine hit")
    hit = hits[0]
    require(isinstance(hit, dict) and hit.get("file_path") == "stable.py"
            and hit.get("symbol_name") == QUERY and type(hit.get("start_line")) is int
            and type(hit.get("end_line")) is int and hit["start_line"] == 1
            and hit["end_line"] == 2 and isinstance(hit.get("text"), str),
            "hybrid exact stable.py entity/span missing")
    metadata = hit.get("metadata", {})
    fresh = metadata.get("source_freshness", {})
    proof = metadata.get("source_evidence", {})
    span = proof.get("span", {})
    require(fresh.get("status") == "current_verified" and fresh.get("disk_checked") is True,
            "hybrid hit is not currently source verified")
    require(type(span.get("start")) is int and type(span.get("end")) is int
            and span["start"] == 0 and span["end"] in (len(source), len(source) - 1)
            and proof.get("source", {}).get("byte_len") == len(source)
            and proof.get("source", {}).get("encoding") == "utf8"
            and source[span["start"]:span["end"]] == hit["text"].encode(),
            "hybrid returned source bytes/span differ from current fixture")
    return dict(file_path="stable.py", symbol_name=QUERY, source_bytes=len(source),
                source_sha256=hashlib.sha256(source).hexdigest(), span=span,
                method="current owned source bytes and exact entity/span; no digest self-report accepted as byte proof")


def validate_cache_probe(probe, previous, project):
    require(probe.get("protocol") == SOAK_READ_PROTOCOL, "unknown soak cache read protocol")
    calls = probe.get("requests", [])
    require([call.get("role") for call in calls] == list(SOAK_READ_ROLES)
            and all(call.get("status") == "success" for call in calls),
            "cache read lacks all four successful original requests")
    wanted = (("status", {"aspect": "index"}),
              ("search", {"query": QUERY, "mode": "symbol", "top_k": 5}),
              ("search", {"query": QUERY, "mode": "hybrid", "top_k": 1, "retrieval_strategy": "local"}),
              ("status", {"aspect": "index"}))
    for call, (name, arguments) in zip(calls, wanted):
        require(call.get("name") == name and call.get("arguments") == arguments
                and type(call.get("started_ns")) is int and type(call.get("finished_ns")) is int
                and call["finished_ns"] >= call["started_ns"], "cache request identity/timing differs")
    require(all(a["finished_ns"] <= b["started_ns"] for a, b in zip(calls, calls[1:])),
            "cache requests overlap or have reversed timing")
    require(isinstance(calls[0].get("response"), dict) and isinstance(calls[3].get("response"), dict),
            "cache status original response missing")
    before, after = calls[0]["response"], calls[3]["response"]
    identity = cache_identity(before)
    require(type(probe.get("server_pid")) is int and probe["server_pid"] == identity["pid"],
            "cache native PID differs from owned stdio process")
    mutation = probe.get("preceding_mutation")
    if mutation is not None:
        require(mutation.get("index_epoch") == identity["generation"]["index_epoch"],
                "cache generation differs from preceding completed mutation")
    lookup = cache_lookup(before, after)
    if previous is not None:
        require(previous["identity"]["pid"] == identity["pid"], "soak cache server PID changed")
    same = previous is not None and previous["identity"] == identity
    expected = "hit" if same else "miss"
    require(lookup["state"] == expected, "cache lookup differs from actual previous accepted generation")
    require_stable_symbol(calls[1]["response"])
    witness = require_stable_hybrid(calls[2]["response"], project)
    old_pool = before.get("diagnostics", {}).get("query_execution", {})
    new_pool = after.get("diagnostics", {}).get("query_execution", {})
    for key in ("completed", "rejected", "cpu_limit", "async_limit", "queue_limit"):
        require(type(old_pool.get(key)) is int and type(new_pool.get(key)) is int
                and 0 <= old_pool[key] <= new_pool[key], "query pool counter missing/reset")
    require(new_pool["completed"] > old_pool["completed"]
            and new_pool["rejected"] == old_pool["rejected"]
            and all(old_pool[k] == new_pool[k] > 0 for k in ("cpu_limit", "async_limit", "queue_limit")),
            "shared query pool completion/bounds not observed")
    if previous is not None:
        require(old_pool["completed"] >= previous["completed"], "shared query pool completion regressed")
    return dict(identity=identity, lookup=lookup, expected=expected,
                invalidated=previous is not None and not same,
                previous_identity=None if previous is None else previous["identity"],
                completed=new_pool["completed"], completed_delta=new_pool["completed"] - old_pool["completed"],
                source_witness=witness,
                worker_scope="same native server and shared query pool path/counters; not individual OS-thread identity or semantic worker")


def soak_read(product, project, row, previous, preceding_mutation, elapsed):
    """Four explicit RPCs inside ONE offered read, under the soak admission lock."""
    probe = dict(protocol=SOAK_READ_PROTOCOL, server_pid=product.process.pid,
                 preceding_mutation=preceding_mutation, requests=[])
    row["cache_probe"] = probe  # Keep attempted/failed prefix even on an exception.
    requests = (("before_status", "status", {"aspect": "index"}),
                ("symbol", "search", {"query": QUERY, "mode": "symbol", "top_k": 5}),
                ("hybrid", "search", {"query": QUERY, "mode": "hybrid", "top_k": 1, "retrieval_strategy": "local"}),
                ("after_status", "status", {"aspect": "index"}))
    for role, name, arguments in requests:
        call = dict(role=role, name=name, arguments=arguments, started_ns=elapsed(), status="error")
        probe["requests"].append(call)
        try:
            call["response"] = product.tool(name, arguments, timeout=60 if name == "search" else 30)
            call["status"] = "success"
            if role == "symbol": row["response"] = call["response"]
        except Exception as error:
            call["error"] = f"{type(error).__name__}: {error}"
            raise
        finally:
            call["finished_ns"] = elapsed()
    probe["observation"] = validate_cache_probe(probe, previous, project)
    return probe["observation"]


def soak_cache_summary(rows, operations, start_ns, end_ns, project):
    reads = sorted((r for r in rows if r["operation"] == "read"), key=lambda r:r.get("call_started_ns", r["offered_ns"]))
    expected_reads = operations - (operations + 2) // 3
    request_counts = {role: Counter() for role in SOAK_READ_ROLES}
    states, errors, mutation_coverage = Counter(), [], Counter()
    quarters = [dict(reads=0, hits=0, misses=0, invalidations=0, mutations={}) for _ in range(4)]
    builds = sorted((r for r in rows if r["operation"] == "build" and r["status"] == "success"),
                    key=lambda r:r["finished_ns"])
    def quarter_at(at): return min(3, max(0, (at - start_ns) * 4 // max(1, end_ns - start_ns)))
    for row in builds:
        action = row["mutation"]["action"]
        bucket = quarters[quarter_at(row["finished_ns"])]["mutations"]
        bucket[action] = bucket.get(action, 0) + 1
    previous = None
    for row in reads:
        probe = row.get("cache_probe", {})
        for call in probe.get("requests", []):
            if call.get("role") in request_counts: request_counts[call["role"]][call.get("status", "missing")] += 1
        try:
            require(row["status"] == "success", "original read did not succeed")
            preceding = [r for r in builds if r["finished_ns"] <= row["call_started_ns"]]
            last = preceding[-1] if preceding else None
            expected_mutation = None if last is None else dict(
                action=last["mutation"]["action"], ordinal=last["mutation_ordinal"], operation_id=last["id"],
                index_epoch=last["response"]["resolution_freshness"].get("index_epoch"))
            require(probe.get("preceding_mutation") == expected_mutation,
                    "cache preceding mutation differs from original operation timeline")
            observed = validate_cache_probe(probe, previous, project)
            require(probe["requests"][0]["started_ns"] >= row["call_started_ns"]
                    and probe["requests"][-1]["finished_ns"] <= row["finished_ns"],
                    "compound requests escape original operation timing")
            require(observed == probe.get("observation") and row.get("response") == probe["requests"][1]["response"],
                    "cache receipt differs from original requests/symbol response")
            previous = observed
            states[observed["lookup"]["state"]] += 1
            states["invalidations"] += observed["invalidated"]
            quarter = quarter_at(probe["requests"][2]["finished_ns"])
            bucket = quarters[quarter];bucket["reads"] += 1
            bucket["hits" if observed["expected"] == "hit" else "misses"] += 1
            bucket["invalidations"] += observed["invalidated"]
            mutation = probe.get("preceding_mutation") or {}
            action = mutation.get("action", "not_observed")
            mutation_coverage[action] += 1
        except (ValueError, KeyError, TypeError, AttributeError, OSError) as error:
            errors.append(dict(id=row["id"], error=str(error)))
    complete = len(reads) == expected_reads and not errors
    demonstrated = all(q["hits"] > 0 and q["misses"] > 0 and q["invalidations"] > 0 for q in quarters)
    actual_counts = {role:dict(counts) for role, counts in request_counts.items()}
    statuses = request_counts["before_status"] + request_counts["after_status"]
    return dict(protocol=SOAK_READ_PROTOCOL, passed=complete and demonstrated,
                expected_offered_reads=expected_reads, recorded_offered_reads=len(reads),
                validated_reads=states["hit"] + states["miss"], observed=dict(states),
                request_counts=actual_counts, status_request_counts=dict(statuses),
                expected_requests_per_role=expected_reads, errors=errors,
                time_quarters=quarters, preceding_mutation_coverage=dict(mutation_coverage),
                quarter_time_basis="actual hybrid RPC completion and actual mutation operation completion; never offered time",
                quarter_mutation_scope="successful completed builds; every failure remains in original outcomes/statistics",
                temporal_cache_coverage=demonstrated,
                denominator="one offered compound read; symbol+hybrid+two status RPCs and all their time/failures included; no extra latency samples",
                comparison_scope="not directly comparable to the previous symbol-only read protocol",
                isolation="soak read/build share original write admission lock; wait remains measured; mixed unchanged")


def median(values):
    values = sorted(values)
    require(bool(values), "median requires observations")
    return values[len(values) // 2]


def rss_trend(samples):
    values = [s.get("server", {}).get("resident_bytes") for s in samples]
    valid = [v for v in values if isinstance(v, int) and not isinstance(v, bool) and v > 0]
    if len(valid) != len(values) or len(valid) < 8:
        return dict(status="unavailable", observed=len(values), valid=len(valid),
                    passed=False, reason="positive native server RSS required for every sample")
    quarter = len(valid) // 4
    warmed, tail = median(valid[quarter:2 * quarter]), median(valid[3 * quarter:])
    allowed = warmed + warmed // 4 + 32 * 1024 * 1024
    return dict(status="observed", observed=len(values), warmed_median_bytes=warmed,
                tail_median_bytes=tail, allowed_bytes=allowed, sampled_peak_bytes=max(valid),
                passed=tail <= allowed, rule="existing_soak_warmed_plus_25_percent_plus_32MiB")


def sample_coverage(samples, start_ns, end_ns, maximum_gap_ns=5_000_000_000):
    """Require native observations throughout the measured work, including edges."""
    times = [row.get("at_ns") for row in samples]
    if not times or any(not isinstance(t, int) or isinstance(t, bool) for t in times):
        return dict(passed=False, reason="missing sample timestamps")
    if times != sorted(set(times)) or start_ns >= end_ns:
        return dict(passed=False, reason="invalid observation time order")
    gaps = [max(0, times[0] - start_ns), max(0, end_ns - times[-1])]
    gaps.extend(b - a for a, b in zip(times, times[1:]))
    return dict(passed=max(gaps) <= maximum_gap_ns,
                observed_samples=len(times), first_ns=times[0], last_ns=times[-1],
                work_start_ns=start_ns, work_end_ns=end_ns,
                maximum_gap_ns=max(gaps), allowed_maximum_gap_ns=maximum_gap_ns,
                method="monotonic sample starts including work start and end gaps")


def latency_summary(rows):
    values = sorted(r["finished_ns"] - r["offered_ns"] for r in rows
                    if r.get("finished_ns") is not None)
    def quantile(p):
        return values[max(0, math.ceil(len(values) * p) - 1)] if values else None
    return dict(n=len(values), p50_ns=quantile(.5), p95_ns=quantile(.95),
                p99_ns=quantile(.99), maximum_ns=max(values) if values else None,
                scope="all_offered_terminal_outcomes_including_rejections_and_failures",
                tail_stability_claim=False)


def verify_build(root, binary, receipt_path, oracle=None, statistics=None):
    receipt = json.loads(receipt_path.read_text())
    binaries = {"codecortex": binary,
                "p8-oracle": oracle or Path(receipt["oracle_path"]),
                "p8-runtime-statistics": statistics or Path(receipt["statistics_path"])}
    verified = verify_receipt(root, receipt_path, binaries)
    return dict(binary_path=str(binary), binary_sha256=digest(binary), package_kind="default",
                build_identity=verified)


def replay_statistics(statistics, output):
    """The Rust owner sees the original rows; Python does not choose a sample subset."""
    commands = []
    for name in ("statistics.json", "statistics-replay.json"):
        command = [str(statistics), "--plan", str(output / "plan.json"),
                   "--raw", str(output / "raw.jsonl"), "--output", str(output / name)]
        with (output / (name + ".stdout")).open("xb") as stdout, (output / (name + ".stderr")).open("xb") as stderr:
            result = subprocess.run(command, stdout=stdout, stderr=stderr, timeout=120)
        commands.append(dict(argv=command, exit_code=result.returncode))
        write_json(output / "statistics-execution.json", commands)
        require(result.returncode in (0, 1), "runtime statistics could not validate the original plan/raw")
    require((output / "statistics.json").read_bytes() == (output / "statistics-replay.json").read_bytes()
            and commands[0]["exit_code"] == commands[1]["exit_code"],
            "runtime statistics replay is not byte-identical")
    report = json.loads((output / "statistics.json").read_text())
    require(report.get("plan_sha256") == digest(output / "plan.json")
            and report.get("raw_sha256") == digest(output / "raw.jsonl")
            and report.get("replay_binary_sha256") == digest(statistics)
            and report.get("exit_code") == commands[0]["exit_code"],
            "statistics output does not bind the exact original inputs and executable")
    return dict(path="statistics.json", sha256=digest(output / "statistics.json"),
                replay_path="statistics-replay.json", replay_identical=True,
                exit_code=commands[0]["exit_code"], status=report.get("observation_status"))


def run(args):
    require(args.concurrency in (1, 4, 8, 16), "C must be 1/4/8/16")
    require(60 <= args.operations <= 10000 and 8 <= args.files <= 10000,
            "operation/file bounds exceeded")
    require(0 < args.interval_ms <= 60000, "invalid fixed offered interval")
    if args.profile == "soak":
        require((args.operations - 1) * args.interval_ms >= 3_600_000,
                "long soak requires at least one hour of actual offered work")
    root = Path(__file__).resolve().parents[1]
    binary, oracle = args.binary.resolve(strict=True), args.oracle.resolve(strict=True)
    receipt_path = args.build_receipt.resolve(strict=True)
    build_receipt = json.loads(receipt_path.read_text())
    statistics = Path(getattr(args, "statistics", None) or build_receipt["statistics_path"]).resolve(strict=True)
    identity = verify_build(root, binary, receipt_path, oracle, statistics)
    binaries = {"codecortex": binary, "p8-oracle": oracle, "p8-runtime-statistics": statistics}
    out = new_directory(args.output)
    plan = dict(schema_version=1, profile=args.profile, concurrency=args.concurrency,
                files=args.files, operations=args.operations, offer_interval_ms=args.interval_ms,
                queue_capacity=128, request_timeout_seconds=60, resource_interval_seconds=1,
                source=json.loads(args.build_receipt.read_text())["source_before"],
                product_sha256=digest(binary), oracle_sha256=digest(oracle),
                local_semantic_state="disabled", paid_provider_requests=0,
                rss_rule="existing_soak_warmed_plus_25_percent_plus_32MiB")
    plan["offer_schedule"] = "uniform" if args.profile == "soak" else "fixed_concurrency_sized_bursts"
    plan["mutation_schedule"] = "six-action cycle in serialized write-admission order"
    plan["observer_concurrency_scope"] = "one separate status RPC; excluded from workload C and latency"
    if args.profile == "soak":
        reads = args.operations - (args.operations + 2) // 3
        plan["read_protocol"] = dict(
            name=SOAK_READ_PROTOCOL, offered_read_operations=reads,
            planned_request_counts=dict(symbol=reads, hybrid=reads, before_status=reads, after_status=reads),
            request_roles=list(SOAK_READ_ROLES), strategy="local",
            rpc_timeout_seconds=dict(before_status=30, symbol=60, hybrid=60, after_status=30),
            rpc_timeout_sum_seconds=180,
            timeout_scope="180 is the four sequential RPC timeout sum, excludes admission wait, and is not a performance SLA; outer request_timeout_seconds=60 is the maximum single-RPC timeout",
            admission="read/build share original write lock only in soak; lock wait remains in operation timing",
            denominator="one offered compound read with every RPC and validation included; no added samples",
            comparison_scope="not directly comparable to prior symbol-only reads")
    plan["execution_environment"] = execution_environment()
    plan["statistics_sha256"] = digest(statistics)
    plan["build_identity"] = identity["build_identity"]
    proof = out / "build-evidence"
    proof.mkdir()
    proof_names = ["build-receipt.json", "seal.json", "source-before.json", "source-after.json",
                   "product-build.jsonl", "product-build.stderr"]
    proof_names.extend("observer-source/" + relative for relative in OBSERVER_FILES)
    expected_proof = {}
    for name in proof_names:
        original, copied = receipt_path.parent / name, proof / name
        expected_proof[name] = dict(bytes=original.stat().st_size, sha256=digest(original))
        copied.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(original, copied)
    require(artifact_inventory(proof, exclude=None) == expected_proof, "runtime build-evidence copy differs")
    plan["retained_build_evidence"] = expected_proof
    write_json(out / "plan.json", plan)
    raw = Raw(out / "raw.jsonl")
    project, full = out / "project", out / "fresh-full"
    rows, resource_rows, failures, sampler_errors = [], [], [], []
    row_lock, write_lock = threading.Lock(), threading.Lock()
    active = dict(read=0, build=0, maximum=0, read_build_overlap=False)
    mutation_sequence = [0]
    cache_previous, preceding_mutation = [None], [None]
    stop = threading.Event()
    cancel_work = threading.Event()
    product, comparison_product, sampler = None, None, None
    worker_cleanup = dict(status="not_started", cleanup_bound_seconds=70, still_running=0)
    begun = time.monotonic_ns()
    report = dict(schema_version=1, status="running", exit_code=2, task_complete=False)
    def elapsed(): return time.monotonic_ns() - begun
    try:
        make_fixture(project, args.files)
        product = Product(identity, project, out / "product", out / "semantic-cache")
        product.initialize()
        initial = product.tool("index", {"path": str(project), "full": True}, timeout=180)
        raw.emit("initial_build", report=initial)
        require(initial.get("parse_errors") == []
                and initial.get("resolution_freshness", {}).get("complete") is True,
                "initial parse/closure report is not complete")
        def sample():
            while not stop.is_set():
                at = elapsed()
                try:
                    state = diagnostics(product)
                    own = resource.getrusage(resource.RUSAGE_SELF)
                    multiplier = 1 if sys.platform == "darwin" else 1024
                    row = dict(at_ns=at, server=state.get("process_resources", {}),
                               query_execution=state.get("query_execution"),
                               search_cache=state.get("search_cache"),
                               runner=dict(pid=os.getpid(), peak_resident_bytes=own.ru_maxrss * multiplier,
                                           user_cpu_seconds=own.ru_utime, system_cpu_seconds=own.ru_stime,
                                           method="getrusage(RUSAGE_SELF); lifetime high-water"))
                except Exception as error:
                    row = dict(at_ns=at, error=str(error), server={})
                resource_rows.append(row)
                try:
                    raw.emit("resources", **row)
                except Exception as error:
                    sampler_errors.append(str(error))
                    return
                stop.wait(1)
        sampler = threading.Thread(target=sample, name="p8-runtime-self-samples", daemon=True)
        sampler.start()
        slots = threading.BoundedSemaphore(128 + args.concurrency)
        offer_start = time.monotonic_ns()
        def operation(number, scheduled, offered):
            kind = "build" if number % 3 == 0 else "read"
            row = dict(id=number, operation=kind, scheduled_ns=scheduled, offered_ns=offered,
                       started_ns=elapsed(), status="error")
            held_write = False
            try:
                if cancel_work.is_set(): raise CancelledError("drain deadline exceeded")
                if kind == "build" or args.profile == "soak":
                    while not write_lock.acquire(timeout=.1):
                        if cancel_work.is_set(): raise CancelledError("waiting write canceled")
                    held_write = True
                    if cancel_work.is_set(): raise CancelledError("write canceled before mutation")
                if kind == "build":
                    row["mutation_ordinal"] = mutation_sequence[0]
                    row["mutation"] = mutate(project, mutation_sequence[0])
                    mutation_sequence[0] += 1
                row["call_started_ns"] = elapsed()
                with row_lock:
                    active[kind] += 1
                    active["maximum"] = max(active["maximum"], active["read"] + active["build"])
                    active["read_build_overlap"] |= active["read"] > 0 and active["build"] > 0
                try:
                    if kind == "read" and args.profile == "soak":
                        cache_previous[0] = soak_read(product, project, row, cache_previous[0],
                                                      preceding_mutation[0], elapsed)
                        response = row["response"]  # Original symbol response stays Rust-replayable.
                    else:
                        response = product.tool("index", {"path": str(project), "full": False}, timeout=60) if kind == "build" else product.tool(
                            "search", {"query": QUERY, "mode": "symbol", "top_k": 5}, timeout=60)
                finally:
                    with row_lock: active[kind] -= 1
                row["response"] = response
                if kind == "build":
                    require(response.get("parse_errors") == [], "incremental parse errors or missing coverage")
                    require(response.get("resolution_freshness", {}).get("complete") is True,
                            "incremental closure incomplete or not observed")
                    preceding_mutation[0] = dict(action=row["mutation"]["action"],
                                                ordinal=row["mutation_ordinal"], operation_id=number,
                                                index_epoch=response["resolution_freshness"].get("index_epoch"))
                else:
                    require_stable_symbol(response)
                row["status"] = "success"
            except CancelledError as error:
                row.update(status="canceled", error=str(error))
            except Exception as error:
                row["error"] = f"{type(error).__name__}: {error}"
            finally:
                row["finished_ns"] = elapsed()
                if held_write: write_lock.release()
                with row_lock: rows.append(row)
                try:
                    raw.emit("operation", **row)
                finally:
                    slots.release()
        futures = {}
        pool = ThreadPoolExecutor(max_workers=args.concurrency)

        def cleanup_workers(reason):
            # Every exceptional offering path shares the original bounded
            # cancellation/kill/drain policy. Raw retention failure must not
            # interrupt cleanup or allow a worker to write after sealing.
            cancel_work.set()
            stop.set()
            pending = [future for future in futures if not future.done()]
            worker_cleanup.update(status="stopping", reason=reason, pending_at_cleanup=len(pending),
                                  canceled=0, retention_errors=[])
            for future in pending:
                if future.cancel():
                    number, scheduled, offered = futures[future]
                    row = dict(id=number, operation="build" if number % 3 == 0 else "read",
                               scheduled_ns=scheduled, offered_ns=offered, finished_ns=elapsed(),
                               status="canceled", reason=reason + "_before_start")
                    with row_lock: rows.append(row)
                    worker_cleanup["canceled"] += 1
                    try:
                        raw.emit("operation", **row)
                    except Exception as error:
                        worker_cleanup["retention_errors"].append(dict(id=number, error=str(error)))
                    finally:
                        slots.release()
            if pending and product.process.poll() is None:
                try:
                    product.process.kill()
                except ProcessLookupError:
                    pass  # The owned process may have exited between poll and kill.
            wait([future for future in pending if not future.cancelled()], timeout=70)
            still_running = sum(not future.done() for future in futures)
            worker_cleanup.update(status="unconfirmed" if still_running else "completed",
                                  still_running=still_running)

        try:
            for number in range(args.operations):
                tick = number if args.profile == "soak" else number // args.concurrency * args.concurrency
                target = offer_start + tick * args.interval_ms * 1_000_000
                remaining = (target - time.monotonic_ns()) / 1e9
                if remaining > 0: time.sleep(remaining)
                scheduled, offered = target - begun, elapsed()
                if not slots.acquire(blocking=False):
                    row = dict(id=number, operation="build" if number % 3 == 0 else "read",
                               scheduled_ns=scheduled, offered_ns=offered, finished_ns=elapsed(),
                               status="queue_rejected")
                    rows.append(row)
                    raw.emit("operation", **row)
                    continue
                future = pool.submit(operation, number, scheduled, offered)
                futures[future] = (number, scheduled, offered)
            done, pending = wait(futures, timeout=180)
            if pending:
                # A live request has a 60-second RPC deadline. Canceling queued
                # futures prevents a second unbounded queue drain in __exit__.
                cleanup_workers("drain_deadline")
                raw.emit("drain_deadline", pending_at_deadline=len(pending),
                         canceled=worker_cleanup["canceled"],
                         still_running=worker_cleanup["still_running"], cleanup_bound_seconds=70)
                require(not worker_cleanup["still_running"], "owned requests did not stop after kill and RPC deadline")
                raise RuntimeError("owned work failed to drain in 180 seconds; queued work canceled")
            for future in done: future.result()
        finally:
            try:
                if worker_cleanup["status"] == "not_started":
                    if any(not future.done() for future in futures):
                        cleanup_workers("offering_or_worker_failure")
                    else:
                        worker_cleanup.update(status="completed", still_running=0)
            finally:
                # Never join unconfirmed user work without a deadline. Its
                # artifacts stay explicitly unsealed below if it cannot stop.
                worker_cleanup["still_running"] = sum(not future.done() for future in futures)
                if worker_cleanup["still_running"]:
                    worker_cleanup["status"] = "unconfirmed"
                pool.shutdown(wait=not worker_cleanup["still_running"], cancel_futures=True)
        observed_span = time.monotonic_ns() - offer_start
        observed_end = elapsed()
        stop.set()
        sampler.join(timeout=35)
        require(not sampler.is_alive(), "resource sampler failed to drain")
        require(not sampler_errors, "resource raw retention failed: " + repr(sampler_errors))
        endpoint_status = diagnostics(product)
        raw.emit("endpoint_status", response=endpoint_status)
        # This copy happens only after the last incremental write. There is no
        # catch-up build/repair on that side before the independent comparison.
        shutil.copytree(project, full, ignore=shutil.ignore_patterns(".codecortex"))
        comparison_product = Product(identity, full, out / "full-product", out / "full-cache")
        try:
            comparison_product.initialize()
            full_report = comparison_product.tool("index", {"path": str(full), "full": True}, timeout=180)
            raw.emit("full_control", report=full_report)
            require(full_report.get("parse_errors") == []
                    and full_report.get("resolution_freshness", {}).get("complete") is True,
                    "fresh full control parse/closure report incomplete")
            left = product.tool("search", {"query": QUERY, "mode": "symbol", "top_k": 5})
            right = comparison_product.tool("search", {"query": QUERY, "mode": "symbol", "top_k": 5})
            require_stable_symbol(left)
            require_stable_symbol(right)
            raw.emit("endpoint_public", incremental=left, full=right)
            command = [str(oracle), "--left", str(project), "--right", str(full), "--output", str(out / "parity.json")]
            result = subprocess.run(command, capture_output=True, text=True, timeout=300)
            raw.emit("oracle_process", argv=command, exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr)
            require(digest(oracle) == plan["oracle_sha256"] == build_receipt["oracle_sha256"],
                    "oracle executable changed during observation")
            require(result.returncode in (0, 1), "complete no-repair endpoint oracle could not execute")
            if result.returncode == 1:
                failures.append("complete no-repair endpoint oracle failed")
        finally:
            comparison_product.close()
        product.close()
        product = None
        raw.close()
        statistics_result = replay_statistics(statistics, out)
        statuses = {name: sum(r["status"] == name for r in rows) for name in sorted({r["status"] for r in rows})}
        ids = [r["id"] for r in rows]
        require(sorted(ids) == list(range(args.operations)), "offered/terminal denominator differs")
        compactions = (out / "product/product-stderr.log").read_text().count(COMPACTION_EVENT)
        switches = sum(r.get("mutation", {}).get("action") == "real_git_branch_switch" for r in rows)
        trend = rss_trend(resource_rows)
        coverage = sample_coverage(resource_rows, offer_start - begun, observed_end)
        if statuses != {"success": args.operations}: failures.append("non-success offered outcomes")
        require((statistics_result["exit_code"] == 0) == (statuses == {"success": args.operations}),
                "raw statistics and original terminal outcome gate disagree")
        if args.profile == "mixed" and args.concurrency > 1 and not active["read_build_overlap"]:
            failures.append("configured concurrency did not exercise read/build overlap")
        if args.profile == "soak":
            if observed_span < 3_600_000_000_000: failures.append("less than one hour of actual offered workload")
            if not trend["passed"]: failures.append("RSS trend unavailable or failed")
            if not coverage["passed"]: failures.append("RSS samples do not cover the complete work interval")
            if compactions == 0: failures.append("no actual catalog compaction")
            if switches < 2: failures.append("branch switches not exercised")
            cache_reuse = soak_cache_summary(rows, args.operations, offer_start - begun, observed_end, project)
            if not cache_reuse["passed"]: failures.append("complete soak cache invalidation/refill/hit evidence failed")
        report = dict(schema_version=1, status="passed_observation" if not failures else "failed",
                      exit_code=0 if not failures else 1, task_complete=False,
                      profile=args.profile, offered=args.operations, outcomes=statuses,
                      execution_environment=plan["execution_environment"],
                      observed_work_ns=observed_span, actual_concurrency=active,
                      real_branch_switches=switches, observed_catalog_compactions=compactions,
                      rss=trend, resource_time_coverage=coverage,
                      observer_concurrency_scope=plan["observer_concurrency_scope"],
                      latency=latency_summary(rows),
                      latency_by_operation={kind: latency_summary([r for r in rows if r["operation"] == kind])
                                            for kind in ("read", "build")}, failures=failures,
                      statistics=statistics_result,
                      parity_exit_code=result.returncode,
                      parity_sha256=digest(out / "parity.json"), product_sha256=digest(binary),
                      oracle_sha256=digest(oracle), semantic_backfill="not_run_in_default_product_profile",
                      release_approval=False)
        if args.profile == "soak": report["cache_reuse"] = cache_reuse
    except Exception as error:
        report.update(status="failed", exit_code=2, error=f"{type(error).__name__}: {error}",
                      offered_terminal_rows=len(rows), failures=failures)
    finally:
        stop.set()
        if sampler is not None:
            sampler.join(timeout=35)
            if sampler.is_alive(): report.update(status="failed", exit_code=2, sampler_cleanup="timed_out")
        if product is not None:
            try: product.close()
            except Exception as error:
                report.update(status="failed", exit_code=2, cleanup_error=str(error))
        def product_stopped(value):
            return value is None or (value.process.poll() is not None
                                    and not value.transport.reader.is_alive()
                                    and not value.transport.exit_watcher.is_alive())
        cleanup = dict(worker=worker_cleanup, sampler_stopped=sampler is None or not sampler.is_alive(),
                       product_stopped=product_stopped(product),
                       comparison_product_stopped=product_stopped(comparison_product))
        writers_stopped = (worker_cleanup["still_running"] == 0 and cleanup["sampler_stopped"]
                           and cleanup["product_stopped"] and cleanup["comparison_product_stopped"])
        report["owned_cleanup"] = cleanup
        if writers_stopped:
            raw.close()
            if args.profile == "soak" and "cache_reuse" not in report:
                # Preserve actual attempted RPC counts on infrastructure failures,
                # without presenting a partial run as completed planned workload.
                report["cache_reuse"] = soak_cache_summary(rows, args.operations, 0, elapsed(), project)
                report["cache_reuse"]["scope"] = "partial failure; quarter origin unavailable; not certification"
        else:
            report.update(status="failed", exit_code=2, artifact_seal_status="unsealed_owned_writers")
        try:
            final_identity = verify_receipt(root, receipt_path, binaries)
            require(final_identity == identity["build_identity"],
                    "runtime source, observer, copy source or retained artifact changed during observation")
            require(artifact_inventory(proof, exclude=None) == expected_proof,
                    "runtime retained build evidence changed during observation")
            report["final_build_verification"] = final_identity
        except Exception as error:
            report.update(status="failed", exit_code=2,
                          final_build_verification_error=f"{type(error).__name__}: {error}")
        report["raw_sha256"] = digest(out / "raw.jsonl") if writers_stopped else None
        report["plan_sha256"] = digest(out / "plan.json")
        report["artifact_seal"] = "seal.json" if writers_stopped else None
        if writers_stopped: report["artifact_seal_status"] = "sealed"
        report["elapsed_ns"] = elapsed()
        write_json(out / "report.json", report)
        if writers_stopped:
            seal_output(out)
            verify_output(out)
    return report


def main():
    if len(sys.argv) > 1 and sys.argv[1] == "verify":
        parser = argparse.ArgumentParser(description="Verify retained runtime artifacts, including failed observations")
        parser.add_argument("--output", type=Path, required=True)
        parser.add_argument("--build-output", type=Path)
        args = parser.parse_args(sys.argv[2:])
        try:
            seal = verify_output(args.output)
            report = json.loads((args.output / "report.json").read_text())
            plan = json.loads((args.output / "plan.json").read_text())
            require(report.get("raw_sha256") == digest(args.output / "raw.jsonl")
                    and report.get("plan_sha256") == digest(args.output / "plan.json"),
                    "runtime report no longer binds its raw and plan")
            if args.build_output is not None:
                verify_output(args.build_output)
                require(digest(args.build_output / "seal.json") == plan["build_identity"]["build_seal_sha256"],
                        "supplied build archive differs from the observed artifact set")
            print(json.dumps(dict(status="sealed_artifacts_verified", files=len(seal["artifact_inventory"]),
                                  observation_status=report.get("status"), observation_exit_code=report.get("exit_code"))))
            return 0
        except Exception as error:
            print(f"{type(error).__name__}: {error}", file=sys.stderr)
            return 2
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--oracle", type=Path, required=True)
    parser.add_argument("--statistics", type=Path, help="defaults to the exact statistics artifact in the build receipt")
    parser.add_argument("--build-receipt", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--profile", choices=("mixed", "soak"), default="mixed")
    parser.add_argument("--concurrency", type=int, default=4)
    parser.add_argument("--operations", type=int, default=900)
    parser.add_argument("--files", type=int, default=1000)
    parser.add_argument("--interval-ms", type=int, default=50)
    args = parser.parse_args()
    try:
        report = run(args)
        print(json.dumps(report, ensure_ascii=False, indent=2))
        return report["exit_code"]
    except Exception as error:
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
