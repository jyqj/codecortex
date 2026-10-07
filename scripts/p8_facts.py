#!/usr/bin/env python3
"""Derive narrowly scoped documentation facts from actual repository declarations."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[1]
DOCUMENT = "docs/roadmap/code-index-v2/P8-FACTS.md"
START, END = "<!-- p8-facts:start -->", "<!-- p8-facts:end -->"
LIMIT = 2 * 1024 * 1024


def read(root, relative):
    target = root / relative
    if target.is_symlink() or not target.is_file() or target.stat().st_size > LIMIT:
        raise ValueError("missing, symlinked or oversized facts input: " + relative)
    with target.open("rb") as stream:
        raw = stream.read(LIMIT + 1)
    if len(raw) > LIMIT:
        raise ValueError("facts input grew beyond limit: " + relative)
    return raw.decode("utf-8")


def one(pattern, source, label, flags=0):
    matches = re.findall(pattern, source, flags)
    if len(matches) != 1:
        raise ValueError("unsupported or ambiguous declaration: " + label)
    return matches[0]


def defaults(source, type_name, fields):
    # This handles the current literal/default-function forms only. It is not
    # a Rust parser or a generated serialization/runtime schema.
    body = one(r"impl Default for " + re.escape(type_name) +
               r"\s*\{\s*fn default\(\)\s*->\s*Self\s*\{\s*Self\s*\{(.*?)\n\s*\}\s*\}\s*\}",
               source, type_name + " default", re.S)
    result = {}
    for field in fields:
        value = one(r"^\s*" + re.escape(field) + r":\s*([^,\n]+),\s*$",
                    body, type_name + "." + field, re.M).strip()
        if re.fullmatch(r"default_[a-z_]+\(\)", value):
            function = value[:-2]
            value = one(r"\bfn " + function + r"\(\)\s*->\s*(?:u32|u64|usize|bool)\s*\{\s*"
                        r"([0-9_]+|true|false)\s*\}", source, function)
        if value in ("true", "false", "None"):
            result[field] = {"true": True, "false": False, "None": None}[value]
        elif re.fullmatch(r"[0-9_]+", value):
            result[field] = int(value.replace("_", ""))
        elif value in ("RetrievalStrategy::Local", "RetrievalStrategy::Auto", "RetrievalStrategy::Semantic"):
            result[field] = value.rsplit("::", 1)[1].lower()
        else:
            raise ValueError("unsupported default expression: " + type_name + "." + field)
    return result


def collect(root):
    paths = ["Cargo.toml", "crates/cc-server/Cargo.toml", "crates/cc-db/src/index_migrate.rs",
             "crates/cc-db/src/sql/index_v1.sql", "docs/internals/MODULE_CAPABILITIES.json",
             "crates/cc-server/src/mcp.rs", "crates/cc-server/src/capability_status.rs",
             "crates/cc-model/src/config.rs", "crates/cc-model/src/query.rs"]
    sources = {relative: read(root, relative) for relative in paths}
    workspace = tomllib.loads(sources["Cargo.toml"])["workspace"]
    features = tomllib.loads(sources["crates/cc-server/Cargo.toml"])["features"]
    capabilities = json.loads(sources["docs/internals/MODULE_CAPABILITIES.json"])
    schema = int(one(r"pub const CURRENT_SCHEMA_VERSION: u32 = (\d+);",
                     sources["crates/cc-db/src/index_migrate.rs"], "database schema"))
    if type(capabilities.get("database_schema")) is not int or capabilities["database_schema"] != schema:
        raise ValueError("database schema/capabilities declaration mismatch")
    sql = sources["crates/cc-db/src/sql/index_v1.sql"]
    tables = re.findall(r"^CREATE TABLE IF NOT EXISTS ([a-z_][a-z_0-9]*)\s*\(", sql, re.M)
    virtual = re.findall(r"^CREATE VIRTUAL TABLE IF NOT EXISTS ([a-z_][a-z_0-9]*) USING fts5\(", sql, re.M)
    # Count declarations more broadly than the supported parser: newly added
    # indentation/case/spacing forms must fail closed rather than disappear.
    all_create = re.findall(r"^\s*CREATE\s+(?:VIRTUAL\s+)?TABLE\b", sql, re.M | re.I)
    if not tables or len(tables) + len(virtual) != len(all_create) or len(set(tables + virtual)) != len(all_create):
        raise ValueError("unsupported/duplicate SQL table declaration")
    mcp = sources["crates/cc-server/src/mcp.rs"]
    tools = re.findall(r'#\[tool\(\s*name\s*=\s*"([a-z_][a-z_0-9]*)"', mcp)
    all_tools = re.findall(r"#\s*\[\s*tool\b", mcp)
    if not tools or len(tools) != len(all_tools) or len(set(tools)) != len(tools):
        raise ValueError("unsupported/duplicate MCP tool registration")
    config = sources["crates/cc-model/src/config.rs"]
    values = {
        "query": defaults(sources["crates/cc-model/src/query.rs"], "QueryConfig",
                          ["strategy", "deadline_ms", "lane_timeout_ms", "semantic_timeout_ms", "semantic_top_k"]),
        "auto_index": defaults(config, "AutoIndexConfig", ["enabled", "file_limit", "idle_timeout_secs"]),
        "semantic": defaults(config, "SemanticProviderConfig", ["enabled", "network_opt_in",
                             "allow_query_network", "allow_http", "reembed_budget_max",
                             "worker_lease_secs", "gc_min_retention_secs"]),
    }
    facts = {"workspace_crates": [Path(member).name for member in workspace["members"]],
             "declared_msrv": workspace["package"]["rust-version"], "database_schema": schema,
             "base_tables": tables, "fts5_tables": virtual,
             "schema_policy": capabilities["database_schema_compatibility"]["policy"],
             "project_model_version": capabilities["project_model_version"],
             "default_network": capabilities["default_network"],
             "runtime_executes_indexed_code": capabilities["runtime_executes_indexed_code"],
             "mcp_tools": tools, "capability_spec": one(r'pub const CAPABILITY_SPEC: &str = "([^"]+)";',
                  sources["crates/cc-server/src/capability_status.rs"], "capability spec"),
             "default_product_features": features.get("default", []), "defaults": values}
    hashes = {relative: hashlib.sha256(text.encode()).hexdigest() for relative, text in sources.items()}
    return facts, hashes


def scalar(value):
    return str(value) if isinstance(value, str) else json.dumps(value, separators=(",", ":"))


def managed(facts):
    rows = [
        ("Cargo workspace crate 数", len(facts["workspace_crates"]), "Cargo.toml"),
        ("workspace 成员", ", ".join(facts["workspace_crates"]), "Cargo.toml"),
        ("声明 MSRV", facts["declared_msrv"], "Cargo.toml"),
        ("数据库 schema", facts["database_schema"], "index_migrate.rs / MODULE_CAPABILITIES.json"),
        ("显式普通表数（不含 FTS/shadow）", len(facts["base_tables"]), "sql/index_v1.sql"),
        ("显式 FTS5 虚表数", len(facts["fts5_tables"]), "sql/index_v1.sql"),
        ("数据库版本不匹配策略", facts["schema_policy"], "MODULE_CAPABILITIES.json"),
        ("ProjectModel 版本", facts["project_model_version"], "MODULE_CAPABILITIES.json"),
        ("默认网络声明", facts["default_network"], "MODULE_CAPABILITIES.json"),
        ("执行被索引源码声明", facts["runtime_executes_indexed_code"], "MODULE_CAPABILITIES.json"),
        ("MCP 工具数", len(facts["mcp_tools"]), "cc-server/src/mcp.rs"),
        ("MCP 工具名", ", ".join(facts["mcp_tools"]), "cc-server/src/mcp.rs"),
        ("能力观测 spec", facts["capability_spec"], "capability_status.rs"),
        ("产品默认 Cargo features", facts["default_product_features"], "cc-server/Cargo.toml"),
    ]
    rows += [(section + "." + field, value, "query.rs" if section == "query" else "config.rs")
             for section, values in facts["defaults"].items() for field, value in values.items()]
    return "\n".join([START, "| 事实 | 当前声明值 | 直接来源 |", "|---|---|---|"] +
                     [f"| {label} | `{scalar(value)}` | `{source}` |" for label, value, source in rows] + [END])


def entry_errors(root, facts):
    errors = []
    architecture = read(root, "docs/ARCHITECTURE.md")
    for text in (f'{len(facts["workspace_crates"])} crate 的 Cargo 工作区',
                 f'{len(facts["base_tables"])} 基表 + {len(facts["fts5_tables"])} FTS5、schema v{facts["database_schema"]}'):
        if text not in architecture:
            errors.append("ARCHITECTURE.md current counts differ: " + text)
    if f'{len(facts["mcp_tools"])} 个工具全部常驻可用' not in read(root, "docs/MCP_TOOLS.md"):
        errors.append("MCP_TOOLS.md current tool count differs")
    configuration = read(root, "docs/CONFIGURATION.md")
    headings = {"query": "## query", "auto_index": "## auto_index",
                "semantic": "### `semantic` 配置节"}
    for section, values in facts["defaults"].items():
        start = configuration.find(headings[section])
        if start < 0:
            errors.append("CONFIGURATION.md missing section: " + section)
            continue
        rest = configuration[start:].split("\n", 1)[1]
        block = re.split(r"\n#{1,3} ", rest, maxsplit=1)[0]
        for key, value in values.items():
            if f"| `{key}` | `{scalar(value)}` |" not in block:
                errors.append("CONFIGURATION.md default differs/missing: " + section + "." + key)
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=ROOT)
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--check", action="store_true")
    action.add_argument("--write", action="store_true")
    args = parser.parse_args(argv)
    try:
        facts, hashes = collect(args.root)
        document = read(args.root, DOCUMENT)
        if document.count(START) != 1 or document.count(END) != 1 or document.index(START) >= document.index(END):
            raise ValueError("missing/duplicate facts document markers")
        start, end = document.index(START), document.index(END) + len(END)
        expected = managed(facts)
        drift = document[start:end] != expected
        if args.write and drift:
            target = args.root / DOCUMENT
            target.write_text(document[:start] + expected + document[end:], encoding="utf-8")
            drift = False
        errors = entry_errors(args.root, facts)
        if drift:
            errors.append("P8-FACTS.md managed table differs; run --write after reviewing declarations")
        print(json.dumps({"status": "failed" if errors else "passed", "scope": "declared_documentation_facts_only",
                          "runtime_certified": False, "facts": facts, "input_sha256": hashes, "errors": errors},
                         ensure_ascii=False, sort_keys=True))
        return 1 if errors else 0
    except (ValueError, OSError, KeyError, TypeError) as exc:
        print(json.dumps({"status": "invalid_input", "reason": str(exc), "runtime_certified": False}), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
