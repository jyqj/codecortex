#!/usr/bin/env python3
"""Bind completed, independent follow-up reviews to immutable published P3.

No test, benchmark, task completion or release approval is inferred from hashes.
The prior accepted source review remains available at its original Git commit.
"""
import hashlib
import json
from pathlib import Path
import subprocess

from inventory_git_inputs import inventory

HERE = Path(__file__).resolve().parent
WORK = HERE.parent
REPO = WORK / "codecortex"
BASE = "7354db236c9d9850a75f31672697ae9eab44565e"
P2 = "7e4e4fefb8a5302b90579a41f4d334fe176aeb3f"
R = "32674879181b5fd6e8117167eab38922a7f1187d"
G = "bb9a96d71622458c39a143055360cc97f0d11d78"
P3 = "b11addeed93b02c7b840bd3b61a452ce0e671287"
LOCAL = "d0fe84a3e3f556c4f5907cc62190b20902a9f84d"
REVIEW_PATH = "artifacts/checkpoints/p8-completion-20261009/independent-source-review.json"
PRIOR_SHA = "8047a10a1fa2aa1e80fd5227f7140635c95a858edf313a0b6630a541e486fabc"
ARCHIVE_PATH = "artifacts/checkpoints/p8-completion-20261009/followup-engineering"


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(["git", "--no-pager", "-C", str(REPO), *args])


def checked_json(relative, digest):
    raw = (WORK / relative).read_bytes()
    assert sha(raw) == digest, relative
    return json.loads(raw)


