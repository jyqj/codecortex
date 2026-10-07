#!/usr/bin/env python3
"""Re-run the P8 facts CLI on current declarations and isolated negative copies."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile

parser = argparse.ArgumentParser()
parser.add_argument("--root", type=Path, required=True)
args = parser.parse_args()
root = args.root.resolve()
script = root / "scripts/p8_facts.py"
document = "docs/roadmap/code-index-v2/P8-FACTS.md"
rows = []


def run(label, checkout, action, expected):
    command = [sys.executable, str(script), "--root", str(checkout), action]
    result = subprocess.run(command, capture_output=True, text=True, timeout=30)
    output = json.loads(result.stdout or result.stderr)
    rows.append({"case": label, "command": command, "exit_code": result.returncode,
                 "expected_exit_code": expected, "result": output})
    if result.returncode != expected:
        raise AssertionError(label + ": unexpected CLI result")
    return output


baseline = run("current_declarations_match_docs", root, "--check", 0)
inputs = list(baseline["input_sha256"]) + [document, "docs/ARCHITECTURE.md",
           "docs/CONFIGURATION.md", "docs/MCP_TOOLS.md"]
hashes_before = {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in inputs}
hashes_before["scripts/p8_facts.py"] = hashlib.sha256(script.read_bytes()).hexdigest()

with tempfile.TemporaryDirectory(prefix="p8-facts-audit-") as temporary:
    copy = Path(temporary)
    def reset():
        for relative in inputs:
            target = copy / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(root / relative, target)
    reset()
    target = copy / document
    target.write_text(target.read_text().replace("| MCP 工具数 | `14` |", "| MCP 工具数 | `999` |"))
    run("managed_table_drift_rejected", copy, "--check", 1)
    run("write_regenerates_only_managed_table", copy, "--write", 0)
    run("regenerated_table_passes", copy, "--check", 0)
    reset()
    target = copy / "docs/CONFIGURATION.md"
    target.write_text(target.read_text().replace("| `worker_lease_secs` | `600` |", "| `worker_lease_secs` | `0` |"))
    run("configuration_default_drift_rejected", copy, "--check", 1)
    run("write_does_not_hide_unfixed_configuration", copy, "--write", 1)
    reset()
    target = copy / "docs/internals/MODULE_CAPABILITIES.json"
    capabilities = json.loads(target.read_text())
    capabilities["database_schema"] = 24
    target.write_text(json.dumps(capabilities))
    run("schema_capability_mismatch_rejected", copy, "--check", 2)
    reset()
    target = copy / "crates/cc-db/src/sql/index_v1.sql"
    target.write_text(target.read_text() + "\nCREATE TABLE IF NOT EXISTS p8_extra (id INTEGER);\n")
    run("new_sql_table_requires_document_review", copy, "--check", 1)
    reset()
    target = copy / "crates/cc-db/src/sql/index_v1.sql"
    target.write_text(target.read_text() + "\n  CREATE TABLE IF NOT EXISTS p8_indented (id INTEGER);\n")
    run("indented_sql_declaration_cannot_be_silently_ignored", copy, "--check", 2)
    reset()
    target = copy / "crates/cc-db/src/sql/index_v1.sql"
    target.write_text(target.read_text() + "\ncreate table if not exists p8_lowercase (id INTEGER);\n")
    run("lowercase_sql_declaration_cannot_be_silently_ignored", copy, "--check", 2)
    reset()
    target = copy / "crates/cc-server/src/mcp.rs"
    target.write_text(target.read_text() + '\n# [ tool ( name = "p8_spaced" ) ]\n')
    run("spaced_tool_attribute_cannot_be_silently_ignored", copy, "--check", 2)
    reset()
    target = copy / "crates/cc-server/src/mcp.rs"
    target.write_text(target.read_text() + '\n#[tool]\n')
    run("bare_tool_attribute_cannot_be_silently_ignored", copy, "--check", 2)
    reset()
    target = copy / "crates/cc-model/src/config.rs"
    target.write_text(target.read_text().replace("worker_lease_secs: default_semantic_worker_lease_secs(),",
                                                "worker_lease_secs: dynamically_compute_lease(),"))
    run("unsupported_rust_default_fails_without_guessing", copy, "--check", 2)

connection = sqlite3.connect(":memory:")
connection.executescript((root / "crates/cc-db/src/sql/index_v1.sql").read_text())
objects = list(connection.execute("PRAGMA table_list"))
ordinary = sorted(row[1] for row in objects if row[0] == "main" and row[2] == "table"
                  and not row[1].startswith("sqlite_"))
virtual = sorted(row[1] for row in objects if row[0] == "main" and row[2] == "virtual")
assert ordinary == sorted(baseline["facts"]["base_tables"])
assert virtual == sorted(baseline["facts"]["fts5_tables"])
hashes_after = {p: hashlib.sha256((root / p).read_bytes()).hexdigest() for p in hashes_before}
assert hashes_before == hashes_after
print(json.dumps({"status": "passed", "scope": "declaration/doc drift controls plus in-memory DDL",
                  "source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root, text=True).strip(),
                  "source_files_sha256": hashes_before, "source_unchanged": True,
                  "cli_cases": rows, "sqlite": {"version": sqlite3.sqlite_version,
                  "ordinary_count": len(ordinary), "fts5_count": len(virtual)},
                  "runtime_certified": False}, ensure_ascii=False, indent=2))
