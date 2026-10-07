#!/usr/bin/env python3
"""Verify archived evidence bytes; this does not rerun Rust or approve new source."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, help="optionally verify the fixed 769 source inputs")
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    index = json.loads((here / "archive-index.json").read_text())
    archive = here / index["archive"]["path"]
    assert digest(archive.read_bytes()) == index["archive"]["sha256"]
    assert archive.stat().st_size == index["archive"]["size_bytes"]
    blobs = {}
    with tarfile.open(archive, "r:gz") as handle:
        assert len(handle.getmembers()) == len(index["archive"]["members"])
        for member in handle.getmembers():
            assert member.isfile() and member.name not in blobs
            expected = index["archive"]["members"][member.name]
            data = handle.extractfile(member).read()
            assert len(data) == expected["size_bytes"] == member.size
            assert digest(data) == expected["sha256"]
            blobs[member.name] = data
    for name, expected in index["direct_originals"].items():
        data = (here / name).read_bytes()
        assert digest(data) == expected["sha256"]
        assert len(data) == expected["size_bytes"]

    current = json.loads((here / "source-sha256.json").read_text())
    assert len(current) == 769
    fixture = "crates/cc-server/tests/p7_dense_artifact_coverage.rs"
    original = json.loads(blobs["baseline-source-sha256.json"])
    baseline = json.loads(blobs["baseline-v2-source-sha256.json"])
    assert original.keys() == baseline.keys() == current.keys()
    assert {name for name in current if original[name] != baseline[name]} == {fixture}
    changed_production = {
        "crates/cc-semantic/src/vector/exact.rs": "exact.rs",
        "crates/cc-server/src/semantic_wiring.rs": "semantic_wiring.rs",
    }
    assert {name for name in current if baseline[name] != current[name]} == set(changed_production)
    assert digest(blobs["attempt-sources/initial-compile/p7_dense_artifact_coverage.rs"]) == original[fixture]
    for name, basename in changed_production.items():
        assert digest(blobs["attempt-sources/baseline/" + basename]) == baseline[name]

    behavior = {
        "baseline-v2": (2, 2, 0),
        "fixed-behavior": (4, 0, 0),
        "exact-regression": (21, 0, 0),
        "scope-regression": (13, 0, 0),
        "artifact-regression": (11, 0, 0),
        "fusion-acceptance": (10, 0, 0),
    }
    for name in index["direct_originals"]:
        if not name.endswith("-receipt.json"):
            continue
        receipt = json.loads((here / name).read_text())
        label = receipt["label"]
        log = blobs[label + ".log"]
        manifest = blobs[label + "-source-sha256.json"]
        assert digest(log) == receipt["log_sha256"]
        assert digest(manifest) == receipt["source_manifest_sha256"]
        assert receipt["source_count"] == 769 and receipt["source_unchanged"]
        assert receipt["exit_code"] == (101 if label in {"baseline", "baseline-v2"} else 0)
        if label not in {"baseline", "baseline-v2"}:
            assert receipt["source_commit"] == index["source"]
            assert json.loads(manifest) == current
        summaries = re.findall(
            r"^test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;",
            log.decode(), re.MULTILINE,
        )
        if label in behavior:
            assert tuple(sum(int(row[i]) for row in summaries) for i in range(3)) == behavior[label]
        else:
            assert not summaries
    assert b"error[E0624]" in blobs["baseline.log"]
    if args.source_root is not None:
        for name, expected in current.items():
            relative = PurePosixPath(name)
            assert not relative.is_absolute() and ".." not in relative.parts
            assert name in {"Cargo.toml", "Cargo.lock"} or relative.parts[0] == "crates"
            assert digest((args.source_root / name).read_bytes()) == expected, name
    print(json.dumps({
        "evidence_valid": True,
        "archive_members": len(blobs),
        "direct_originals": len(index["direct_originals"]),
        "fixed_source": index["source"],
        "fixed_tests": {"passed": 59, "failed": 0, "ignored": 0},
        "source_inputs_checked": 769 if args.source_root is not None else 0,
        "rust_rerun": False,
    }, sort_keys=True))


if __name__ == "__main__":
    main()
