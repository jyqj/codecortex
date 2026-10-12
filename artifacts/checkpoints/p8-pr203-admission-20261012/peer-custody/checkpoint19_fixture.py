#!/usr/bin/env python3
"""Synthetic byte custody control for exact P; no measured workload or approval.

The unchanged original snapshot/acknowledge functions execute on bounded mock
streams. Artifact IDs/digests here are explicitly synthetic fixture metadata.
"""
import copy
import hashlib
import json
from pathlib import Path
import sys
import datetime

import yaml

HERE = Path(__file__).resolve().parent
P = "0692ed27c4f86a254484e76155fe07a55ab2e09c"
sys.path.insert(0, str(HERE / "source/scripts"))


def git_blob(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x") as out:
        json.dump(value, out, indent=2, sort_keys=True)
        out.write("\n")


def artifact_digest(bundle):
    # A synthetic test token, NOT a GitHub upload receipt or ZIP digest.
    values = {p.name: hashlib.sha256(p.read_bytes()).hexdigest()
              for p in sorted(bundle.iterdir()) if p.is_file()}
    return hashlib.sha256(json.dumps(values, sort_keys=True).encode()).hexdigest()


def fixture(name, upload18_success):
    import p8_task_profile_capture as task
    root = HERE / "fixtures" / name
    root.mkdir(parents=True, exist_ok=False)
    write_json(root / "configuration.json", {
        "fixture": "SYNTHETIC_BYTE_CUSTODY_ONLY", "source": P,
        "official_run_id": None, "native_workload_executed": False,
    })
    stream = root / "shard/native-shard/native/raw.jsonl"
    stream.parent.mkdir(parents=True)
    stream.write_bytes(b"")
    manifests, bundles, ends = {}, {}, {}
    for index in range(20):
        # Deliberately not complete JSON: a valid byte prefix need not have EOF.
        with stream.open("ab") as raw:
            raw.write((f"synthetic-prefix-{index:02d}:" + "x" * (index + 1) + "\x00").encode())
        previous_id = previous_digest = ""
        if index > 0 and (index != 19 or upload18_success):
            previous_id = str(900000 + index - 1)
            previous_digest = artifact_digest(bundles[index - 1])
        bundle, manifest = task.core.snapshot(root, index, previous_id, previous_digest)
        bundles[index], manifests[index], ends[index] = bundle, manifest, stream.stat().st_size
    checkpoint19 = manifests[19]
    confirmed_index = 18 if upload18_success else 17
    assert checkpoint19["acknowledged_prefix_bytes_before_this_upload"][str(stream.relative_to(root))] == ends[confirmed_index]
    assert [x["checkpoint"] for x in checkpoint19["acknowledged_uploads_before_this_upload"]] == list(range(confirmed_index + 1))
    state_before_final = task.core.read_json(root / "capture-state.json")
    with stream.open("ab") as raw:
        raw.write(b"synthetic-final-20:still-partial-json")
    # Exactly the changed workflow: finish 20 supplies no upload_19 environment.
    final_bundle, final = task.core.snapshot(root, 20)
    state_after_final = task.core.read_json(root / "capture-state.json")
    assert state_after_final["uploads"] == state_before_final["uploads"]
    assert state_after_final["acknowledged_chunks"] == state_before_final["acknowledged_chunks"]
    assert final["native_EOF_claimed"] is False
    assert final["prefix_may_end_inside_JSON_record"] is True
    assert final["acknowledged_prefix_bytes_before_this_upload"][str(stream.relative_to(root))] == ends[confirmed_index]
    missing_upload_indices = list(range(confirmed_index + 1, 21))
    included = [c for c in final["all_captured_chunks"] if c["name"] in final["included_chunk_names"]]
    assert len(included) == len(missing_upload_indices)
    assert included[0]["start"] == ends[confirmed_index]
    assert included[-1]["end"] == stream.stat().st_size
    cursor = ends[confirmed_index]
    recovered = bytearray(stream.read_bytes()[:cursor])
    for chunk in sorted(included, key=lambda c: c["start"]):
        part = (final_bundle / chunk["name"]).read_bytes()
        assert chunk["start"] == cursor
        assert hashlib.sha256(part).hexdigest() == chunk["sha256"]
        assert len(part) == chunk["bytes"] == chunk["end"] - chunk["start"]
        recovered.extend(part)
        cursor = chunk["end"]
    assert bytes(recovered) == stream.read_bytes()
    assert task.verdict(root) == 1  # Raw bytes never become measurement success.
    result = {
        "fixture": name, "synthetic_only": True, "upload18_success": upload18_success,
        "checkpoint19_remote_upload_performed": False,
        "last_acknowledged_checkpoint_before_final_upload": confirmed_index,
        "last_acknowledged_prefix_bytes": ends[confirmed_index],
        "final_pending_chunk_indices": missing_upload_indices,
        "final_pending_chunk_count": len(included),
        "reconstructs_exact_original_stream_if_final_upload_succeeds": True,
        "original_stream_sha256": hashlib.sha256(stream.read_bytes()).hexdigest(),
        "original_stream_bytes": stream.stat().st_size,
        "final_manifest_sha256": task.core.sha(final_bundle / "manifest.json"),
        "state_has_no_fabricated_19_or_20_ack": True,
        "runner_loss_or_final_upload_failure_keeps_last_confirmed_boundary": confirmed_index,
        "tail_after_confirmed_remote_boundary_is_unknown_not_zero": True,
        "raw_only_verdict": 1, "native_EOF_claimed": False,
    }
    write_json(root / "fixture-result.json", result)
    return result


def main():
    custody = json.loads((HERE / "source-custody.json").read_text())
    before = {}
    for row in custody["files"]:
        raw = (HERE / row["path"]).read_bytes()
        assert git_blob(raw) == row["git_blob"]
        assert hashlib.sha256(raw).hexdigest() == row["sha256"]
        before[row["path"]] = raw
    old = yaml.safe_load((HERE / "main-p8-task-profile.yml").read_text())
    new = yaml.safe_load((HERE / "source/.github/workflows/p8-task-profile.yml").read_text())
    expected = copy.deepcopy(old)
    steps = expected["jobs"]["measure"]["steps"]
    removed = [s for s in steps if s.get("id") == "upload_19"]
    assert len(removed) == 1
    steps[:] = [s for s in steps if s.get("id") != "upload_19"]
    finish = next(s for s in steps if s.get("id") == "finish")
    removed_finish_env = finish.pop("env")
    assert set(removed_finish_env) == {"PREVIOUS_ARTIFACT_ID", "PREVIOUS_ARTIFACT_DIGEST"}
    assert all("upload_19" in v for v in removed_finish_env.values())
    assert new == expected
    cells = new["jobs"]["measure"]["strategy"]["matrix"]["include"]
    measure_steps = new["jobs"]["measure"]["steps"]
    uploads = [s for s in measure_steps if s.get("uses", "").startswith("actions/upload-artifact@")]
    other_uploads = sum(1 for job_id, job in new["jobs"].items() if job_id != "measure"
                        for step in job["steps"] if step.get("uses", "").startswith("actions/upload-artifact@"))
    assert len(cells) == 45 and len(uploads) == 22 and other_uploads == 2
    freeze19 = next(s for s in measure_steps if s.get("id") == "freeze_19")
    assert all("upload_18" in v for v in freeze19["env"].values())
    results = [fixture("upload18-success-final-carries19and20", True),
               fixture("upload18-failed-final-carries18to20", False)]
    for name, raw in before.items():
        assert (HERE / name).read_bytes() == raw
    record = {
        "schema": "independent_p8_checkpoint19_transport_review_v1", "source": P,
        "reviewer": "/root/acceptance_audit", "recorded_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "decision": "accepted_exact_scoped_workflow_change_pending_source_admission_and_CI",
        "parsed_workflow_change_exact": ["remove upload_19", "remove finish upload_19 ACK environment"],
        "checkpoint19_still_executes_and_acknowledges18": True,
        "registry_N45cells85records_native_raw_gate_algorithms_unchanged": True,
        "artifact_upper_bound_this_workflow_attempt": {"cells": 45, "per_cell": 22, "build": 1, "aggregate": 1, "total": 992},
        "additional_run_artifacts_and_rerun_attempts_must_be_counted_separately": True,
        "synthetic_results": results, "executed_code_bytes_unchanged": True,
        "new_measurements": 0, "Rust_or_native_executions": 0, "provider_calls": 0, "network_calls": 0,
        "original_source_validator_executions": 0, "CI_execution_claim": False,
        "guard_semantics_changed": False, "task_completion_credit": 0, "release_certified": False,
        "limitations": ["Fixture uses explicitly synthetic artifact IDs and digest tokens, not actual GitHub uploads.",
                        "Final bundle byte completeness is distinct from successful remote upload or native measurement acceptance.",
                        "This review does not create or approve the subsequent independent admission record; root coordinates exact P/R/G and real CI."]
    }
    write_json(HERE / "independent-workflow-custody-review.json", record)
    print(json.dumps(record, ensure_ascii=False))


if __name__ == "__main__":
    main()
