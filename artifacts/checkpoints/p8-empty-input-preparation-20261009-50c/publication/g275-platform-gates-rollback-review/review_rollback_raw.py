#!/usr/bin/env python3
"""Read G275 original rollback/fault evidence; execute no product or build.

Only SQLite copies are opened. Pure validators are imported from the fixed
G275 observer; raw originals, products, source and ledger are never changed.
"""
import argparse
from collections import Counter
from contextlib import closing
import hashlib
import json
from pathlib import Path
import re
import shutil
import sqlite3
import subprocess
import sys

sys.dont_write_bytecode = True
A = Path(__file__).resolve().parent
G = "275e8799d4947d297329073eaa3ca675d3fd0777"
PREVIOUS = "277f2490fad3fa30f2812b5547bad033867c9ea5"
S = A / "g275-source"
R = A / "extracted/g275-full-recovery/p8-full-recovery"
sys.path.insert(0, str(S / "scripts"))
from p8_rollback import (db_snapshot, file_manifest, source_manifest,
                         validate_rebuild, verify_fixture_search)
from p8_recovery import (ADDED, DELETED, MARKER, UNLOCKED,
                         classify_killed_request, verify_symbol)


def check(value, message):
    if not value:
        raise AssertionError(message)


def load(path):
    return json.loads(Path(path).read_text())


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def rel(path):
    return Path(path).relative_to(R).as_posix()


def rows(path):
    return [json.loads(line) for line in Path(path).read_text().splitlines() if line.strip()]


def decoded(response):
    check("error" not in response, "unexpected MCP error")
    value = response["result"]
    check(value.get("isError") is not True, "unexpected MCP tool error")
    value = value.get("structuredContent", value)
    return value.get("result", value)


def regular_rpc(directory, binary_sha, expected_exit=0, required_count=None):
    process = load(directory / "process.json")
    check(process["binary_sha256"] == binary_sha, "process/build identity drift")
    check(process["exit_code"] == process["expected_exit_code"] == expected_exit,
          "unexpected product exit")
    check(process["cleanup"] == "completed" and process["initialized"] is True
          and process["tool_count"] == 14, "original cleanup/initialization failed")
    event_rows = rows(directory / "rpc.jsonl")
    requests = [e["payload"] for e in event_rows if e.get("event") == "request"]
    responses = [e["payload"] for e in event_rows if e.get("event") == "response"]
    req = {v["id"]: v for v in requests}
    resp = {v["id"]: v for v in responses}
    check(len(req) == len(requests) and len(resp) == len(responses), "duplicate RPC id")
    check(set(resp) <= set(req), "unmatched response")
    if required_count is not None:
        check(len(req) == len(resp) == required_count, "incomplete rollback RPC sequence")
    check(requests[0]["method"] == "initialize" and requests[1]["method"] == "tools/list",
          "initial MCP sequence differs")
    check(len(resp[requests[1]["id"]]["result"]["tools"]) == 14, "raw tool surface differs")
    values = []
    for item in requests:
        response = resp.get(item["id"])
        if item["method"] == "tools/call" and response and "error" not in response:
            values.append((item["params"]["name"], decoded(response), item["params"]["arguments"]))
    return values, dict(path=rel(directory), rpc_sha256=digest(directory / "rpc.jsonl"),
                       process_sha256=digest(directory / "process.json"),
                       binary_sha256=binary_sha, requests=len(req), responses=len(resp),
                       exit_code=expected_exit, tool_count=14), event_rows


