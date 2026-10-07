#!/usr/bin/env python3
"""Execute both fixed approval chains before admitting their reviewed merge.

The v11 chain and main's joint22 v10 chain remain independent and byte-frozen.
Only an immutable independent review can reconcile their four differing inputs;
the current checkout supplies neither authorization nor a replacement manifest.
Historical runtime, quality, cold-build and release claims keep their old scope.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

import verify_reviewed_source_v11 as previous
import verify_reviewed_source_joint22_v10 as joint

ROOT = Path(__file__).resolve().parents[1]
VERSION = "p8-joint-source-20261008-v12"
BASE = "65dd32934b3f8cb3ff4f5f5deb154a431a8c09e3"
JOINT_BASE = "d77a2143cdb82e722b1d1c62851298c43e707b65"
PRODUCT = "8e12c3884edbb2743eb2aee82fafa285e8b28ef7"
REVIEW = "3056a14ccc3496b4e5c9ff1e3746bf6cf33a1b95"
PREVIOUS_SNAPSHOT = "2a75e65d01a3722155e7a1858d7e0e9c8a5558cf"
JOINT_SNAPSHOT = "d53a4972af92fd10a5cddb9f15ffdf06414b3d54"
REVIEW_PATH = "artifacts/checkpoints/p8-next-ten-20261008/release/main-merge-source-review.json"
LOAD_PATH = "crates/cc-eval/src/benchmark/p8_load.rs"
JOINT_OVERRIDES = frozenset({
    LOAD_PATH,
    "crates/cc-eval/src/benchmark/statistics.rs",
    "crates/cc-eval/tests/p8_measurements.rs",
    "crates/cc-eval/tests/p7_worker_contention.rs",
})
REGISTRY = ROOT / "scripts/reviewed-source-registry-v12.json"
REGISTRY_SHA256 = "a600301187ac43276122186732bc0e5c3dd55952dcc5e33ecc5d3e2c244e67d4"
JOINT_REGISTRY_PATH = "scripts/reviewed-source-joint22-v10.json"
JOINT_FIXTURE_ROOT = "scripts/source_snapshots/joint22_v10"
JOINT_TEST_PATH = JOINT_FIXTURE_ROOT + "/test_reviewed_source.py.txt"
SNAPSHOTS = {
    "scripts/verify_reviewed_source_v11.py":
        (PREVIOUS_SNAPSHOT, "scripts/verify_reviewed_source_v11.py"),
    "scripts/reviewed-source-registry-v11.json":
        (PREVIOUS_SNAPSHOT, "scripts/reviewed-source-registry-v11.json"),
    "scripts/verify_reviewed_source_joint22_v10.py":
        (JOINT_SNAPSHOT, "scripts/verify_reviewed_source.py"),
    JOINT_REGISTRY_PATH:
        (JOINT_SNAPSHOT, "scripts/reviewed-source-registry.json"),
    JOINT_TEST_PATH:
        (JOINT_SNAPSHOT, "tests/source_integrity/test_reviewed_source.py"),
    JOINT_FIXTURE_ROOT + "/.github/workflows/ci.yml":
        (JOINT_SNAPSHOT, ".github/workflows/ci.yml"),
}
git = previous.git
require = previous.require
changed_paths = joint.previous.v2.changed_paths


def identities():
    return dict(schema_version=7, source_version=VERSION,
                previous_source_version=previous.VERSION,
                joint_source_version=joint.VERSION,
                base_source=BASE, joint_base_source=JOINT_BASE,
                product_source=PRODUCT, review_source=REVIEW,
                previous_snapshot=PREVIOUS_SNAPSHOT, joint_snapshot=JOINT_SNAPSHOT,
                scope="source_integrity_and_reviewed_merge_delta_only",
                quality_and_100k="not_inherited", product_behavior_change=False)


def verify_pins():
    require(all(isinstance(pin, str) and re.fullmatch(r"[0-9a-f]{40}", pin)
                for pin in (BASE, JOINT_BASE, PRODUCT, REVIEW,
                            PREVIOUS_SNAPSHOT, JOINT_SNAPSHOT)),
            "v12 pins must be immutable full commit SHAs")
    require(BASE == previous.PRODUCT, "v12 base must be the proven v11 product")
    require(JOINT_BASE == joint.PRODUCT, "v12 joint base must be the proven joint22 product")


def check_identity(registry):
    verify_pins()
    for key, value in identities().items():
        require(registry.get(key) == value, "wrong v12 identity: " + key)


def load_registry(path=REGISTRY):
    verify_pins()
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == REGISTRY_SHA256,
            "stale or altered v12 registry")
    registry = json.loads(raw)
    check_identity(registry)
    return registry


def verify_snapshots(root=ROOT):
    verify_pins()
    git.ensure_refs([BASE, JOINT_BASE, PRODUCT, REVIEW, PREVIOUS_SNAPSHOT, JOINT_SNAPSHOT])
    for name, (ref, original) in SNAPSHOTS.items():
        target = root / name
        require(not target.is_symlink() and target.read_bytes() == git.blob(ref, original),
                "v12 approval snapshot changed: " + name)


def verify_joint_ci_snapshot(root=ROOT):
    # The unchanged joint guard has an explicit root interface. Its historical
    # workflow is actually verified in this small fixed snapshot, never patched
    # into the current workflow and never approved by a hash-only bypass.
    joint.verify_ci(root=root / JOINT_FIXTURE_ROOT)


def reconstruct(registry, inherited, joint_inherited, root=ROOT):
    """Bind both proven inventories to the exact independently reviewed result."""
    check_identity(registry)
    for source, expected, label in ((BASE, inherited, "v11"),
                                    (JOINT_BASE, joint_inherited, "joint22")):
        require(set(expected) == git.inputs(source)
                and all(raw == git.blob(source, name) for name, raw in expected.items()),
                label + " inherited bytes differ")
    changes = changed_paths(BASE, PRODUCT)
    joint_changes = changed_paths(JOINT_BASE, PRODUCT)
    require(changes == {LOAD_PATH} == set(registry.get("delta", {})),
            "v12 must change exactly the reviewed load test file from v11")
    require(joint_changes == JOINT_OVERRIDES == set(registry.get("joint_overrides", {})),
            "v12 joint override inventory differs")
    raw_review = git.blob(REVIEW, REVIEW_PATH)
    require(git.sha(raw_review) == registry.get("review_sha256"), "v12 review digest differs")
    target = root / REVIEW_PATH
    require(not target.is_symlink() and target.read_bytes() == raw_review,
            "v12 fixed review record changed")
    review = json.loads(raw_review)
    require(review.get("source") == PRODUCT and review.get("base") == BASE
            and review.get("joint_base") == JOINT_BASE
            and review.get("verdict") == "accepted_scoped"
            and set(review.get("paths", {})) == changes
            and set(review.get("joint_paths", {})) == joint_changes,
            "v12 review does not accept both fixed source deltas")
    result = dict(inherited)
    for paths, rows, reviewed, before, label in (
            (changes, registry["delta"], review["paths"], inherited, "v11"),
            (joint_changes, registry["joint_overrides"], review["joint_paths"],
             joint_inherited, "joint22")):
        for name in sorted(paths):
            row = rows[name]
            after = git.blob(PRODUCT, name)
            require(row.get("before_sha256") == git.sha(before[name]),
                    label + " before digest differs: " + name)
            require(row.get("sha256") == git.sha(after),
                    label + " after digest differs: " + name)
            require(all(reviewed[name].get(key) == row[key]
                        for key in ("before_sha256", "sha256")),
                    label + " independent review delta differs: " + name)
            result[name] = after
    require(all(result.get(name) == raw for name, raw in joint_inherited.items()
                if name not in joint_changes),
            "joint22 adopted bytes differ outside reviewed overrides")
    require(len(result) == 785 and set(result) == git.inputs(PRODUCT),
            "v12 complete input inventory differs")
    require({name: git.sha(raw) for name, raw in result.items()}
            == registry.get("complete_inputs"), "v12 complete manifest differs")
    require(all(raw == git.blob(PRODUCT, name) for name, raw in result.items()),
            "v12 complete source bytes differ")
    return result


def approved_union(registry, root=ROOT):
    verify_snapshots(root)
    inherited = previous.approved_union(previous.load_registry(), root=root)
    # This explicit registry path is essential: the unchanged alias retains its
    # historical canonical default, which belongs to our older frozen guard.
    joint_registry = joint.load_registry(root / JOINT_REGISTRY_PATH)
    joint_inherited = joint.approved_union(joint_registry, root=root)
    verify_joint_ci_snapshot(root)
    return reconstruct(registry, inherited, joint_inherited, root=root)


def expected_ci():
    original = previous.expected_ci()
    old = "verify_reviewed_source_v11.py --source-version " + previous.VERSION
    new = "verify_reviewed_source_v12.py --source-version " + VERSION
    require(original.count(old) == 1, "v11 CI selector differs")
    return original.replace(old, new)


def verify_ci(root=ROOT):
    require((root / ".github/workflows/ci.yml").read_text() == expected_ci(),
            "CI differs beyond the explicit v12 selector")
    require((root / ".github/workflows/p7-engineering.yml").read_bytes()
            == git.blob(previous.previous.P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"),
            "P7 engineering workflow changed")


def main():
    if sys.flags.optimize:
        raise SystemExit("source verification requires Python without -O")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-version", required=True, choices=[VERSION])
    parser.parse_args()
    expected = approved_union(load_registry())
    tracked = git.git("ls-files", "--", "crates", "Cargo.toml", "Cargo.lock")
    git.verify_tree(ROOT, expected, tracked.decode().splitlines())
    verify_ci()
    print(json.dumps(dict(identities(), status="passed", complete_inputs=len(expected),
                          executed_proofs=[previous.VERSION, joint.VERSION],
                          previous_delta_inputs=1, joint_override_inputs=len(JOINT_OVERRIDES))))


if __name__ == "__main__":
    main()
