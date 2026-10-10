#!/usr/bin/env python3
"""Offline custody/identity reader; never imports or executes the native driver.

Input ZIPs and metadata are read only. Exit 0 means this reader found no internal
contradiction among supplied originals. It is not a successful native outcome,
complete custody, original validate_build/shard, or scale-cohort acceptance.
Missing artifacts, receipts, final output, ACKs and tails remain explicit unknowns.
"""
import argparse
from collections import defaultdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import sys
import zipfile

STREAMS = (
    "shard/native/raw.jsonl", "shard/native/worker.stderr",
    "shard/supervisor.stdout", "shard/supervisor.stderr",
    "driver.stdout", "driver.stderr", "wrapper.stdout", "wrapper.stderr",
)
META = (
    "configuration.json", "supervisor-started.json", "supervisor-terminal.json",
    "observer-before.json", "observer-after.json", "observer-after-error.json",
    "shard/registered-plan.json", "shard/source-before.json", "shard/source-after.json",
    "shard/disk-preflight.json", "shard/disk-after.json", "shard/shard.json",
    "shard/native/plan.json", "shard/native/report.json", "shard/native/worker-summary.json",
)


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_sha(path):
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key: " + key)
        result[key] = value
    return result


def decode(data):
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def same(a, b):
    if type(a) is not type(b):
        return False
    if isinstance(a, dict):
        return a.keys() == b.keys() and all(same(a[k], b[k]) for k in a)
    if isinstance(a, list):
        return len(a) == len(b) and all(same(x, y) for x, y in zip(a, b))
    return a == b


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()


def hex_value(value, length):
    return isinstance(value, str) and re.fullmatch("[0-9a-f]{" + str(length) + "}", value) is not None


def digest_field(value):
    if value is None or value == "":
        return None
    value = value.removeprefix("sha256:")
    require(hex_value(value, 64), "invalid official artifact digest")
    return value


def interval_sha(stream, length):
    digest = hashlib.sha256()
    while length:
        block = stream.read(min(length, 1024 * 1024))
        require(block, "retained interval truncated")
        digest.update(block)
        length -= len(block)
    return digest.hexdigest()


def integer(value, low, high=None):
    return type(value) is int and value >= low and (high is None or value <= high)


class OriginalZip:
    def __init__(self, row, binding):
        self.row = row
        self.kind = row["kind"]
        self.artifact = row["artifact"]
        self.id = str(self.artifact["id"])
        self.path = Path(row["zip_path"])
        require(self.path.is_file() and not self.path.is_symlink(), "missing/nonregular original ZIP")
        require(row.get("official_original_reference"), "missing original metadata/transport reference")
        require(integer(self.artifact["id"], 1), "invalid artifact ID")
        require(str(row["run_attempt"]) == str(binding["run_attempt"]), "artifact attempt differs")
        wr = self.artifact["workflow_run"]
        require(str(wr["id"]) == str(binding["run_id"]) and wr["head_sha"] == binding["G"],
                "artifact run/source differs")
        stem = f"{binding['run_id']}-{binding['run_attempt']}"
        names = {"build": "p8-100k-diagnostic-build-" + stem,
                 "capacity": "p8-100k-diagnostic-capacity-" + stem,
                 "final": "p8-100k-diagnostic-original-" + stem}
        if self.kind == "checkpoint":
            require(integer(row.get("index"), 0, 19), "checkpoint outside original 20 intervals")
            wanted = f"p8-100k-diagnostic-progress-{stem}-{row['index']:02d}"
        else:
            require(self.kind in names, "unexpected artifact kind")
            wanted = names[self.kind]
        require(self.artifact["name"] == wanted, "artifact namespace differs")
        require(self.path.stat().st_size == self.artifact["size_in_bytes"], "whole ZIP size differs")
        self.digest = file_sha(self.path)
        declared = digest_field(self.artifact.get("digest"))
        require(declared is None or declared == self.digest, "whole ZIP official SHA differs")
        self.zip = zipfile.ZipFile(self.path)
        self.members = {}
        for entry in self.zip.infolist():
            name = entry.filename
            parts = PurePosixPath(name)
            require(not parts.is_absolute() and ".." not in parts.parts and "\\" not in name,
                    "unsafe ZIP member")
            require(name not in self.members, "duplicate ZIP member")
            mode = entry.external_attr >> 16
            require(not stat.S_ISLNK(mode), "symlink ZIP member")
            if entry.is_dir():
                continue
            require(stat.S_IFMT(mode) in (0, stat.S_IFREG), "nonregular ZIP member")
            h = hashlib.sha256()
            count = 0
            with self.zip.open(entry) as stream:
                for block in iter(lambda: stream.read(1024 * 1024), b""):
                    count += len(block)
                    h.update(block)
            require(count == entry.file_size, "ZIP member size differs")
            self.members[name] = {"bytes": count, "sha256": h.hexdigest(),
                                  "zip_crc32": entry.CRC, "zip_posix_mode": oct(mode & 0o777)}
        self.record = {"id": self.id, "kind": self.kind, "name": wanted,
                       "zip_path": str(self.path), "bytes": self.path.stat().st_size,
                       "sha256": self.digest, "official_digest_verified": declared is not None,
                       "crc_and_member_sha_checked": True, "members": self.members,
                       "original_reference": row["official_original_reference"]}

    def data(self, name):
        return self.zip.read(name)

    def json(self, name):
        return decode(self.data(name))