def local_smoke(directory, fixture, binary_sha, state=None):
    values, record, _ = regular_rpc(directory, binary_sha, required_count=5)
    check([name for name, _, _ in values] == ["index", "status", "search"], "wrong local tool order")
    local = load(directory / "local.json")
    for name, key in [("index", "index"), ("status", "capabilities"), ("search", "search")]:
        check(next(v for n, v, _ in values if n == name) == local[key], "local summary differs from RPC")
    check(local["index"].get("parse_errors") == [], "parse errors or missing record")
    capability = local["capabilities"]
    check(capability["capabilities"]["search"] is True, "local search unavailable")
    check(capability["retrieval"]["semantic_state"] == "not_configured"
          and capability["retrieval"]["dense_state"] == "disabled", "false disabled status")
    public = verify_fixture_search(fixture, local["search"])
    if "public_source" in local:
        check(public == local["public_source"], "public source receipt mismatch")
    count = (directory / "product-stderr.log").read_text().count(
        "index schema version mismatch, rebuild required")
    if state:
        check(local == state["local"] and digest(directory / "product-stderr.log") == state["stderr_sha256"],
              "version state differs from originals")
        check(count == state["schema_mismatch_diagnostics"], "schema diagnostics summary drift")
    record.update(local_sha256=digest(directory / "local.json"), public_source=public,
                  schema_mismatch_diagnostics=count, semantic_state="not_configured", dense_state="disabled")
    return record


def copy_database(path, output):
    destination = output / rel(path)
    destination.parent.mkdir(parents=True, exist_ok=True)
    for suffix in ["", "-wal", "-shm"]:
        original = Path(str(path) + suffix)
        if original.exists():
            shutil.copy2(original, Path(str(destination) + suffix))
    return destination


def check_backup(directory, item, output):
    original = directory / "backups" / Path(item["path"]).name
    check(digest(original) == item["sha256"], "original backup digest mismatch")
    snapshot = db_snapshot(copy_database(original, output))
    check(snapshot == item["snapshot"], "original backup/SQLite snapshot mismatch")
    return dict(path=rel(original), sha256=digest(original), snapshot=snapshot)


