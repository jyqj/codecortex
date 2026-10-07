#!/usr/bin/env python3
"""Compose the fixed P7/P8 approvals and one independently reviewed integration.

Historical guards and their overlap rejection remain unchanged. A sequential
delta binds the merged CLI and subsequent fixes to an immutable review; the
current checkout can never authorize its own source bytes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import sys

import verify_reviewed_source as p8
import verify_reviewed_source_p7_v9 as p7

ROOT = Path(__file__).resolve().parents[1]
VERSION = "p8-next-ten-20261008-v10"
P8_SNAPSHOT = "ae906513886bef4501d0a1d2ecffd68ef76c3f5d"
P7_SNAPSHOT = "eb7cdc55aa94c8d6865bed14fa37fff08080af33"
BASE = "18499879a8ba3197e599cfd2e9ae96bcb657d65b"
PRODUCT = "78ae91eeae6edae6bea29c27f24b251773341c00"
REVIEW = "7f992a0e328aa1f088746dca360d7d5302e2f2e7"
REVIEW_PATH = "artifacts/checkpoints/p8-next-ten-20261008/source-review.json"
REGISTRY = ROOT / "scripts/reviewed-source-registry-v10.json"
REGISTRY_SHA256 = "099617f589ff6d2acfeb93b58b8049ee3eac9d0c6636cf14eaa524873b838708"
P7_REGISTRY = ROOT / "scripts/reviewed-source-p7-v9.json"
P7_OVERRIDES = frozenset({"crates/cc-eval/src/bin/cc-eval.rs"})
SNAPSHOTS = {
    "scripts/verify_reviewed_source.py": (P8_SNAPSHOT, "scripts/verify_reviewed_source.py"),
    "scripts/reviewed-source-registry.json": (P8_SNAPSHOT, "scripts/reviewed-source-registry.json"),
    "scripts/verify_reviewed_source_p7_v9.py": (P7_SNAPSHOT, "scripts/verify_reviewed_source.py"),
    "scripts/reviewed-source-p7-v9.json": (P7_SNAPSHOT, "scripts/reviewed-source-registry.json"),
}
git = p8.previous.v2.v1
require = p8.require


def identities():
    return {
        "schema_version": 5,
        "source_version": VERSION,
        "base_source": BASE,
        "product_source": PRODUCT,
        "review_source": REVIEW,
        "p7_snapshot": P7_SNAPSHOT,
        "p8_snapshot": P8_SNAPSHOT,
        "scope": "source_integrity_only",
        "quality_and_100k": "not_inherited",
    }


def verify_pins():
    require(all(isinstance(pin, str) and re.fullmatch(r"[0-9a-f]{40}", pin)
                for pin in [P8_SNAPSHOT, P7_SNAPSHOT, BASE, PRODUCT, REVIEW]),
            "source pins must be immutable full commit SHAs")
    require(BASE == p8.PRODUCT, "sequential base must be the proven P8 product")


def load_registry(path=REGISTRY):
    verify_pins()
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == REGISTRY_SHA256,
            "stale or altered v10 registry")
    data = json.loads(raw)
    for key, value in identities().items():
        require(data.get(key) == value, "wrong v10 identity: " + key)
    return data


def verify_snapshots(root=ROOT):
    git.ensure_refs([P8_SNAPSHOT, P7_SNAPSHOT, BASE, PRODUCT, REVIEW])
    for path, (ref, original) in SNAPSHOTS.items():
        target = root / path
        require(not target.is_symlink() and target.read_bytes() == git.blob(ref, original),
                "historical approval snapshot changed: " + path)


def reconstruct(registry, expected_p8, expected_p7, root=ROOT):
    """Apply only the exact reviewed sequence after both old proofs succeed."""
    verify_pins()
    for key, value in identities().items():
        require(registry.get(key) == value, "wrong v10 identity: " + key)
    require(set(expected_p8) == git.inputs(BASE)
            and all(raw == git.blob(BASE, path) for path, raw in expected_p8.items()),
            "sequential base differs from the approved P8 bytes")
    review_raw = git.blob(REVIEW, REVIEW_PATH)
    require(git.sha(review_raw) == registry.get("review_sha256"), "review digest differs")
    target = root / REVIEW_PATH
    require(not target.is_symlink() and target.read_bytes() == review_raw,
            "fixed independent review record changed")
    review = json.loads(review_raw)
    require(review.get("source") == PRODUCT and review.get("base") == BASE
            and review.get("verdict") == "accepted_scoped",
            "independent review does not accept this fixed delta")
    changes = p8.previous.v2.changed_paths(BASE, PRODUCT)
    require(changes == set(registry.get("delta", {})), "sequential delta inventory differs")
    require(set(review.get("paths", {})) == changes, "review path inventory differs")
    expected = dict(expected_p8)
    product_inputs = git.inputs(PRODUCT)
    for path in sorted(changes):
        row = registry["delta"][path]
        before = expected.get(path)
        require(row.get("before_sha256") == (git.sha(before) if before is not None else None),
                "sequential delta before digest differs: " + path)
        after = git.blob(PRODUCT, path) if path in product_inputs else None
        require(row.get("sha256") == (git.sha(after) if after is not None else None),
                "sequential delta source digest differs: " + path)
        require(review["paths"][path].get("before_sha256") == row["before_sha256"]
                and review["paths"][path].get("sha256") == row["sha256"],
                "independent review delta digest differs: " + path)
        if after is None:
            expected.pop(path, None)
        else:
            expected[path] = after
    p7_changes = p8.previous.v2.changed_paths(p8.APPROVED["p8_local_engineering"]["base"], p7.PRODUCT)
    require(set(registry.get("p7_reviewed_overrides", [])) == P7_OVERRIDES,
            "P7 override inventory differs")
    require(P7_OVERRIDES <= changes & p7_changes, "P7 overrides must be explicitly reviewed changes")
    for path in sorted(p7_changes - P7_OVERRIDES):
        require(expected.get(path) == expected_p7.get(path),
                "P7 adopted bytes differ outside the reviewed override: " + path)
    require(set(expected) == product_inputs, "complete product inventory differs")
    require({path: git.sha(raw) for path, raw in expected.items()}
            == registry.get("complete_inputs"), "complete manifest differs")
    require(all(raw == git.blob(PRODUCT, path) for path, raw in expected.items()),
            "complete product source bytes differ")
    return expected


def approved_union(registry, root=ROOT):
    verify_snapshots(root)
    old_p8 = p8.approved_union(p8.load_registry(), root=root)
    old_p7 = p7.approved_union(p7.load_registry(P7_REGISTRY), root=root)
    return reconstruct(registry, old_p8, old_p7, root=root)


def expected_ci():
    git.ensure_refs([P7_SNAPSHOT])
    original = git.blob(P7_SNAPSHOT, ".github/workflows/ci.yml").decode()
    old = "verify_reviewed_source.py --source-version " + p7.VERSION
    new = "verify_reviewed_source_v10.py --source-version " + VERSION
    require(original.count(old) == 1, "historical CI selector differs")
    original = original.replace(old, new)
    anchor = ("          python3 scripts/code_index_plan.py\n"
              "          PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/source_integrity -v\n")
    require(original.count(anchor) == 1, "historical source check anchor differs")
    checks = ("          python3 scripts/code_index_plan.py\n"
              "          PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p 'test_p8_*.py' -v\n"
              "          python3 scripts/p8_facts.py --check\n"
              "          PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/source_integrity -v\n")
    return original.replace(anchor, checks)


def verify_ci(root=ROOT):
    require((root / ".github/workflows/ci.yml").read_text() == expected_ci(),
            "CI differs beyond reviewed P8 checks and explicit v10 selector")
    require((root / ".github/workflows/p7-engineering.yml").read_bytes()
            == git.blob(P7_SNAPSHOT, ".github/workflows/p7-engineering.yml"),
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