def check_snapshot(value, binding):
    require(value["source_commit"] == binding["G"] and value["source_tree"] == binding["G_tree"],
            "original source snapshot commit/tree differs")
    require(same(value["inputs"], binding["product_inputs"]), "original full product map differs")
    require(value["input_count"] == len(binding["product_inputs"]) and
            value["manifest_sha256"] == sha(json_bytes(binding["product_inputs"])),
            "original full product map digest/count differs")


def check_driver(value, binding):
    require(value["source_commit"] == binding["G"] and same(value["inputs"], binding["driver_inputs"]),
            "original driver/helper identity differs")
    require(value["manifest_sha256"] == sha(json_bytes(binding["driver_inputs"])), "driver map digest differs")


def check_observer(value, binding):
    require(value["schema"] == "p8-diagnostic-observer-source-v1" and value["source"] == binding["G"]
            and same(value["inputs"], binding["observer_inputs"]), "three observer source identities differ")


def read_build(original, binding):
    if "build.json" not in original.members:
        return {"status": "build_receipt_missing", "ELF_execution": "not_run"}, None
    record = original.json("build.json")
    require(record["source_commit"] == binding["G"], "build source differs")
    check_driver(record["driver_source"], binding)
    require(record["driver_sha256"] == binding["driver_inputs"]["scripts/p8_scale_matrix.py"], "driver SHA differs")
    expected = {name: {"bytes": entry["bytes"], "sha256": entry["sha256"]}
                for name, entry in original.members.items() if name != "build.json"}
    require(same(record["files"], expected), "original build sealed file map differs")
    for name in ("source-before.json", "source-after.json"):
        if name in original.members:
            check_snapshot(original.json(name), binding)
    if "source-after.json" in original.members:
        require(same(original.json("source-before.json"), original.json("source-after.json")), "build before/after differ")
    summary = {"status": record.get("status"), "exit_code": record.get("exit_code"),
               "build_receipt_sha256": original.members["build.json"]["sha256"],
               "binary_sha256": record.get("binary_sha256"), "binary_blake3_recorded": record.get("binary_blake3"),
               "ELF_execution": "not_run", "original_validate_build": "not_run_by_this_reader"}
    if "p8-scale" in original.members:
        binary = original.members["p8-scale"]
        require(binary["sha256"] == record["binary_sha256"] and binary["bytes"] == record["binary_bytes"], "ELF bytes differ")
        with original.zip.open("p8-scale") as stream:
            require(stream.read(4) == b"\x7fELF", "preserved binary lacks ELF magic")
        copied = record["copy_source"]
        artifact = record["cargo_artifact"]
        target = artifact["target"]
        require(copied["bytes"] == binary["bytes"] and copied["sha256"] == binary["sha256"] and
                copied["path"] == artifact["executable"], "producer copy identity differs")
        require(target["name"] == "p8-scale" and target["kind"] == ["bin"] and
                artifact.get("features") in ([], ["default"]), "producer target differs")
        root = record["build_root"]
        require(artifact["manifest_path"] == root + "/crates/cc-eval/Cargo.toml" and
                target["src_path"] == root + "/crates/cc-eval/src/bin/p8-scale.rs", "producer checkout paths differ")
        command = ["cargo", "build", "--release", "--locked", "-p", "cc-eval", "--bin", "p8-scale",
                   "--message-format=json", "--target-dir", record["target_directory"]]
        require(record["command"] == command, "original Cargo command differs")
        profile = artifact["profile"]
        require(profile.get("opt_level") in ("1", "2", "3", "s", "z") and
                profile.get("debug_assertions") is False and profile.get("test") is False, "producer release profile differs")
        cargo = [decode(line) for line in original.data("cargo.jsonl").splitlines() if line]
        require(cargo[-1] == {"reason": "build-finished", "success": True}, "Cargo completion differs")
        require([m for m in cargo if m.get("reason") == "compiler-artifact" and
                 m.get("target", {}).get("name") == "p8-scale"] == [artifact], "producer Cargo row differs")
        summary["ELF_bytes_verified"] = True
    return summary, record