def active_case(seed, binary_sha, output):
    directory = R / f"active-stdio-{seed}"
    report = load(directory / "active-faults.json")
    check(report["status"] == "passed" and report["seed"] == seed, "active control not passed")
    check(report["binary_sha256"] == report["binary_sha256_after"] == binary_sha, "active binary drift")
    cases = {c["id"]: c for c in report["cases"]}
    check(len(cases) == 4 and all(c["status"] == "passed" for c in cases.values()), "missing active controls")
    events = rows(directory / "events.jsonl")
    check([e["sequence"] for e in events] == list(range(len(events))), "event ordering/gap")
    requests = {(e["session"], e["request"]["id"]): e for e in events if e["kind"] == "mcp_request"}
    responses = {(e["session"], e["response"]["id"]): e for e in events if e["kind"] == "mcp_response"}
    check(set(requests) == set(responses) and len(requests) == sum(e["kind"] == "mcp_request" for e in events)
          and len(responses) == sum(e["kind"] == "mcp_response" for e in events), "active unmatched/duplicate MCP")
    values, semantic = [], []
    for key, request in requests.items():
        response = responses[key]
        check(request["sequence"] < response["sequence"], "response precedes request")
        value = decoded(response["response"])
        req = request["request"]
        if req["method"] == "tools/call":
            values.append(value)
            args = req["params"]["arguments"]
            if req["params"]["name"] == "search" and args.get("retrieval_strategy") == "semantic":
                lanes = [l for l in value["evidence_summary"]["retrieval"]["lanes"] if l["lane_id"] == "semantic"]
                check(len(lanes) == 1, "semantic lane missing/duplicated")
                semantic.append((value, lanes[0]))
    check([lane["status"] for _, lane in semantic] == ["complete", "partial", "complete", "partial", "complete"],
          "active positive/negative/restored sequence differs")
    corrupt = cases["active_semantic_cache_corruption"]
    unknown = cases["active_cache_reader_rejects_unknown_format"]
    check(corrupt["corrupt_query"] == semantic[1][0] and corrupt["restored_lane"] == semantic[2][1],
          "corrupt cache summary differs from raw MCP")
    check(semantic[1][1]["coverage"]["complete"] is False
          and semantic[1][1]["truncation_reason"] == "semantic_artifact_unavailable", "corrupt cache falsely complete")
    check(corrupt["additional_provider_requests"] == 0
          and digest(directory / "original-vector.bin") == corrupt["original_sha256"], "cache control bytes/cost drift")
    check(unknown["injected_format"] == 999 and unknown["rejected_lane"] == semantic[3][1]
          and unknown["restored_lane"] == semantic[4][1]
          and semantic[3][1]["coverage"]["complete"] is False, "unknown format control mismatch")
    check(digest(directory / "original-vector.meta.json") == unknown["original_metadata_sha256"]
          and load(directory / "original-vector.meta.json")["format_version"] != 999, "original format witness mismatch")
    disconnect = cases["active_provider_network_disconnect"]
    statuses = [v["retrieval"] for v in values if isinstance(v, dict) and "retrieval" in v]
    check(disconnect["before"] in statuses and disconnect["after"] in statuses, "disconnect statuses not in raw MCP")
    before = disconnect["before"]
    check(before["query_pins"] == 0 and before["semantic_pending"] > 0 and before["semantic_state"] == "backfilling"
          and before["dense_state"] == "partial" and before["dense_published"] < before["dense_desired"], "disconnect false ready")
    check(disconnect["after"]["semantic_state"] == "ready", "disconnect recovery not ready")
    network = [e for e in events if e["kind"] == "http_disconnect"]
    check(len(network) == 1 and network[0]["response_bytes_sent"] == 0, "no actual pre-response disconnect")
    db_events = {e["phase"]: e["snapshot"] for e in events if e["kind"] == "database_read_only"}
    check(any(r["state"] == "pending" and r["attempt_count"] > 0 for r in db_events["disconnect_pending"]["outbox"]),
          "disconnect has no persisted retryable work")
    calls = report["provider_calls"]
    check(len(calls) == report["observed_provider_requests"] == 6 and report["paid_cost"] is None, "provider scope/count drift")
    for call in calls:
        kind = "http_request" if call["mode"] == "disconnect" else "http_response"
        event = next(e for e in events if e["kind"] == kind and e["request_number"] == call["request_number"])
        check(call == {k: event[k] for k in call}, "provider receipt not raw HTTP event")
        check(call["actual_paid_cost"] is None, "synthetic cost relabeled")
    replacement = cases["concurrent_database_replacement"]
    check(all(replacement[k] in statuses for k in ["held", "swapped", "settled"]), "replacement status missing raw support")
    check(replacement["held"]["query_pins"] > 0
          and replacement["held"]["generation"]["incarnation"] != replacement["swapped"]["generation"]["incarnation"]
          and replacement["swapped"]["semantic_state"] != "ready"
          and replacement["settled"]["semantic_state"] == "ready", "replacement barrier/ready mismatch")
    check(replacement["persisted_manifest"] == db_events["replacement_settled"]["manifest"]
          and len(replacement["persisted_manifest"]) == 1, "replacement persisted summary drift")
    project = directory / "project"
    check(digest(project / "keep.rs") == replacement["current_source_sha256"]
          and not (project / "remove.rs").exists() and replacement["deleted_source_absent"] is True,
          "deleted/current source integrity mismatch")
    database = copy_database(project / ".codecortex/index.sqlite3", output)
    state = db_snapshot(database)
    check(state["integrity"] == "ok" and state["foreign_key_errors"] == 0, "active SQLite corruption")
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as connection:
        connection.row_factory = sqlite3.Row
        manifest = [dict(r) for r in connection.execute("SELECT * FROM semantic_manifest ORDER BY doc_key")]
        invalid = connection.execute("SELECT COUNT(*) FROM semantic_manifest s LEFT JOIN document_manifest d "
                                     "ON d.doc_key=s.doc_key AND d.doc_version=s.doc_version WHERE d.doc_key IS NULL").fetchone()[0]
    check(manifest == replacement["persisted_manifest"] and invalid == 0, "stale manifest resurrected after reopen")
    exits = []
    for receipt in report["product_exits"]:
        event = next(e for e in events if e["kind"] == "product_exit" and e["session"] == receipt["session"])
        check(receipt == {k: event[k] for k in receipt}, "exit receipt/raw event mismatch")
        check(receipt["exit_code"] == 0 and receipt["cleanup"] is False and receipt["requested_sigkill"] is False
              and receipt["stdout_reader_joined"] is True and receipt["stdout_errors"] == [], "active cleanup failed")
        for stream in ["stdout", "stderr"]:
            check(digest(directory / f'{receipt["session"]}.{stream}.log') == receipt[f"{stream}_sha256"], "active stream hash drift")
        exits.append(dict(session=receipt["session"], exit_code=0, forced_cleanup=False))
    check(len(exits) == 2, "reopen process missing")
    return dict(seed=seed, status="accepted_original_active_controls", report_sha256=digest(directory / "active-faults.json"),
                events_sha256=digest(directory / "events.jsonl"), raw_counts=dict(Counter(e["kind"] for e in events)),
                semantic_lane_sequence=[l["status"] for _, l in semantic], original_vector_sha256=corrupt["original_sha256"],
                original_metadata_sha256=unknown["original_metadata_sha256"], injected_unknown_format=999,
                provider_requests=len(calls), actual_paid_cost=None, additional_requests_for_corrupt_control=0,
                recovery=disconnect["recovery"], sqlite=state, stale_manifest_rows=invalid,
                deleted_source_absent=True, product_exits=exits,
                scope="original owned-loopback execution; transient writes supported by bound observer and raw query controls; no new product run")


