#!/usr/bin/env python3
"""Inherit the complete v10 proof and admit one independently reviewed test fix."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

import verify_reviewed_source_v10 as previous

ROOT = Path(__file__).resolve().parents[1]
VERSION = "p8-worker-readiness-20261008-v11"
BASE = "78ae91eeae6edae6bea29c27f24b251773341c00"
PRODUCT = "65dd32934b3f8cb3ff4f5f5deb154a431a8c09e3"
REVIEW = "b64746f42750422dc38aa9bb73a6f2f2b43fdc2b"
PREVIOUS_SNAPSHOT = "48efa5a6058005fe141d7a835ea9f127db9bd28b"
REVIEW_PATH = "artifacts/checkpoints/p8-next-ten-20261008/ci/worker-source-review.json"
TEST_PATH = "crates/cc-eval/tests/p7_worker_contention.rs"
REGISTRY = ROOT / "scripts/reviewed-source-registry-v11.json"
REGISTRY_SHA256 = "ef0b1ed53c07e850b7a226195cb1bf4b4a48b705a9e4a9c5311b33e758d89d5e"
SNAPSHOTS = ("scripts/verify_reviewed_source_v10.py", "scripts/reviewed-source-registry-v10.json")
git = previous.git
require = previous.require


def identities():
    return dict(schema_version=6, source_version=VERSION, previous_source_version=previous.VERSION,
                base_source=BASE, product_source=PRODUCT, review_source=REVIEW,
                previous_snapshot=PREVIOUS_SNAPSHOT, scope="source_integrity_and_test_fixture_delta_only",
                quality_and_100k="not_inherited", product_behavior_change=False)


def verify_pins():
    require(all(isinstance(pin, str) and re.fullmatch(r"[0-9a-f]{40}", pin)
                for pin in (BASE, PRODUCT, REVIEW, PREVIOUS_SNAPSHOT)),
            "v11 pins must be immutable full commit SHAs")
    require(BASE == previous.PRODUCT, "v11 base must be the proven v10 product")


def load_registry(path=REGISTRY):
    verify_pins()
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == REGISTRY_SHA256, "stale or altered v11 registry")
    data = json.loads(raw)
    for key, value in identities().items():
        require(data.get(key) == value, "wrong v11 identity: " + key)
    return data


def verify_snapshots(root=ROOT):
    verify_pins()
    git.ensure_refs([BASE, PRODUCT, REVIEW, PREVIOUS_SNAPSHOT])
    for name in SNAPSHOTS:
        target = root / name
        require(not target.is_symlink() and target.read_bytes() == git.blob(PREVIOUS_SNAPSHOT, name),
                "v10 approval snapshot changed: " + name)


def reconstruct(registry, inherited, root=ROOT):
    verify_pins()
    for key, value in identities().items():
        require(registry.get(key) == value, "wrong v11 identity: " + key)
    require(set(inherited) == git.inputs(BASE)
            and all(raw == git.blob(BASE, name) for name, raw in inherited.items()),
            "v10 inherited bytes differ")
    changes = previous.p8.previous.v2.changed_paths(BASE, PRODUCT)
    require(changes == {TEST_PATH} == set(registry.get("delta", {})),
            "v11 must change exactly the reviewed worker test")
    raw_review = git.blob(REVIEW, REVIEW_PATH)
    require(git.sha(raw_review) == registry.get("review_sha256"), "v11 review digest differs")
    target = root / REVIEW_PATH
    require(not target.is_symlink() and target.read_bytes() == raw_review, "v11 fixed review record changed")
    review = json.loads(raw_review)
    require(review.get("source") == PRODUCT and review.get("base") == BASE
            and review.get("verdict") == "accepted_scoped" and set(review.get("paths", {})) == changes,
            "v11 review does not accept this fixed test delta")
    row = registry["delta"][TEST_PATH]
    after = git.blob(PRODUCT, TEST_PATH)
    require(row.get("before_sha256") == git.sha(inherited[TEST_PATH]), "v11 before digest differs")
    require(row.get("sha256") == git.sha(after), "v11 after digest differs")
    require(all(review["paths"][TEST_PATH].get(key) == row[key] for key in ("before_sha256", "sha256")),
            "v11 independent review delta differs")
    result = dict(inherited)
    result[TEST_PATH] = after
    require(set(result) == git.inputs(PRODUCT), "v11 complete input inventory differs")
    require({name: git.sha(raw) for name, raw in result.items()} == registry.get("complete_inputs"),
            "v11 complete manifest differs")
    require(all(raw == git.blob(PRODUCT, name) for name, raw in result.items()),
            "v11 complete source bytes differ")
    return result


def approved_union(registry, root=ROOT):
    verify_snapshots(root)
    inherited = previous.approved_union(previous.load_registry(), root=root)
    return reconstruct(registry, inherited, root=root)


def expected_ci():
    original = previous.expected_ci()
    old = "verify_reviewed_source_v10.py --source-version " + previous.VERSION
    new = "verify_reviewed_source_v11.py --source-version " + VERSION
    require(original.count(old) == 1, "v10 CI selector differs")
    return original.replace(old, new)


def verify_ci(root=ROOT):
    require((root / ".github/workflows/ci.yml").read_text() == expected_ci(),
            "CI differs beyond the explicit v11 selector")
    require((root / ".github/workflows/p7-engineering.yml").read_bytes()
            == git.blob(previous.P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"),
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
    print(json.dumps(dict(identities(), status="passed", complete_inputs=len(expected))))


if __name__ == "__main__":
    main()
