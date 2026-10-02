#!/usr/bin/env python3
"""Read-only source/provenance checks; never run retrieval or rewrite gold."""
import hashlib
import json
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[3]
MANIFEST = ROOT / "crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json"
manifest = json.loads(MANIFEST.read_text())
queries_path = (MANIFEST.parent / manifest["queries"]).resolve()
queries = [json.loads(line) for line in queries_path.read_text().splitlines()]
assert len(queries) == 14
assert all(a["symbol"] is None and a["span"] is None
           for q in queries for answer in q["answers"] for a in answer["alternatives"])

def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT)

def sha(data):
    return hashlib.sha256(data).hexdigest()

dirty = "crates/cc-index/src/indexer_phases/dirty.rs"
removed = "0a56a257f9a92c54d06ea5be0ce1d1763917a527"
parent = git("rev-parse", removed + "^").decode().strip()
assert parent == "4514630dcd26481cf6dbc2aff38824ed71ef06da"
old = git("show", parent + ":" + dirty)
checkpoint = git("show", removed + ":" + dirty)
assert b"fn compute_fingerprint_for_unit" in old
assert b"fn compute_fingerprint_for_unit" not in checkpoint
assert b"compute_fingerprint_for_unit" not in (ROOT / dirty).read_bytes()
frozen = ROOT / ("artifacts/checkpoints/20261001-paused-github-sync/evidence/"
                 "artifacts/benchmarks/p0-g0-20260927/frozen-inputs/"
                 "p0-codecortex-subset/source/cc-index/src/indexer_phases/dirty.rs")
assert frozen.read_bytes() == old

checks = {
    "R01": ("cc-model/src/id.rs", r"pub fn symbol_uid\("),
    "R03": ("cc-parsers/src/chunker.rs", r"pub fn chunk_with_symbols\("),
    "R05": ("cc-db/src/index_migrate.rs", r"pub const CURRENT_SCHEMA_VERSION:"),
    "R07": ("cc-index/src/dirty_closure.rs", r"fn compute_dirty_closure<"),
    "R11": ("cc-search/src/preselect.rs", r"pub fn preselect_files\("),
    "R13": ("cc-search/src/scope.rs", r"fn chunk_scope\("),
}
symbol_checks = {}
for row, (path, pattern) in checks.items():
    lines = (ROOT / "crates" / path).read_text().splitlines()
    found = [i for i, line in enumerate(lines, 1) if re.search(pattern, line)]
    assert found, (row, path)
    symbol_checks[row] = {"path": path, "declaration_lines": found}
extra = ["cc-model/src/public_surface.rs", "cc-parsers/src/chunker/split.rs",
         "cc-parsers/src/chunker/budget.rs", "cc-model/src/chunk_policy.rs",
         "cc-eval/src/benchmark/metrics.rs", "cc-eval/src/benchmark/manifest.rs"]
for anchor in ["pub fn fingerprint(", "pub fn changed_from("]:
    assert anchor in (ROOT / "crates/cc-model/src/public_surface.rs").read_text()
assert "cc-model/src/public_surface.rs" not in manifest["source"]["files"]
result = {
    "scope": "14 current p0-codecortex-subset rows; source read, no retrieval",
    "source_checkpoint": "80a43e0ae9a2f640e51d101968d4fef87855c01c",
    "manifest_sha256": sha(MANIFEST.read_bytes()),
    "queries_sha256": sha(queries_path.read_bytes()),
    "source_lock": manifest["source"]["digest"],
    "query_lock": manifest["queries_digest"],
    "confirmed_current_source_invalid_rows": ["R09"],
    "retrieval_outcome": "NOT_MEASURED",
    "all_alternatives_are_file_only": True,
    "symbol_declaration_checks": symbol_checks,
    "R09": {
        "first_observable_committed_mismatch": removed,
        "parent": parent,
        "parent_and_frozen_source_sha256": sha(old),
        "historical_frozen_question_valid": True,
        "current_literal_helper_present": False,
        "current_entry": "cc-model/src/public_surface.rs:201,214",
        "current_consumer": "cc-index/src/indexer_phases/dirty.rs:70-74",
        "current_entry_admitted_to_corpus": False,
        "classification": "responsibility migration plus canonical surface semantics change",
    },
    "source_sha256": {path: sha((ROOT / "crates" / path).read_bytes())
                       for path in manifest["source"]["files"] + extra},
    "verification": "PASS: structural assertions only; manual semantic findings in audit.md",
}
assert result["queries_sha256"] == "728e17bfa3061d627fbd77cdb668a38728532dd1bbfb433d1de36bf0225649a1"
print(json.dumps(result, indent=2, sort_keys=True))