def local_cases(binary_sha, output):
    directory = R / "local-recovery"
    report = load(directory / "recovery.json")
    check(report["counts"] == dict(passed=3, failed=0, not_run=4, cancelled=0)
          and report["complete_P8_011"] is False, "local scope was relabeled")
    result = []
    for case in report["cases"]:
        if case["status"] == "not_run":
            continue
        name, data = case["id"], case["result"]
        case_dir, project = directory / name, directory / name / "fixture"
        check(load(case_dir / "receipt.json") == case, "local embedded receipt mismatch")
        check(source_manifest(project) == data["expected_source_after"], "authored source changed")
        database = copy_database(project / ".codecortex/index.sqlite3", output)
        state = db_snapshot(database)
        check(state["integrity"] == "ok" and state["foreign_key_errors"] == 0, "local database corrupt")
        all_values, processes = [], []
        for receipt_path in sorted(case_dir.glob("*/process.json")):
            process = load(receipt_path)
            expected_exit = -9 if receipt_path.parent.name == "before-kill" else 0
            values, record, _ = regular_rpc(receipt_path.parent, binary_sha, expected_exit)
            all_values.extend(v for _, v, _ in values)
            processes.append(record)
        journal = rows(case_dir / "stages.jsonl")
        if name == "kill_restart":
            check(load(case_dir / "kill-observation.json") == data["kill"], "kill observation mismatch")
            classify_killed_request(data["kill"])
            check(any(e.get("kind") == "SIGSTOP" for e in journal) and any(e.get("kind") == "SIGKILL" for e in journal), "kill journal missing")
            check(data["query"]["response"] in all_values and data["capabilities"] in all_values, "recovery not in raw RPC")
            check(verify_symbol(project, data["query"]["response"], ADDED, present=True) == data["query"]["check"], "restart source span mismatch")
            check(state == data["database"], "restart persisted DB differs")
            observation = dict(original_exit_code=-9, pending_request_failed=True, stop_observed=True,
                               injection_scope=data["stage"])
        elif name == "database_busy":
            busy = data["busy"]
            check(load(case_dir / "busy-observation.json") == busy and busy["lock_probe"]["sqlite_errorcode"] == 5
                  and busy["lock_probe"]["blocked"] is True and busy["new_symbol_rows_while_locked"] == 0,
                  "writer lock witness missing")
            check(busy["request_while_locked"]["kind"] == "error" and "locked" in busy["request_while_locked"]["message"], "busy update falsely passed")
            check(data["incremental"] in all_values and data["query"]["response"] in all_values, "unlock recovery missing raw RPC")
            check(verify_symbol(project, data["query"]["response"], UNLOCKED, present=True) == data["query"]["check"], "unlock span mismatch")
            check(state == data["database"], "unlock DB snapshot mismatch")
            observation = dict(sqlite_errorcode=5, held_seconds=busy["held_seconds"], new_symbol_rows_while_locked=0,
                               recovered_by_bounded_incremental_retry=True, scope=busy["scope"])
        else:
            check(name == "deleted_source" and data["deleted_source_recreated"] is False
                  and not (project / "src/deleted.rs").exists(), "deleted source recreated")
            for phase in ["after_incremental", "after_reopen"]:
                item = data[phase]
                for key, symbol, present in [("deleted_query", DELETED, False), ("retained_query", MARKER, True)]:
                    check(item[key]["response"] in all_values, "deletion query missing raw response")
                    check(verify_symbol(project, item[key]["response"], symbol, present=present,
                                        deleted_path="src/deleted.rs" if not present else None) == item[key]["check"], "deletion query mismatch")
            check(data["after_reopen"]["explicit_index_called"] is False and state == data["after_reopen"]["database"], "reopen scope mismatch")
            reopened, _, _ = regular_rpc(case_dir / "reopen-without-index", binary_sha)
            check(all(n != "index" for n, _, _ in reopened), "reopen unexpectedly reindexed")
            with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as connection:
                check(connection.execute("SELECT COUNT(*) FROM symbols WHERE name=?", (DELETED,)).fetchone()[0] == 0, "deleted symbol resurrected")
            observation = dict(deleted_source_absent=True, deleted_symbol_rows=0,
                               explicit_index_on_reopen=False)
        result.append(dict(id=name, status="accepted_original_local_control", receipt_sha256=digest(case_dir / "receipt.json"),
                           stages_sha256=digest(case_dir / "stages.jsonl"), observation=observation,
                           database=state, processes=processes))
    return dict(status="accepted_local_submatrix_with_original_limits", original_counts=report["counts"],
                original_complete_P8_011=False, cases=result,
                original_not_run_preserved=[c for c in report["cases"] if c["status"] == "not_run"])