def inspect(binding, index):
    require(hex_value(binding.get("G"), 40) and hex_value(binding.get("G_tree"), 40), "actual G/tree must be supplied")
    require(integer(binding.get("run_id"), 1) and integer(binding.get("run_attempt"), 1), "actual run/attempt must be supplied")
    require(binding.get("event") in ("pull_request", "workflow_dispatch"), "unexpected run event")
    run = index["run"]
    require(str(run["id"]) == str(binding["run_id"]) and str(run["run_attempt"]) == str(binding["run_attempt"])
            and run["head_sha"] == binding["G"] and run["event"] == binding["event"]
            and run["path"] == binding["workflow_path"], "official run differs from binding")
    result = {"schema": "p8-independent-100k-custody-reader-report-v1", "binding": {k: binding[k] for k in
              ("P", "R", "G", "G_tree", "run_id", "run_attempt", "event", "workflow_path")},
              "official_run": run, "official_jobs": index.get("jobs", []), "artifacts": [],
              "build": None, "checkpoints": [], "capture_faults": [], "metadata_parse_unknown": [],
              "reported_native_outcome": "unknown", "native_semantic_validation": "not_run",
              "original_driver_or_ELF_executed": False, "accepted_cohort_shards": 0,
              "accepted_cohort_samples": 0, "task_completion_credit": 0}
    originals, known = [], {}
    ordered = sorted(index["artifacts"], key=lambda row: (
        {"build": 0, "capacity": 1, "checkpoint": 2, "final": 3}[row["kind"]], row.get("index", 0)))
    for row in ordered:
        original = OriginalZip(row, binding)
        require(original.id not in known, "duplicate artifact ID in intake")
        require(not any(o.artifact["name"] == original.artifact["name"] for o in originals), "duplicate artifact name")
        known[original.id] = original
        originals.append(original)
        result["artifacts"].append(original.record)
    build_record = None
    build_sha = None
    for original in originals:
        if original.kind == "build":
            result["build"], build_record = read_build(original, binding)
            build_sha = original.members.get("build.json", {}).get("sha256")

    manifests, declared, available, metadata = [], {}, {}, defaultdict(list)
    for original in originals:
        for name in original.members:
            if name.endswith("manifest.json") and original.kind in ("checkpoint", "final"):
                candidate = original.json(name)
                if candidate.get("schema") != "p8-diagnostic-byte-checkpoint-v1":
                    continue
                idx = candidate["index"]
                expected_index = original.row["index"] if original.kind == "checkpoint" else 20
                require(idx == expected_index, "checkpoint manifest index differs")
                require(candidate.get("diagnostic_only") is True and candidate.get("native_EOF_claimed") is False,
                        "checkpoint claims non-diagnostic outcome/EOF")
                base = name.rpartition("/")[0]
                prefix = base + "/" if base else ""
                included = candidate["included_chunk_names"]
                require(len(included) == len(set(included)), "duplicate included chunk names")
                by_name = {}
                for chunk in candidate["all_captured_chunks"]:
                    require(chunk["source"] in STREAMS and integer(chunk["start"], 0) and
                            integer(chunk["end"], chunk["start"] + 1) and chunk["bytes"] == chunk["end"] - chunk["start"]
                            and hex_value(chunk["sha256"], 64), "invalid captured byte interval")
                    expected_name = chunk["source"].replace("/", "_") + f".{chunk['start']:012d}-{chunk['end']:012d}.bin"
                    require(chunk["name"] == expected_name and chunk["name"] not in by_name, "chunk name/duplicate differs")
                    require(chunk["name"] not in declared or same(declared[chunk["name"]], chunk), "chunk definition changed")
                    declared[chunk["name"]] = chunk
                    by_name[chunk["name"]] = chunk
                require(set(included) <= set(by_name), "included chunk missing declaration")
                for short in included:
                    member = prefix + short
                    chunk = by_name[short]
                    entry = original.members.get(member)
                    require(entry and entry["bytes"] == chunk["bytes"] and entry["sha256"] == chunk["sha256"],
                            "retained chunk size/SHA differs")
                    available.setdefault(short, []).append({"artifact_id": original.id, "member": member})
                for item in candidate["metadata_snapshots"]:
                    require(item["source"] in META, "unknown snapshot source")
                    member = prefix + item["file"]
                    entry = original.members.get(member)
                    require(entry and entry["bytes"] == item["bytes"] and entry["sha256"] == item["sha256"], "snapshot bytes differ")
                    metadata[item["source"]].append((original, member, "point_in_time_snapshot"))
                result["capture_faults"].extend({"artifact_id": original.id, "index": idx, **fault}
                                                 for fault in candidate["capture_faults"])
                manifests.append((original, name, candidate))
        if original.kind == "final":
            for relative in META:
                if relative in original.members:
                    metadata[relative].append((original, relative, "final_original"))

    parsed = defaultdict(list)
    for relative, versions in metadata.items():
        for original, member, context in versions:
            try:
                value = original.json(member)
            except (ValueError, UnicodeError) as error:
                # Mid-write snapshots may be incomplete JSON. Preserve bytes and uncertainty.
                result["metadata_parse_unknown"].append({"artifact_id": original.id, "member": member,
                                                        "context": context, "error": str(error)})
                continue
            parsed[relative].append((original, member, context, value))
            if relative in ("observer-before.json", "observer-after.json"):
                check_observer(value, binding)
            elif relative in ("shard/source-before.json", "shard/source-after.json"):
                check_snapshot(value, binding)
            elif relative in ("shard/registered-plan.json", "shard/native/plan.json"):
                require(same(value, binding["plan"]), "original registered/native plan differs")
            elif relative == "shard/shard.json":
                require(value["source_commit"] == binding["G"] and same(value["plan"], binding["plan"]), "shard identity differs")
                check_driver(value["driver_source"], binding)
                if build_record:
                    for key in ("source_commit", "source_manifest_sha256", "binary_sha256", "binary_blake3", "driver_source"):
                        require(same(value[key], build_record[key]), "shard/build binding differs: " + key)
                    require(value["build_receipt_sha256"] == build_sha, "shard build receipt differs")

    configs = parsed["configuration.json"]
    for original, member, context, config in configs:
        require(config.get("schema") == "p8-independent-100k-diagnostic-v1" and config["source"] == binding["G"]
                and str(config["run_id"]) == str(binding["run_id"]) and str(config["run_attempt"]) == str(binding["run_attempt"]),
                "capture configuration source/run differs")
        require(config["original_driver_sha256"] == binding["driver_inputs"]["scripts/p8_scale_matrix.py"]
                and config["registered_N_unchanged"] == 30 and config["diagnostic_only"] is True
                and config["whole_cohort_completion_credit"] is False, "capture configuration protocol differs")
        require(config["checkpoint_count"] == 20 and config["checkpoint_interval_seconds"] == 900
                and config["outer_job_seconds"] == 21000, "capture schedule differs")
        command = config["command"]
        require(len(command) == 19 and command[1] == config["source_root"] + "/scripts/p8_scale_matrix.py"
                and command[2:5] == ["run", "--root", config["source_root"]]
                and command[5] == "--build" and command[7:17] == ["--scale", "100000", "--shard-index", "0",
                "--shard-count", "30", "--repetitions", "30", "--capacity-profile", "scale_wide_dirty_v1"]
                and command[17] == "--output", "capture original driver argv differs")
        if build_sha:
            require(config["original_build_receipt_sha256"] == build_sha, "configuration build receipt differs")
    config_hashes = {o.members[n]["sha256"] for o, n, _, _ in configs}
    require(len(config_hashes) <= 1, "configuration bytes changed across artifacts")
    observer_hashes = {o.members[n]["sha256"] for o, n, _, _ in parsed["observer-before.json"]}
    for _, _, _, config in configs:
        require(not observer_hashes or config["observer_before_sha256"] in observer_hashes, "observer-before receipt SHA differs")

    manifest_by_id = {o.id: (o, name, value) for o, name, value in manifests}
    require(len(manifest_by_id) == len(manifests), "multiple checkpoint manifests in one artifact")
    for original, name, manifest in manifests:
        if config_hashes:
            require(manifest["configuration_sha256"] in config_hashes, "checkpoint configuration SHA differs")
        acked, acknowledgements = set(), []
        for ack in manifest["acknowledged_uploads_before_this_upload"]:
            acked.update(ack["included_chunk_names"])
            target = manifest_by_id.get(str(ack["artifact_id"]))
            verified = False
            if target:
                other, other_name, other_manifest = target
                require(ack["checkpoint"] == other_manifest["index"] and
                        ack["manifest_sha256"] == other.members[other_name]["sha256"] and
                        ack["included_chunk_names"] == other_manifest["included_chunk_names"] and
                        ack["artifact_digest"] == other.digest, "ACK does not match retained original artifact")
                verified = digest_field(other.artifact.get("digest")) is not None
            acknowledgements.append({**ack, "retained_original_and_official_digest_verified": verified})
        for stream in STREAMS:
            position = 0
            for chunk in sorted((c for c in manifest["all_captured_chunks"] if c["source"] == stream), key=lambda c: c["start"]):
                if chunk["start"] != position or chunk["name"] not in acked:
                    break
                position = chunk["end"]
            require(manifest["acknowledged_prefix_bytes_before_this_upload"][stream] == position, "claimed ACK prefix differs")
        result["checkpoints"].append({"artifact_id": original.id, "index": manifest["index"],
                                      "manifest_sha256": original.members[name]["sha256"],
                                      "supervisor_terminal_observed_claim": manifest["supervisor_terminal_observed"],
                                      "acknowledgements": acknowledgements,
                                      "official_artifact_custody_is_independent_of_later_local_ACK": True})

    result["streams"] = {}
    for stream in STREAMS:
        chunks = sorted((c for c in declared.values() if c["source"] == stream), key=lambda c: c["start"])
        prior = 0
        for chunk in chunks:
            require(chunk["start"] >= prior, "overlapping nonidentical captured intervals")
            prior = chunk["end"]
        position = 0
        for chunk in chunks:
            if chunk["start"] != position or chunk["name"] not in available:
                break
            position = chunk["end"]
        result["streams"][stream] = {"verified_retained_contiguous_chunk_prefix_bytes": position,
                                    "highest_declared_offset": max((c["end"] for c in chunks), default=0),
                                    "chunks": [{**c, "retained_locations": available.get(c["name"], [])} for c in chunks],
                                    "unknown_tail_bytes": None, "prefix_is_not_EOF": True}
        for original in originals:
            if original.kind == "final" and stream in original.members:
                entry = original.members[stream]
                result["streams"][stream]["final_original"] = {"artifact_id": original.id, "member": stream, **entry}
                with original.zip.open(stream) as full:
                    for chunk in chunks:
                        if chunk["end"] > entry["bytes"]:
                            result["streams"][stream]["final_shorter_than_captured_offset"] = True
                            continue
                        full.seek(chunk["start"])
                        require(interval_sha(full, chunk["bytes"]) == chunk["sha256"], "final original differs from captured bytes")

    terminals = parsed["supervisor-terminal.json"]
    native_reports = parsed["shard/native/report.json"]
    shard_reports = parsed["shard/shard.json"]
    result["supervisor_receipts"] = [{"artifact_id": o.id, "member": n, "context": context, "record": value}
                                     for o, n, context, value in terminals]
    result["native_reports"] = [{"artifact_id": o.id, "member": n, "context": context, "record": value}
                                 for o, n, context, value in native_reports]
    result["original_shard_statuses"] = [{"artifact_id": o.id, "member": n, "context": context,
                                           "status": v.get("status"), "exit_code": v.get("exit_code"), "error": v.get("error")}
                                          for o, n, context, v in shard_reports]
    if terminals:
        terminal = terminals[-1][3]
        code = terminal.get("driver_returncode")
        require(code is None or type(code) is int, "driver return code is not an integer/null")
        if type(code) is int:
            require(terminal.get("driver_signal") == (-code if code < 0 else None), "driver signal differs from return code")
        bound_native = [v for o, n, _, v in native_reports if
                        o.members[n]["sha256"] == terminal.get("native_report_sha256")]
        for native in bound_native:
            require(terminal.get("native_exit_code") == native.get("exit_code") and
                    terminal.get("native_worker_exit_code") == native.get("worker_exit_code") and
                    terminal.get("native_report_status") == native.get("status"), "supervisor/native receipt differs")
        if code not in (None, 0) or terminal.get("wrapper_timeout") or terminal.get("wrapper_error"):
            result["reported_native_outcome"] = "driver_failed_or_wrapper_failed; native cause only as original report states"
        elif code == 0 and bound_native and shard_reports and bound_native[-1].get("exit_code") == 0 and \
                bound_native[-1].get("status") == "measurement_complete" and shard_reports[-1][3].get("status") == "passed" \
                and shard_reports[-1][3].get("exit_code") == 0:
            result["reported_native_outcome"] = "native_and_driver_success_claims_present; original_semantic_validation_not_run"
    result["observer_before_seen"] = bool(parsed["observer-before.json"])
    result["observer_after_seen"] = bool(parsed["observer-after.json"])
    result["configuration_seen_and_bound"] = bool(configs)
    result["original_build_seen_and_bound"] = build_record is not None
    result["observer_errors"] = [v for _, _, _, v in parsed["observer-after-error.json"]]
    result["custody_status"] = "partial_with_capture_faults" if result["capture_faults"] else "retained_bytes_verified; whole_run_completeness_unknown"
    result["limits"] = ["No original validate_build, validate_shard, inspect_raw, BLAKE3 ELF command or workload executed.",
                         "No missing tail is assigned zero bytes; a retained prefix is not a complete 100k run.",
                         "Missing historical checkpoint manifests can hide transient capture faults; absence of observed faults is not proof of none.",
                         "Runner loss and descendant cleanup remain unknown unless original receipts establish them.",
                         "No rows, shards or results enter any old 150-slot cohort or close a task."]
    for original in originals:
        require(file_sha(original.path) == original.digest, "original ZIP changed while reading")
        original.zip.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binding", required=True)
    parser.add_argument("--official-index", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args()
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=False)
    started = datetime.now(timezone.utc).isoformat()
    binding_path, index_path = Path(args.binding), Path(args.official_index)
    inputs = {str(p): file_sha(p) for p in (binding_path, index_path)}
    result, code = None, 0
    try:
        result = inspect(decode(binding_path.read_bytes()), decode(index_path.read_bytes()))
        require(inputs == {str(p): file_sha(p) for p in (binding_path, index_path)}, "bound input changed while reading")
        (output / "report.json").write_bytes(json_bytes(result))
    except (OSError, ValueError, KeyError, TypeError, IndexError, zipfile.BadZipFile) as error:
        code = 1
        (output / "reader-failure.json").write_bytes(json_bytes({"error_type": type(error).__name__, "error": str(error),
                    "scope": "Reader/custody inconsistency, not a new native execution result; all original ZIPs remain untouched."}))
    receipt = {"argv": sys.argv, "started_utc": started, "ended_utc": datetime.now(timezone.utc).isoformat(),
               "actual_exit_code": code, "input_sha256": inputs,
               "reader_sha256": file_sha(Path(__file__)), "product_or_driver_execution": False}
    (output / "execution-receipt.json").write_bytes(json_bytes(receipt))
    print(json.dumps({"actual_exit_code": code, "output": str(output), "new_cohort_samples": 0}))
    return code


if __name__ == "__main__":
    sys.exit(main())
