#!/usr/bin/env python3
"""Offline source-verified evidence and exact-token budget scoring of MCP raw JSONL.

Reports actual full-handler tokens separately from an explicit normalized source
prefix adapter. The adapter is NOT the production MCP output or an agent outcome.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.metadata
import json
import os
import platform
from collections import defaultdict
from pathlib import Path
from statistics import mean
from typing import Any, Callable

POLICY = "source-verified-line-union-v1"
PACKING = "normalized-source-prefix-v1"
ENCODING = "cl100k_base"
CACHE_KEY = "9b5ad71b2ce5302211f9c61530b329a4922fc6a4"
CACHE_SHA256 = "223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7"


def canonical(value: Any) -> str:
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":"), allow_nan=False)


def lines(text: str) -> list[str]:
    # Rust str::lines semantics for LF/CRLF source; don't treat U+2028 as LF.
    result = text.split("\n")
    if result and result[-1] == "":
        result.pop()
    return [line.removesuffix("\r") for line in result]


def pointer(value: Any, path: str) -> Any:
    for part in path.split("/")[1:]:
        key = part.replace("~1", "/").replace("~0", "~")
        try:
            value = value[int(key)] if isinstance(value, list) else value[key]
        except (ValueError, IndexError, KeyError, TypeError):
            return None
    return value


def safe_path(path: str) -> bool:
    return bool(path) and ":" not in path and "\\" not in path and all(
        p not in ("", ".", "..", ".git", ".codecortex", ".codecortex.json") for p in path.split("/"))


def validate_manifest(manifest: dict) -> tuple[dict, dict]:
    if manifest.get("schema_version") != 1 or manifest.get("purpose") not in ("regression", "held_out"):
        raise ValueError("unsupported manifest")
    repos = {r["id"]: r for r in manifest["repositories"]}
    tasks = {t["id"]: t for t in manifest["tasks"]}
    if not repos or not tasks or len(repos) != len(manifest["repositories"]) or len(tasks) != len(manifest["tasks"]):
        raise ValueError("empty or duplicate repository/task set")
    for repo in repos.values():
        if not repo["revision"] or not repo["files"] or not all(safe_path(p) and isinstance(t, str) for p, t in repo["files"].items()):
            raise ValueError("invalid source snapshot")
    for task in tasks.values():
        source = repos[task["repo"]]["files"]
        if task.get("evidence_mode", "source") not in ("source", "locator"):
            raise ValueError("invalid evidence mode")
        labels = {label["id"]: label for label in task["labels"]}
        if len(labels) != len(task["labels"]) or bool(task.get("no_answer", False)) != (not labels):
            raise ValueError("invalid label set")
        if task["result_pointer"] and not task["result_pointer"].startswith("/"):
            raise ValueError("invalid result pointer")
        for label in labels.values():
            start, end = label["start_line"], label["end_line"]
            if not (type(start) is int and type(end) is int and 1 <= start <= end <= len(lines(source[label["file_path"]]))):
                raise ValueError("invalid source region")
            if label.get("anchor") is not None and (not label["anchor"] or label["anchor"] not in "\n".join(lines(source[label["file_path"]])[start-1:end])):
                raise ValueError("anchor absent from source")
        for group in task.get("required_groups", []):
            if not group or any(label not in labels for label in group):
                raise ValueError("invalid evidence group")
    return repos, tasks


def read_raw(path: Path) -> tuple[dict, list[dict]]:
    header = None
    samples = []
    seen = set()
    with path.open(encoding="utf-8") as stream:
        for line in stream:
            record = json.loads(line)
            kind = record.get("kind")
            if kind == "header" and header is None and not samples:
                header = record["data"]
                repos, tasks = validate_manifest(header["manifest"])
                if header.get("schema_version") != 1 or type(header["repetitions"]) is not int or header["repetitions"] < 1:
                    raise ValueError("invalid raw header")
            elif kind == "sample" and header is not None:
                sample = record["data"]
                key = (sample["task_id"], sample["mode"], sample["iteration"])
                if (key in seen or key[0] not in tasks or key[1] not in ("cold_session", "warm_cache") or
                        type(key[2]) is not int or not 0 <= key[2] < header["repetitions"] or
                        (sample.get("output") is None) == (sample.get("error") is None)):
                    raise ValueError("invalid, unknown or duplicate sample")
                seen.add(key)
                samples.append(sample)
            elif kind in ("index", "warmup") and header is not None:
                continue
            else:
                raise ValueError("unexpected raw record")
    if header is None or len(samples) != len(tasks) * 2 * header["repetitions"]:
        raise ValueError("missing header or incomplete sample grid")
    return header, samples


def verified_lines(hit: Any, source: dict[str, str]) -> tuple[str, set[int]] | None:
    if not isinstance(hit, dict):
        return None
    path, start, end, text = (hit.get(k) for k in ("file_path", "start_line", "end_line", "text"))
    if path not in source or type(start) is not int or type(end) is not int or not isinstance(text, str):
        return None
    original, returned = lines(source[path]), lines(text)
    if not (1 <= start <= end <= len(original)) or not returned or len(returned) > end-start+1:
        return None
    # Only exact, correctly positioned source lines count. A metadata span, an
    # unrelated copy of the anchor, or a partly truncated line is not evidence.
    if original[start-1:start-1+len(returned)] != returned:
        return None
    return path, set(range(start, start+len(returned)))


def covered_labels(task: dict, source: dict[str, str], hits: list[Any]) -> set[str]:
    covered: dict[str, set[int]] = defaultdict(set)
    for hit in hits:
        valid = verified_lines(hit, source)
        if valid:
            covered[valid[0]].update(valid[1])
    return {label["id"] for label in task["labels"]
            if set(range(label["start_line"], label["end_line"]+1)) <= covered[label["file_path"]]}


def evidence_metrics(task: dict, covered: set[str]) -> dict:
    groups = task.get("required_groups", [])
    satisfied = sum(any(label in covered for label in group) for group in groups)
    return {"region_coverage": len(covered)/len(task["labels"]) if task["labels"] else None,
            "group_coverage": satisfied/len(groups) if groups else None,
            "sufficient": bool(groups) and satisfied == len(groups)}


def degraded(output: Any) -> bool:
    if not isinstance(output, dict):
        return False
    evidence = output.get("evidence_summary", {})
    if evidence.get("resolution_freshness") in ("incomplete", "unknown"):
        return True
    for lane in evidence.get("retrieval", {}).get("lanes", {}).values():
        if lane.get("work_limited") or lane.get("errors"):
            return True
    explain = evidence.get("graph_enrichment", {}).get("graph_explain", {})
    return bool(explain.get("read_errors"))


def normalized_hit(hit: Any) -> dict:
    if not isinstance(hit, dict):
        return {"invalid_result": hit}  # bad results still consume positions/cost
    return {k: hit[k] for k in ("file_path", "start_line", "end_line", "symbol_name", "text") if k in hit}


def score(task: dict, source: dict[str, str], sample: dict, count: Callable[[str], int], budgets: list[int], overhead_tokens: int = 0) -> dict:
    output = sample.get("output")
    hits = pointer(output, task["result_pointer"]) if output is not None else None
    error = sample.get("error") is not None or not isinstance(hits, list)
    hits = hits if isinstance(hits, list) and not error else []
    all_covered = covered_labels(task, source, hits)
    cost = count(canonical(output)) + overhead_tokens if output is not None else 0
    row = {"task_id": task["id"], "repo": task["repo"], "mode": sample["mode"], "iteration": sample["iteration"],
           "source_evidence_eligible": task.get("evidence_mode", "source") == "source" and not task.get("no_answer", False),
           "error": error, "degraded": degraded(output), "no_answer": task.get("no_answer", False),
           "correct_empty_response": task.get("no_answer", False) and not hits and not error and not degraded(output),
           "full_handler_tokens": cost, "returned": len(hits), "budgets": {}, **evidence_metrics(task, all_covered)}
    normalized = [normalized_hit(h) for h in hits]
    for budget in budgets:
        selected = 0
        packed_tokens = count(canonical({"protocol": PACKING, "hits": []})) + overhead_tokens
        feasible = packed_tokens <= budget
        if feasible and not error:
            for length in range(1, len(hits)+1):
                n = count(canonical({"protocol": PACKING, "hits": normalized[:length]})) + overhead_tokens
                if n > budget:
                    break
                selected, packed_tokens = length, n
        prefix_covered = covered_labels(task, source, hits[:selected])
        row["budgets"][str(budget)] = {
            "full_handler_fits": not error and cost <= budget,
            "production_evidence": evidence_metrics(task, all_covered if not error and cost <= budget else set()),
            "adapter_prefix": {"results": selected, "tokens": packed_tokens, "frame_fits": feasible,
                               **evidence_metrics(task, prefix_covered)}}
    # These are locator-stage diagnostics, deliberately not source-evidence
    # claims. A complete trace is necessary for comparisons across candidates.
    diag = output.get("evidence_summary", {}).get("retrieval", {}) if isinstance(output, dict) else {}
    row["stage_locator_recall"] = {}
    row["stage_trace_truncated"] = bool(diag.get("trace_truncated"))
    if not diag.get("trace_truncated") and task["labels"]:
        for stage, locators in diag.get("stages", {}).items():
            labels = set()
            for label in task["labels"]:
                union = set()
                for loc in locators:
                    if loc.get("file_path") == label["file_path"]:
                        union.update(range(max(loc["start_line"], label["start_line"]), min(loc["end_line"], label["end_line"])+1))
                if set(range(label["start_line"], label["end_line"]+1)) <= union:
                    labels.add(label["id"])
            row["stage_locator_recall"][stage] = len(labels)/len(task["labels"])
    return row


def report(header: dict, samples: list[dict], count: Callable[[str], int], budgets: list[int], overhead_tokens: int = 0) -> dict:
    repos, tasks = validate_manifest(header["manifest"])
    if overhead_tokens < 0 or not budgets or any(type(b) is not int or b <= 0 for b in budgets):
        raise ValueError("invalid token budget")
    rows = [score(tasks[s["task_id"]], repos[tasks[s["task_id"]]["repo"]]["files"], s, count, budgets, overhead_tokens) for s in samples]
    summaries = {}
    for budget in budgets:
        by_task = defaultdict(list)
        for row in rows:
            if row["source_evidence_eligible"]:
                b = row["budgets"][str(budget)]
                by_task[(row["repo"], row["task_id"])].append(float(b["adapter_prefix"]["sufficient"]))
        by_repo = defaultdict(list)
        for (repo, _), values in by_task.items():
            by_repo[repo].append(mean(values))
        summaries[str(budget)] = {"adapter_sufficiency_by_repo": {r: mean(v) for r, v in sorted(by_repo.items())},
                                 "adapter_sufficiency_repo_macro": mean(mean(v) for v in by_repo.values()) if by_repo else None}
    return {"schema_version": 1, "policy": POLICY, "adapter_protocol": PACKING,
            "dataset_id": header["manifest"]["dataset_id"], "purpose": header["manifest"]["purpose"],
            "manifest_git_blob": header["manifest_git_blob"], "implementation_commit": header["implementation_commit"],
            "effective_config": header["effective_config"], "provenance": header["provenance"],
            "source_evidence_observations": sum(r["source_evidence_eligible"] for r in rows),
            "locator_positive_observations": sum(not r["no_answer"] and not r["source_evidence_eligible"] for r in rows),
            "fixed_overhead_tokens": overhead_tokens, "summaries": summaries, "observations": rows,
            "notes": ["Full-handler costs include repeated source and metadata in canonical handler JSON, not JSON-RPC/chat framing.",
                      "Normalized-prefix results evaluate an explicit offline adapter, not current production packing or agent success.",
                      "Duplicates and invalid entries consume positions and tokens. Only exact source lines earn coverage.",
                      "Regression fixtures have no inferential confidence interval. Repetitions are not independent tasks."]}


def tokenizer(cache: Path) -> tuple[Callable[[str], int], dict]:
    if importlib.metadata.version("tiktoken") != "0.12.0":
        raise ValueError("install scripts/requirements-eval.txt")
    data = cache / CACHE_KEY
    if not data.is_file() or hashlib.sha256(data.read_bytes()).hexdigest() != CACHE_SHA256:
        raise ValueError("missing or wrong cl100k_base cache: populate in setup, not while measuring")
    os.environ["TIKTOKEN_CACHE_DIR"] = str(cache.resolve())
    import tiktoken
    encoding = tiktoken.get_encoding(ENCODING)
    metadata = {"name": ENCODING, "tiktoken": "0.12.0", "regex": importlib.metadata.version("regex"),
                "python": platform.python_version(), "data_sha256": CACHE_SHA256, "special_tokens": "ordinary-text"}
    return lambda text: len(encoding.encode_ordinary(text)), metadata


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("raw", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--tokenizer-cache", type=Path, required=True)
    parser.add_argument("--budgets", default="1000,2000,4000,8000,16000")
    parser.add_argument("--overhead-tokens", type=int, default=0)
    args = parser.parse_args()
    count, meta = tokenizer(args.tokenizer_cache)
    header, samples = read_raw(args.raw)
    result = report(header, samples, count, [int(b) for b in args.budgets.split(",")], args.overhead_tokens)
    result["tokenizer"] = meta
    result["raw_sha256"] = hashlib.sha256(args.raw.read_bytes()).hexdigest()
    with args.output.open("x", encoding="utf-8") as stream:
        stream.write(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False)+"\n")
    print(json.dumps(result["summaries"], ensure_ascii=False))


if __name__ == "__main__":
    main()