def main(output):
    check(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=S, text=True).strip() == G, "observer checkout is not G275")
    output.mkdir(parents=True, exist_ok=False)
    original_hashes = file_manifest(R)
    integrity = load(A / "raw-integrity-review.json")
    check(integrity["status"] == "passed_raw_byte_and_source_binding", "prior whole inventory audit missing")
    full = load(R / "full-recovery.json")
    check(full["status"] == "passed_declared_fault_matrix" and full["release_certified"] is False,
          "full-matrix declared scope differs")
    products = full["products"]
    pair_dir = R / "actual-version-pair"
    pair = load(pair_dir / "version-pair.json")
    check(pair["status"] == "passed_actual_source_version_pair" and pair["schema_fault_injection"] is False
          and pair["released_version_pair"] is False and pair["release_certified"] is False, "pair scope drift")
    versions = {}
    schema_sources = {}
    for label, commit in [("current", G), ("previous", PREVIOUS)]:
        product = pair["products"][label]
        check(product["source"]["source_commit"] == commit, "version pair source mismatch")
        path = "crates/cc-db/src/index_migrate.rs"
        content = subprocess.check_output(["git", "show", f"{commit}:{path}"], cwd=S)
        sha = hashlib.sha256(content).hexdigest()
        manifest = load(pair_dir / f"{label}-product/source-inputs.json")
        check(manifest[path] == sha, "schema not product-bound")
        matches = re.findall(rb"^pub const CURRENT_SCHEMA_VERSION: u32 = ([0-9]+);$", content, re.MULTILINE)
        check(len(matches) == 1, "schema contract missing")
        versions[label] = int(matches[0])
        schema_sources[label] = dict(source=commit, path=path, sha256=sha, schema_version=versions[label],
                                    source_input_count=product["source"]["input_count"], binary_sha256=product["binary_sha256"])
    check(versions == pair["production_schema_versions"] == {"current": 25, "previous": 24}, "real schema pair differs")
    pc = {c["id"]: c for c in pair["cases"]}
    downgrade = pc["actual_previous_binary_rebuilds_newer_schema"]
    check(downgrade["user_version_injection"] is False and downgrade["source_commit"] == PREVIOUS, "downgrade injected/rebound")
    phase_states = [downgrade["before"], downgrade["after"],
                    pc["current_binary_restored_after_actual_downgrade"]["result"],
                    pc["actual_current_database_backup_restore"]["result"]]
    phase_names = ["01-current-build", "02-previous-opens-current", "03-current-restored", "04-original-backup-restored"]
    pair_phases = []
    for index, (name, state) in enumerate(zip(phase_names, phase_states)):
        label = "previous" if index == 1 else "current"
        phase = local_smoke(pair_dir / name, pair_dir / "fixture", pair["products"][label]["binary_sha256"], state)
        check(state["database"]["schema_version"] == versions[label], "RPC state schema mismatch")
        if index in [1, 2]:
            check(phase["schema_mismatch_diagnostics"] > 0, "missing controlled rebuild diagnostic")
        phase["schema_version"] = versions[label]
        pair_phases.append(phase)
    pair_backups = [check_backup(pair_dir, pair[k], output) for k in
                    ["current_database_backup", "previous_database_backup", "current_rebuilt_backup"]]
    check([x["snapshot"] for x in pair_backups] == [s["database"] for s in phase_states[:3]], "backups/state mismatch")
    check(source_manifest(pair_dir / "fixture") == pair["source_files"] == load(pair_dir / "source-before.json")
          and pair["source_unchanged"] is True, "version source lost")
    check(digest(pair_dir / "fixture/.codecortex.json") == digest(pair_dir / "backups/config-original.json")
          == pair["configuration_sha256"], "version config changed")

    rollback_dir = R / "rollback"
    rollback = load(rollback_dir / "rollback.json")
    check(rollback["status"] == "passed_limited_local_drill", "bounded rollback scope differs")
    rc = {c["id"]: c for c in rollback["cases"]}
    check(len(rc) == 6 and sum(c["status"] == "passed" for c in rc.values()) == 5
          and rc["active_cache_reader_rejects_unknown_format"]["status"] == "not_run", "bounded original scope relabeled")
    rb_backups = [check_backup(rollback_dir, rollback[k], output) for k in ["old_database", "future_database", "rebuilt_database"]]
    check([x["snapshot"]["schema_version"] for x in rb_backups] == [25, 1025, 25], "injected fault/schema differs")
    validate_rebuild(rb_backups[1]["snapshot"], rb_backups[2]["snapshot"])
    rollback_phases = []
    for index, name in enumerate(["01-before", "02-schema-rollback", "03-default-package", "04-backup-restored"]):
        package = "semantic" if index < 2 else "default"
        rollback_phases.append(local_smoke(rollback_dir / name, rollback_dir / "fixture", rollback["products"][package]["binary_sha256"]))
    cache = rc["operator_cache_version_roots"]
    future_root = rollback_dir / "semantic-cache/future-format-999"
    check(file_manifest(future_root) == cache["future_files_unchanged"], "future cache changed")
    check(cache["rollback_root_created"] is False and not (rollback_dir / "semantic-cache/rollback-format-1").exists(),
          "disabled no-write control mismatch")
    source = rc["source_and_config_integrity"]
    check(source_manifest(rollback_dir / "fixture") == source["source_manifest"]
          == load(rollback_dir / "source-before.json"), "bounded source changed")
    check(digest(rollback_dir / "fixture/.codecortex.json") == source["config_sha256"]
          == digest(rollback_dir / "backups/config-before.json"), "bounded config changed")
    check(load(rollback_dir / "fixture/.codecortex.json")["semantic"]["enabled"] is False, "semantic config not disabled")
    active = [active_case(seed, products["semantic-http"]["binary_sha256"], output) for seed in [223, 227, 229]]
    local = local_cases(products["default"]["binary_sha256"], output)
    check(file_manifest(R) == original_hashes, "original evidence bytes changed during independent audit")
    report = dict(schema_version=1, reviewer="/root/pr_audit", source=G,
                  status="accepted_original_rollback_and_fault_subacceptance", original_artifact_id=11597639035,
                  original_run_id=37890757129, original_job_id=113690916709,
                  original_run_url="https://github.com/jyqj/codecortex/actions/runs/37890757129",
                  original_artifact_api_url="https://api.github.com/repos/jyqj/codecortex/actions/artifacts/11597639035",
                  original_zip_sha256="4c3035f0458e7c91088f9881ea9e5b3072b83dad9ae27a75939cc2afefbcd854",
                  original_report_sha256=digest(R / "full-recovery.json"),
                  complete_inventory_audit_sha256=digest(A / "raw-integrity-review.json"),
                  original_bytes_unchanged=True, original_file_count=len(original_hashes),
                  execution_scope="pure original-result validation; Git reads; SQLite copies only; no compilation/product/provider/network execution",
                  actual_version_pair=dict(status="accepted", released_package_certification=False, schema_fault_injection=False,
                                           source_products=schema_sources, production_schemas=versions, phases=pair_phases,
                                           original_backups=pair_backups, source_and_config_unchanged=True),
                  bounded_rollback=dict(status="accepted_in_declared_local_scope", phases=rollback_phases,
                                        original_backups=rb_backups, future_sentinel_rejected=True,
                                        default_and_semantic_disabled_local_search=True,
                                        physical_future_cache_hashes=cache["future_files_unchanged"],
                                        original_not_run_preserved=rc["active_cache_reader_rejects_unknown_format"],
                                        original_limitations=rollback["limitations"]),
                  active_original_controls=active,
                  original_production_linked_tests=integrity["recovery_cases"],
                  P8_016=dict(subacceptance="accepted_actual_source_pair_and_complementary_cache_controls",
                              coverage=["unmodified actual schema 25 -> historical 24 controlled rebuild -> current 25 rebuild",
                                        "original current database and authored configuration backup/restore",
                                        "default/semantic disabled public local search",
                                        "active unsupported format rejection in three original HTTP seeds",
                                        "production artifact-cache different-space and namespace isolation tests"],
                              task_done=False, open_dependencies=["P8-012", "P8-013"]),
                  P8_011=dict(subacceptance="accepted_named_original_local_and_active_fault_controls", local=local,
                              complementary_controls=["three active loopback disconnect/cache/DB replacement seeds",
                                                       "seven exact, nonignored, production-linked persistence/cache tests"],
                              limits=["original local submatrix remains 3 passed/4 not_run; complements keep separate identity",
                                      "SIGSTOP/SIGKILL case is pending RPC, not internal transaction midpoint",
                                      "disconnect recovery supersedes failed work with new source; no shortened retry clocks",
                                      "paid external provider behavior/cost and physical power loss are not measured"],
                              task_done=False, open_dependencies=["P8-007", "P8-010"]),
                  TODO_closed=0, TODO_remaining=29,
                  cross_source_certification=False)
    destination = A / "rollback-fault-acceptance-review.json"
    destination.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n")
    print(json.dumps(dict(status=report["status"], report=str(destination), report_sha256=digest(destination),
                          actual_schema_sequence=[25,24,25,25], rollback_rpc_processes=8,
                          active_seeds=[223,227,229], local_cases=3, production_linked_tests=7,
                          original_bytes_unchanged=True, TODO_remaining=29)))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output-dir", type=Path, required=True)
    arguments = parser.parse_args()
    main(arguments.output_dir.resolve())
