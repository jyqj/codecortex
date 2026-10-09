#!/usr/bin/env python3
"""Compare immutable source-admission inputs without importing or running guards."""
import argparse
import ast
import collections
import datetime
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    head = "275e8799d4947d297329073eaa3ca675d3fd0777"
    checkout = "d53ea0be17a53d1354ee1d6f0a111c5e588e91f5"

    def git(*words):
        return subprocess.check_output(["git", *words], cwd=args.repo)

    def content(ref, path):
        return git("show", ref + ":" + path)

    def tree(ref):
        out = {}
        for row in git("ls-tree", "-r", ref).decode().splitlines():
            meta, path = row.split("\t", 1)
            mode, kind, oid = meta.split()
            out[path] = {"mode": mode, "kind": kind, "git_blob": oid}
        return out

    def literals(source):
        out = {}
        for node in ast.parse(source).body:
            if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
                try:
                    out[node.targets[0].id] = ast.literal_eval(node.value)
                except (ValueError, TypeError):
                    pass
        return out

    a, b = tree(head), tree(checkout)
    ga, gb = {p: v for p, v in a.items() if p.startswith("artifacts/")}, {p: v for p, v in b.items() if p.startswith("artifacts/")}
    deleted = sorted(set(ga) - set(gb))
    changed = sorted(p for p in set(ga) & set(gb) if ga[p] != gb[p])
    added = sorted(set(gb) - set(ga))
    assert not deleted and not changed and len(added) == 443
    assert all(p.startswith("artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/") for p in added)
    guard_path = "scripts/verify_reviewed_source_v15.py"
    history_path = "scripts/v15_historical_context.py"
    guard, history = literals(content(head, guard_path)), literals(content(head, history_path))
    historical_head = history["HISTORICAL_HEAD"]
    assert historical_head == guard["BASE"] == "7354db236c9d9850a75f31672697ae9eab44565e"
    historic_tree = tree(historical_head)
    records = set(history["EXPLICIT_RECORDS"])
    registry_sources = []
    for path in history["REGISTRIES"]:
        raw = content(historical_head, path)
        value = json.loads(raw)
        selected = set(value.get("records", {})) | {row["review_path"] for row in value.get("deltas", {}).values()}
        records.update(selected)
        registry_sources.append({"path": path, "source": historical_head, "git_blob": historic_tree[path]["git_blob"],
                                 "sha256": hashlib.sha256(raw).hexdigest(), "record_path_count": len(selected),
                                 "record_paths_sha256": hashlib.sha256(("\n".join(sorted(selected)) + "\n").encode()).hexdigest()})
    validation_names = {p for p in historic_tree if p.startswith(tuple(x + "/" for x in history["VALIDATION_ROOTS"]))}
    names = validation_names | records
    protected = records | {path for path in names if (
        path.startswith("tests/source_integrity/") or path.startswith("scripts/source_snapshots/")
        or (path.startswith("scripts/") and Path(path).name.startswith(("verify_", "current-source-", "reviewed-source-", "v14_historical_")))
        or path == "scripts/p0_historical_corpus.py")}
    assert len(names) <= history["MAX_FILES"]
    assert all(p in a and p in b and a[p] == b[p] for p in protected)
    assert all(a[p] == historic_tree[p] for p in protected)
    canonical = guard["REVIEW_PATH"]
    review_tree = tree(guard["REVIEW"])
    assert a[canonical] == b[canonical] == review_tree[canonical]
    registry_raw = content(head, "scripts/reviewed-source-registry-v15.json")
    registry = json.loads(registry_raw)
    canonical_raw = content(head, canonical)
    assert hashlib.sha256(canonical_raw).hexdigest() == registry["review_sha256"]
    consumed_artifacts = sorted({p for p in protected if p.startswith("artifacts/")} | {canonical})
    assert not (set(consumed_artifacts) & set(added))
    artifact_manifest = [{"path": p, **a[p]} for p in sorted(ga)]
    raw_manifest = json.dumps(artifact_manifest, sort_keys=True, separators=(",", ":")).encode()
    output = {"schema_version": 1, "status": "accepted_same_source_admission_consumption",
        "reviewed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(), "reviewer": "/root/scale_audit",
        "repository": "jyqj/codecortex", "fixed_head": head, "actual_ci_checkout": checkout,
        "original_ci_run_id": 37890756949, "original_v15_log_line": 5897,
        "original_ci_recheck": {"path": "round6-g275-ci-recheck.json", "sha256": "442d42557e22e89e6e1156f97fb928032e3225ee0d520b665a309814acff0a9b"},
        "complete_artifact_tree_relation": {"head_files": len(ga), "checkout_files": len(gb), "head_files_all_same_mode_type_blob_in_checkout": True,
             "modified": 0, "deleted": 0, "added": len(added), "added_prefix": "artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/",
             "head_artifact_object_manifest_sha256": hashlib.sha256(raw_manifest).hexdigest(),
             "added_paths_sha256": hashlib.sha256(("\n".join(added) + "\n").encode()).hexdigest()},
        "guard_and_context": [{"path": p, **a[p], "same_in_checkout": a[p] == b[p],
             "url": "https://github.com/jyqj/codecortex/blob/" + head + "/" + p} for p in (guard_path, history_path)],
        "context_identity": {"historical_head": historical_head, "fixed_catalogue_paths": len(names), "current_protected_paths": len(protected),
             "all_current_protected_same_as_historical_and_checkout": True,
             "validation_roots": list(history["VALIDATION_ROOTS"]), "explicit_record_paths": list(history["EXPLICIT_RECORDS"])},
        "historical_registry_inputs": registry_sources,
        "consumed_artifact_paths": [{"path": p, **a[p]} for p in consumed_artifacts],
        "consumed_artifact_comparison": "Every listed path/mode/type/blob is identical in head, checkout, and its historical source (or the review commit for the current canonical review).",
        "canonical_review": {"path": canonical, "review_commit": guard["REVIEW"], "sha256": hashlib.sha256(canonical_raw).hexdigest(),
             "matches_registry_sha256": True},
        "reasoning": [
             "The v15 current-root artifact inputs are the fixed independent review path and v15_historical_context protected records.",
             "The historical catalogue is reconstructed from fixed historical Git validation roots, explicit records, and six named registry record/review paths. It does not scan current artifacts for extra files.",
             "The unchanged prior v14 approval is run in a pristine historical context built from that fixed catalogue; the current-root additions are not copied into it.",
             "All protected current-root paths and the current canonical review match both Git sources. All other existing G275 artifact blobs also remain identical; the only443 additions are outside the consumed records.",
             "Together with the complete12-root and1228 product/validation path bridge, this permits attribution of the original d53 source-admission and regression result to the identical G275 source domains without renaming the original checkout."],
        "limits": ["Static path and immutable Git object comparison only; no import/execution of guard code, no CI or product rerun.",
             "The source verifier does not grant runtime, scale, quality or release acceptance.",
             "Original CI identity remains d53ea0be; this is an explicit consumption-equivalence bridge, not a new G275 execution."]}
    args.output.write_text(json.dumps(output, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"status": output["status"], "path": str(args.output), "bytes": args.output.stat().st_size,
                      "sha256": hashlib.sha256(args.output.read_bytes()).hexdigest(), "consumed_artifact_paths": len(consumed_artifacts),
                      "protected_paths": len(protected), "existing_artifact_files": len(ga)}))


if __name__ == "__main__":
    main()
