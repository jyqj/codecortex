"""Fixed G4 receiver delivery intake. Offline metadata/byte checks only.

Never invokes Cargo, product, oracle or the statistics executable. Statistics
execution is the already completed hosted receiver's result, preserved below.
"""
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys
import zipfile

from disk_zip import require, IntakeBudget, audit_zip, compare_expanded, check_seals

G4 = "260f596582f2d82b8d7c707b61a6b8b6a43b069f"
CONTROL = "9aacfbea2771209134b4f2dd8889cbb4e2a3d099"
RUN = 37879784342
ARTIFACT = 11593443086
SIZE = 233905529
SHA = "7bc22b041108af5bf8f29b571fe094c35ede0f92cb56c93ad29bcd4c8f9ff1b6"
SOURCE = Path("/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source")
SPEC = [
    ("mixed/C1", "mixed/C1-artifact-11590168318.zip", 11590168318, 16588933, "d793ce76d6da48c3cc96b28afad80c8eeab5576c4cb4d7ad364e5c823eb35355"),
    ("mixed/C4", "mixed/C4-artifact-11590202791.zip", 11590202791, 16959276, "0adef902e90e21f9d32349d330a4f0d48041ae336878e826ab5b6beca67eec24"),
    ("mixed/C8", "mixed/C8-artifact-11590639057.zip", 11590639057, 16621243, "befead56e71df69ab5c88d0483de67b9c2009b99ee82df0e683f99a25844daa7"),
    ("mixed/C16", "mixed/C16-artifact-11588372461.zip", 11588372461, 16388340, "ad788f605deb6e55e7ad3d944394e079a5983ea774560869d2a9b09624e22e2a"),
    ("soak", "soak/artifact-11590202308.zip", 11590202308, 50994513, "46a72baaecbd0e5ea3197d05e639f618cd3a570b9cc65f46fa51a5f0a4916286"),
]
PRIOR_MAPPING_GIT_BLOB = "95d8e14c6473638d4c4063b31b52864eb85b274f"
MAX_JSON_LINE = 4 * 1024**2

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def git_blob(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()

def value(raw):
    return json.loads(raw)

def records(stream):
    while True:
        line = stream.readline(MAX_JSON_LINE + 1)
        if not line:
            break
        require(len(line) <= MAX_JSON_LINE and line.endswith(b"\n"), "oversized or incomplete original JSONL")
        require(line.strip(), "empty original JSONL row")
        yield value(line)

def endpoint_transport(stream, endpoint_id):
    counts = Counter()
    chosen = {}
    times = {}
    ids = {"request": set(), "response": set(), "stdout_wire": set()}
    eof = exit0 = False
    search_count = 0
    for event in records(stream):
        kind = event.get("kind") if event.get("event") == "terminal" else event.get("event")
        counts[kind] += 1
        if kind in ("request", "response"):
            payload = event["payload"]
            rpc_id = payload["id"]
            require(rpc_id not in ids[kind], "duplicate original transport ID")
            ids[kind].add(rpc_id)
            if kind == "request" and payload.get("method") == "tools/call" and payload.get("params", {}).get("name") == "search":
                search_count += 1
            if rpc_id == endpoint_id:
                chosen[kind] = payload
                times[kind] = event["time_ns"]
        elif kind == "stdout_wire":
            raw = event["text"].encode()
            require(len(raw) == event["wire_bytes"] and sha(raw) == event["wire_sha256"],
                    "original stdout wire identity")
            payload = value(raw)
            if "id" in payload:
                rpc_id = payload["id"]
                require(rpc_id not in ids[kind], "duplicate original wire response ID")
                ids[kind].add(rpc_id)
                if rpc_id == endpoint_id:
                    chosen[kind] = payload
        elif kind in ("stdout_eof", "eof"):
            eof = True
        elif kind == "process_exit":
            exit0 = event.get("exit_code", event.get("code")) == 0
    require(eof and exit0 and ids["request"] == ids["response"] == ids["stdout_wire"],
            "complete original transport denominator and EOF")
    require(set(chosen) == {"request", "response", "stdout_wire"} and chosen["response"] == chosen["stdout_wire"],
            "endpoint exact original response/wire")
    result = chosen["response"].get("result")
    require("error" not in chosen["response"] and isinstance(result, dict)
            and result.get("isError") is not True and isinstance(result.get("structuredContent"), dict),
            "endpoint tool result")
    structured = result["structuredContent"]
    return dict(counts=dict(counts), search_count=search_count, endpoint_request=chosen["request"],
                endpoint_value=structured.get("result", structured), times=times,
                ids=ids["request"])

def inspect(archive_path, source_root, scratch):
    global SOURCE
    SOURCE = Path(source_root)
    require(archive_path.stat().st_size == SIZE, "fixed outer ZIP byte length")
    command_names = ["seal-%d" % spec[2] for spec in SPEC] + ["original-mixed-review", "original-soak-review"]
    selected_names = {
        "receipt.json", "file-inventory.json", "withheld-original-job-logs.json",
        "registration.json", "source-before.json", "source-after.json",
        "observer-before.json", "observer-after.json", "receiver-mapping.json",
        "mixed/review_mixed.py", "soak/inspect.py", "soak/cache_wire.py",
        "mixed/independent-review.json", "soak/inspection.json",
        "soak/independent-review-01/cache-wire-binding.json",
        "soak/independent-review-01/statistics.json",
    }
    for name in command_names:
        for suffix in ("command.json", "result.json", "stdout.log", "stderr.log"):
            selected_names.add("commands/" + name + "/" + suffix)
    for home, _, _, _, _ in SPEC:
        selected_names.add(home + "/originals/p8-build/build-receipt.json")
        if home.startswith("mixed/"):
            selected_names.add(home + "/independent-statistics-replay/result.json")
    with archive_path.open("rb") as raw_archive:
        require(hashlib.file_digest(raw_archive, "sha256").hexdigest() == SHA, "complete original outer ZIP SHA")
        raw_archive.seek(0)
        budget = IntakeBudget()
        with zipfile.ZipFile(raw_archive) as outer:
            members, selected = audit_zip(outer, budget, selected_names)
            get = lambda name: value(selected[name])
            receipt = get("receipt.json")
            require(receipt["status"] == "accepted_scoped_original_G4_offline_replay"
                    and receipt["exit_code"] == 0 and receipt["controller_commit"] == CONTROL
                    and receipt["source_commit"] == G4 and receipt["original_run"] == 37854847820,
                    "fixed actual receiver receipt")
            require(receipt["original_artifact_ids"] == [s[2] for s in SPEC]
                    and receipt["new_product_workload"] is False and receipt["original_sources_relabelled"] is False,
                    "receiver original population and scope")
            inventory = get("file-inventory.json")
            withheld = get("withheld-original-job-logs.json")
            require(sha(selected["file-inventory.json"]) == receipt["file_inventory_sha256"]
                    and sha(selected["withheld-original-job-logs.json"]) == receipt["withheld_original_job_logs_sha256"],
                    "outer inventories bound to actual receipt")
            included = {n: r for n, r in inventory.items() if r["included_in_upload"]}
            excluded = {n: r for n, r in inventory.items() if not r["included_in_upload"]}
            require(set(members) == set(included) | {"receipt.json", "file-inventory.json"},
                    "complete delivered outer population")
            fixed_logs = {
                "mixed/C1/job-113576447129.log", "mixed/C4/job-113576447195.log",
                "mixed/C8/job-113576447334.log", "mixed/C16/job-113576446998.log",
                "soak/job-113576447137.log",
            }
            require(set(excluded) == set(withheld["files"]) == fixed_logs,
                    "only documented five original job logs withheld")
            for name, row in included.items():
                require(all(members[name][k] == row[k] for k in ("bytes", "sha256")), "outer member seal identity")
            for name, row in excluded.items():
                require(all(withheld["files"][name][k] == row[k] for k in ("bytes", "sha256")),
                        "withheld original log byte identity")
            require(selected["source-before.json"] == selected["source-after.json"]
                    and selected["observer-before.json"] == selected["observer-after.json"],
                    "receiver source unchanged")
            source, observer = get("source-before.json"), get("observer-before.json")
            require(source["source_commit"] == G4 and source["input_count"] == len(source["inputs"]) == 1087
                    and observer["source_commit"] == G4 and len(observer["files"]) == 7,
                    "original source and observer population")
            sys.path.insert(0, str(SOURCE / "scripts"))
            import p7_build_identity as identity
            import p8_runtime_build as builder
            local_source = identity.source_snapshot(SOURCE)
            local_observer = builder.observer_snapshot(SOURCE)
            for actual, local in ((source, local_source), (observer, local_observer)):
                normalized = dict(actual, source_root=local["source_root"])
                require(normalized == local, "actual exact original source/observer bytes")
            soak_command = get("commands/original-soak-review/command.json")
            require(len(soak_command["argv"]) == 3 and soak_command["argv"][1] == "-B"
                    and soak_command["argv"][2].endswith("/soak/inspect.py"),
                    "actual original soak reviewer argv")
            python = soak_command["argv"][0]
            require(Path(python).name in ("python3", "python3.12", "python"), "original Python executable role")
            remote_output = soak_command["argv"][2][:-len("/soak/inspect.py")]
            for name in command_names:
                result = get("commands/" + name + "/result.json")
                command = get("commands/" + name + "/command.json")
                require(result["exit_code"] == 0 and command["source_commit"] == G4
                        and command["scope"] == "offline replay only", "original offline command result")
                if name.startswith("seal-"):
                    spec = next(s for s in SPEC if name == "seal-%d" % s[2])
                    original_dir = remote_output + "/" + spec[0] + "/originals"
                    argv = [python, "-B", source["source_root"] + "/scripts/p8_runtime.py",
                            "verify", "--output", original_dir + "/p8-runtime",
                            "--build-output", original_dir + "/p8-build"]
                    cwd = source["source_root"]
                else:
                    relative = "mixed/review_mixed.py" if name == "original-mixed-review" else "soak/inspect.py"
                    argv = [python, "-B", remote_output + "/" + relative]
                    cwd = None
                require(command["argv"] == argv and command["cwd"] == cwd,
                        "actual unchanged offline checker argv and cwd")
            mapping = get("receiver-mapping.json")
            reg = get("registration.json")
            remote_root = source["source_root"]
            expected_bodies = {
                "mixed/review_mixed.py": "e0e6002810aa7722ed070483532a7853e9971f7f45f38ea5355caf9e5920825b",
                "soak/inspect.py": "4fa1e18e355cd303b0ed8df986438f5965fb186ee413249b0ac62a434a6705c1",
                "soak/cache_wire.py": "7ddec02ac351832562a7c0ff367cbccead2291c0fb447b91600c248ded57a5ea",
            }
            for name, digest in expected_bodies.items():
                body = selected[name].decode()
                if name == "mixed/review_mixed.py":
                    before = "REPO=Path(" + repr(remote_root) + ")"
                    after = "REPO=Path('/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source')"
                elif name == "soak/inspect.py":
                    before = "REPO = " + repr(remote_root)
                    after = "REPO = '/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source'"
                else:
                    before = after = None
                if before is not None:
                    require(body.count(before) == 1, "unique exact checkout-root mapping")
                    body = body.replace(before, after)
                require(sha(body.encode()) == digest and mapping[name]["selected_receiver_sha256"] == digest
                        and mapping[name]["mapped_sha256"] == members[name]["sha256"],
                        "original or explicitly derived receiver body")
            nested_results = []
            stream_counters = []
            for home, zipname, artifact, size, digest in sorted(SPEC, key=lambda x: outer.getinfo(x[1]).header_offset):
                require(members[zipname]["bytes"] == size and members[zipname]["sha256"] == digest,
                        "retained original ZIP fixed identity")
                nested_path = scratch / ("original-" + str(artifact) + ".zip")
                h = hashlib.sha256()
                copied = 0
                with outer.open(zipname) as raw, nested_path.open("xb") as copied_file:
                    while block := raw.read(64 * 1024):
                        copied += len(block)
                        require(copied <= size, "nested ZIP exceeds registered byte length")
                        h.update(block)
                        copied_file.write(block)
                require(copied == size and h.hexdigest() == digest, "disk copy of original nested ZIP differs")
                with zipfile.ZipFile(nested_path) as nested:
                    inner_members, seals = audit_zip(nested, budget,
                                                    {"p8-build/seal.json", "p8-runtime/seal.json"})
                    compare_expanded(inner_members, members, home + "/originals/")
                    check_seals(inner_members, seals)
                stream_counters.append(dict(artifact_id=artifact, disk_copy_bytes=copied,
                                            disk_copy_sha256=h.hexdigest(), whole_ZIP_RAM_buffer=False))
                nested_results.append(dict(artifact_id=artifact, bytes=size, sha256=digest,
                                           members=inner_members))
                build = get(home + "/originals/p8-build/build-receipt.json")
                require(build["source_before"] == build["source_after"]
                        and build["source_before"]["inputs"] == source["inputs"]
                        and build["observer_before"] == build["observer_after"],
                        "original build receipt complete provenance")
                stats_path = home + "/originals/p8-build/p8-runtime-statistics"
                require(members[stats_path]["sha256"] == build["statistics_sha256"],
                        "retained statistics binary identity")
                original_stats = home + "/originals/p8-runtime/statistics.json"
                replay_stats = (home + "/independent-statistics-replay/statistics.json"
                                if home.startswith("mixed/") else home + "/independent-review-01/statistics.json")
                require(members[original_stats]["sha256"] == members[replay_stats]["sha256"]
                        and members[original_stats]["bytes"] == members[replay_stats]["bytes"],
                        "actual original statistics replay output bytes")
                if home.startswith("mixed/"):
                    stats_result = get(home + "/independent-statistics-replay/result.json")
                    require(stats_result["exit_code"] == 0 and stats_result["byte_identical"],
                            "actual retained mixed statistics process result")
            mixed = get("mixed/independent-review.json")
            soak = get("soak/inspection.json")
            require(mixed["status"] == "accepted_scoped_mixed_raw" and len(mixed["cells"]) == 4,
                    "complete mixed actual replay")
            require(soak["decision"] == "accepted_scoped_original_G4_soak_observation"
                    and soak["statistics_replay"]["exit_code"] == 0
                    and soak["statistics_replay"]["byte_identical"]
                    and soak["outcomes"] == {"success": 3601} and soak["bound_cache_RPCs"] == 9600,
                    "complete soak actual replay")
            binding = get("soak/independent-review-01/cache-wire-binding.json")
            reference_bytes = (json.dumps(binding["mapping"], sort_keys=True, indent=2) + "\n").encode()
            require(len(binding["mapping"]) == 9600 and git_blob(reference_bytes) == PRIOR_MAPPING_GIT_BLOB,
                    "all 9600 mappings exactly equal prior original-byte independent diagnosis")
            require(binding["compound_reads"] == 2400 and binding["bound_RPCs"] == 9600
                    and binding["role_counts"] == dict(before_status=2400, symbol=2400, hybrid=2400, after_status=2400),
                    "original compound denominator")
            # Stream these original expanded members; do not retain large raw/RPC files.
            endpoint_rows = []
            operation_counts = Counter()
            last_operation_row = -1
            latest_finished = -1
            with outer.open("soak/originals/p8-runtime/raw.jsonl") as raw:
                for index, row in enumerate(records(raw)):
                    if row["kind"] == "operation":
                        operation_counts[row["operation"]] += 1
                        last_operation_row = index
                        latest_finished = max(latest_finished, row["finished_ns"])
                    elif row["kind"] == "endpoint_public":
                        endpoint_rows.append((index, row))
            require(operation_counts == {"build": 1201, "read": 2400}
                    and len(endpoint_rows) == 1 and endpoint_rows[0][0] > last_operation_row,
                    "original endpoint after complete raw operation population")
            with outer.open("soak/originals/p8-runtime/product/rpc.jsonl") as raw:
                product = endpoint_transport(raw, 14400)
            with outer.open("soak/originals/p8-runtime/full-product/rpc.jsonl") as raw:
                full = endpoint_transport(raw, 4)
            expected_params = dict(name="search", arguments=dict(query="p8_runtime_stable_signal", mode="symbol", top_k=5))
            for side, actual in (("incremental", product), ("full", full)):
                require(actual["endpoint_request"]["params"] == expected_params
                        and actual["endpoint_value"] == endpoint_rows[0][1][side],
                        "each endpoint original exact parameters and complete Value")
            require(product["search_count"] == 4801 and full["search_count"] == 1
                    and len(product["ids"]) == 14400 and len(full["ids"]) == 4,
                    "complete source-bound search/transport population")
            endpoint = binding["endpoint_binding"]
            require(endpoint["incremental_rpc_id"] == 14400 and endpoint["full_rpc_id"] == 4
                    and endpoint["endpoint_public_record_index"] == endpoint_rows[0][0]
                    and endpoint["exact_parameters"] == expected_params,
                    "actual derived endpoint mapping identity")
            for side, actual in (("incremental", product), ("full", full)):
                for kind in ("request", "response"):
                    require(endpoint[side + "_" + kind + "_ns"] == actual["times"][kind],
                            "endpoint actual original monotonic times")
            require(endpoint["latest_raw_operation_finished_ns"] == latest_finished
                    and product["times"]["request"] > binding["common_monotonic_origin_interval_ns"][1] + latest_finished
                    and product["times"]["request"] > endpoint["latest_bound_read_response_ns"]
                    and full["times"]["request"] >= product["times"]["response"],
                    "separate endpoint timing after original workload")
            require(identity.source_snapshot(SOURCE) == local_source
                    and builder.observer_snapshot(SOURCE) == local_observer, "local original source unchanged")
            return dict(schema="p8-G4-external-disk-intake-v1", status="accepted_scoped_original_receiver_delivery",
                        controller_commit=CONTROL, source_commit=G4, receiver_run=RUN, artifact_id=ARTIFACT,
                        zip_bytes=SIZE, zip_sha256=SHA, transport="independent disk-backed standard zipfile; not a Range validation",
                        actual_outer_members=members, actual_original_ZIPs=nested_results,
                        source=source, observer=observer, receipt=receipt,
                        original_mixed_replay=mixed, original_soak_replay=soak,
                        original_endpoint_binding=endpoint, exact_9600_mapping_git_blob=PRIOR_MAPPING_GIT_BLOB,
                        mapping_entries=9600, original_workload_samples=3601,
                        original_job_logs_withheld=withheld,
                        nested_stream_counters=stream_counters,
                        budgets=dict(metadata_bytes=budget.metadata_bytes, selected_bytes=budget.selected_bytes,
                                     member_count=budget.member_count),
                        original_statistics_executed_here=False, product_or_Cargo_executed=False,
                        hosted_owned_input_files_written=True, local_shared_files_written=False,
                        selected_original_outputs={k: v.decode() for k, v in selected.items()},
                        TODO_closed=0, TODO_remaining=29,
                        scope="Complete registered ZIP identity, standard zipfile full CRC/SHA, all delivered/nested inventories. Original hosted validators and retained statistics executed on immutable G4; intake performs no new workload. Raw original Actions logs remain intentionally excluded and are required for independent complete checker reruns.")