def main():
    old_raw = git("show", R + ":" + REVIEW_PATH)
    assert sha(old_raw) == PRIOR_SHA
    old = json.loads(old_raw)
    assert old["source"] == P2 and old["verdict"] == "accepted_scoped"
    current = inventory(REPO, P3)
    local = inventory(REPO, LOCAL)
    assert current["source_tree"] == local["source_tree"] == "eaabe503cf1439894326c94ab0f964a1e3fad7f8"
    for key in ("complete_inputs", "paths", "source_modes", "validation_modes"):
        assert current[key] == old[key], key
    assert current["complete_input_count"] == 1087
    assert current["changed_source_input_count"] == 22
    assert current["validation_input_count"] == 135
    changed = {p for p, digest in current["validation_inputs"].items()
               if digest != old["validation_inputs"][p]}
    assert changed == {
        "scripts/p8_backfill.py", "scripts/p8_runtime.py", "scripts/p8_cold_build.py",
        "scripts/tests/test_p8_runtime.py", "scripts/tests/test_p8_platform.py"}
    assert set(git("diff", "--name-only", G, P3).decode().splitlines()) == changed
    for path in ("scripts/verify_reviewed_source_v15.py", "scripts/reviewed-source-registry-v15.json",
                 ".github/workflows/ci.yml"):
        assert git("show", P3 + ":" + path) == git("show", G + ":" + path), path
    composite_path = "p8-followup-independent-review-d0fe84a3.json"
    composite_sha = "1de80493cf51929ed3614ae573ef057ecbcc8b3d7f529fa26256ea6dd4684ee4"
    composite = checked_json(composite_path, composite_sha)
    assert composite["status"] == "accepted_scoped_followup_delta"
    assert composite["commit"] == LOCAL
    assert composite["changed_paths"] == {p: current["validation_inputs"][p] for p in changed}
    references = {
        "runtime_author_controls": ("p8-runtime-followup-controls/validation-receipt.json", "067c35fa421d75b0a67273fc27ba575b6f38568611b8d66e03f02b661794a681"),
        "collector_author_controls": ("p8-collector-followup-controls/validation-receipt.json", "806bde18f3004e23ff6dab5c2115cbc3f07848448a9dbdd4c4720076d3115d66"),
        "runtime_independent_review": ("p8-runtime-independent-review-d5d4d570/audit.json", "ddea53a0e5f50ab16cfda6ef1ec458f957e2d2e3dd1f6d3584c4aa3c1be16f5c"),
        "collector_independent_review": ("p8-collector-independent-review-d0fe84a3/audit.json", "a021e9734b8c3f0e50aab7666d40f0896680cfa61546f8c8f069463ad01ae182"),
        "backfill_independent_review": ("p8-pr159-readonly-audit/backfill-88d44739-independent-review.json", "9b23749136656bdab106f0949f262ceb44a2a6e79a23b83c343ae339fae5a7c7"),
        "scale_input_invariance": ("p8-final-independent-source-review/scale-input-invariance-G-to-d0fe84a3.json", "1299572cc575df409200a222683c2d82f655d92664abc1e955ce1092f63aa9b1"),
        "followup_composite_review": (composite_path, composite_sha),
    }
    checked = {key: checked_json(path, digest) for key, (path, digest) in references.items()}
    assert checked["backfill_independent_review"]["verdict"] == "accepted_scoped"
    assert checked["backfill_independent_review"]["unresolved_blockers"] == []
    runtime = checked["runtime_author_controls"]
    collector = checked["collector_author_controls"]
    assert runtime["checks"][0]["tests"] == 356 and runtime["checks"][0]["exit_code"] == 0
    assert collector["checks"][0]["tests"] == 58 and collector["checks"][0]["exit_code"] == 0
    delivery = checked_json("p8-followup-engineering-archive-delivery.json", "721f1826edf1565c6cdbce8936711d9604077f1a1a6b4e78715bc2157f39cd12")
    for name, entry in delivery["files"].items():
        raw = (WORK / "p8-followup-engineering-archive" / name).read_bytes()
        assert len(raw) == entry["bytes"] and sha(raw) == entry["sha256"], name
    review = {key: current[key] for key in (
        "source", "source_tree", "base", "paths", "complete_inputs", "validation_inputs",
        "source_modes", "validation_modes", "complete_input_count", "changed_source_input_count",
        "validation_input_count", "heldout_corpus_bodies_read")}
    review.update(
        schema_version=1,
        verdict="accepted_scoped",
        scope="independently_reviewed_source_and_validation_inputs",
        independent_reviewers=["/root", "/root/pr_audit", "/root/acceptance_audit"],
        unresolved_blockers=[],
        local_reviewed_source=LOCAL,
        prior_accepted_review={"source": P2, "review_commit": R, "path": REVIEW_PATH,
                               "sha256": PRIOR_SHA, "complete_source_inputs_unchanged": True,
                               "validation_inputs_unchanged": 130,
                               "scope": "Prior independent review retained for byte-identical inputs only"},
        author_independence={
            "/root/pr_audit": "Prior non-author review of unchanged Rust/evaluator/scale/runtime/lifecycle/gates/approval inputs; independent actual Cargo feature-selection review of the one-line backfill change authored by root. Author of the new runtime/cold changes, therefore does not self-approve their semantics.",
            "/root/acceptance_audit": "Prior non-author platform/recovery/rollback source review retained. New runtime and cold-builder changes independently exported from fixed commits and reviewed with actual original/new Python controls and real executor threads; no product execution is claimed for protocol fakes.",
            "/root": "Non-author inspection of all runtime/cold deltas and preserved failed/passing raw controls, fixed 1087/135 Git inventories, unchanged original requirements, and archive bytes. The root-authored backfill flag is independently reviewed by pr_audit.",
        },
        validation_delta_from_prior={p: {"before_sha256": old["validation_inputs"][p],
                                          "sha256": current["validation_inputs"][p]}
                                     for p in sorted(changed)},
        findings=[
            "All 1087 crate/Cargo source inputs, their modes and all 22 BASE-to-product deltas are identical to the prior independently accepted P2/R source. Exactly five validation files change; the other 130 and all validation modes remain identical.",
            "Backfill adds only --no-default-features before the existing --features semantic. An actual dependency-free Cargo 1.95 release protocol control confirms the original default+semantic label and corrected semantic-only label. Exact product/release/package/source admission remains unchanged; the original failed G artifact is not relabeled.",
            "Runtime parity exit 1 retains the complete offered terminal denominator, statistics and summary while returning failure. Oracle infrastructure errors remain exit 2; neither outcome is turned into a successful endpoint-parity claim.",
            "Offering, worker and raw-write exceptions cancel queued work and stop owned workers, processes and samplers before sealing, using the original 180/70/35/60 bounds. Slots release even if terminal raw emission fails. Raw-budget exhaustion explicitly retains cancellation/retention errors and does not claim missing raw terminal rows exist.",
            "Unconfirmed owned writers produce explicit unsealed_owned_writers with null raw digest and no artifact seal. A nominal original deadline control retains 60 distinct terminal IDs, including 59 canceled queued operations.",
            "The cold collection failure wrapper writes an invalid receipt only after acquiring a newly owned output directory. Counts remain null and expected_cell_count remains eight; no producer execution count is invented. Existing output directories and symlinks remain untouched. Original collect_cells and new_directory semantics and success records remain unchanged.",
            "Actual original-plus-new Python controls passed 356 tests at d5d4d570 and 58 platform tests at d0fe84a3, separately; this is not a claim of one combined 359-test run at P3. Independent review additionally ran fixed exported scripts against the original/new runtime and cold controls. Earlier sparse-input failures, red reproductions and the superseded uncommitted receipt draft remain archived.",
            "Original scale production inputs and all executed scale observer/workflow inputs remain byte-identical to G. Separate G measurements may be reviewed under their true G identity, without relabeling their source or admitting absent shards, smaller N or different budgets.",
            "The complete original engineering package contains 2367 files and 13983069 original bytes; its portable verifier rechecked every member, mode and digest and the three-local-commit bundle. It preserves original successful, failed, incomplete and not-run observations.",
        ],
        review_evidence={key: {"original_relative_path": path, "sha256": digest,
                               "archive": ARCHIVE_PATH + "/engineering-originals.tar.gz"}
                         for key, (path, digest) in references.items()},
        engineering_archive={"path": ARCHIVE_PATH, "files": delivery["files"],
                             "original_files": 2367, "original_bytes": 13983069,
                             "bundle_prerequisite": G, "published_source_tree": current["source_tree"]},
        limitations=[
            "This accepts source and validation inputs only. It does not close any TODO or provide final runtime, quality, performance or release certification.",
            "All prior engineering and formal executions keep their actual source commits, binaries and observer identities. The original G backfill feature-admission failure remains a failure and requires a new successful product execution.",
            "The G full original CI pass and the local c709 split 139+42 coverage are distinct executions. No archived result is relabeled as a P3 execution.",
            "New product/review/registry pins are installed in a subsequent commit and require an exact-diff review and actual unchanged previous/current proof. The original CI selector, proof behavior, thresholds and original source gates are not relaxed.",
            "G8/P8-020 complete release and heldout requirements remain independent from this source review; no heldout corpus bodies or paid provider calls are used here.",
        ],
    )
    out = HERE / "independent-source-review-P3.json"
    raw = (json.dumps(review, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()
    with out.open("xb") as stream:
        stream.write(raw)
    print(json.dumps({"output": str(out), "sha256": sha(raw), "source": P3,
                      "verdict": review["verdict"], "complete_inputs": 1087,
                      "validation_inputs": 135, "changed_validation_inputs": len(changed)}))


if __name__ == "__main__":
    main()
