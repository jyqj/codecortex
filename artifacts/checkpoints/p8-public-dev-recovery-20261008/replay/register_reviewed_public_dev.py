#!/usr/bin/env python3
"""Register an actually validated six-repository public DEV projection.

This creates a dataset index and documentation, with byte copies of evidence.
It does not change questions, suites, source files, task status, or acceptance
rules and does not run a retrieval engine or access protected question bodies.
"""
import argparse
from collections import Counter
import json
from pathlib import Path
import re

import replay_original_public_dev as base

CORE_SHA256 = "b81aeef61130ac1344c5f011fea64c246868662457d5b7d64932037631e385e2"
PROJECT_SHA256 = "2b0ba73944d341f09d67929c17592ec1be12cf40cff059c53ed3ea0357807570"
HERE = Path(__file__).resolve().parent
PRODUCT = "crates/cc-eval/benchmarks/public-v19/"
EVIDENCE = "artifacts/benchmarks/p8-public-dev-reviewed-20261008/"
INDEX = "crates/cc-eval/benchmarks/manifests/public-dev-20261008.dataset-index.json"
README = "crates/cc-eval/benchmarks/native/public-dev-20261008/README.md"
REPOS = {"express": (70, 59, 7, 110), "requests": (91, 83, 20, 141),
         "gin": (67, 55, 53, 127), "typescript": (73, 59, 20, 98),
         "serde": (14, 14, 15, 18), "vite": (12, 12, 17, 15)}
TOTALS = {"native_rows": 327, "compat_rows": 282, "source_files": 132,
          "gold_spans": 509, "correlated_components": 305}
BUDGET = {"top_k": 10, "repetitions": 3, "warmup": 0, "timeout_ms": 30000, "seed": 20261003}
FIXED_EVIDENCE = {
    "global-components.json": "1af23f71bf6af25ea48528acf7dd7276afddc8a7cccbcb9bef251b41e2f49939",
    "component-review.json": "8972e6eae9af6c7d1bbf2d63a4f4a75de43eb4639914cc723e936398e6273c92",
    "historical-admission.json": "b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425",
    "serde-source-gold-review.json": "b1c39414b705d660bf62711dc2283dabf43cd337ba4137a3fdf3694663272881",
    "vite-source-gold-review.json": "8938fdc4e3ad87e26418ffb411c2b608a6ace1d2dee3907810d1cd18e5b81d0a"}

def json_bytes(value):
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode()

def pinned(path, digest):
    base.require(re.fullmatch("[0-9a-f]{64}", digest) is not None, "SHA256 required")
    raw = base.read_file(path)
    base.require(base.sha(raw) == digest, "fixed receipt/helper identity")
    return raw

def put(out, name, raw):
    base.require(not name.startswith("/") and "\\" not in name and ":" not in name
                 and all(p not in ("", ".", "..", ".git") for p in name.split("/")), "output path")
    path = out / name
    base.require(not path.exists(), "output collision")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    path.chmod(0o644)
    return {"path": name, "bytes": len(raw), "sha256": base.sha(raw)}

def run(args):
    pinned(HERE / "replay_original_public_dev.py", CORE_SHA256)
    pinned(HERE / "project_reviewed_public_dev.py", PROJECT_SHA256)
    projection, original, out = args.projection_output, args.original_output, args.output
    project_raw = pinned(projection / "receipt.json", args.projection_receipt_sha256)
    original_raw = pinned(original / "receipt.json", args.original_receipt_sha256)
    project, old = json.loads(project_raw), json.loads(original_raw)
    base.require(project["status"] == "reviewed_six_repository_public_dev_projection_validated"
                 and project["totals"] == TOTALS, "validated projection required")
    base.require(old["status"] == "original_public_dev_replay_passed"
                 and (old["native_dev_rows"], old["compat_dev_rows"], old["source_files"], old["gold_spans"])
                 == (301, 256, 100, 476), "historical replay required")
    base.require(len(old["suites_validated"]) == 16
                 and all(c["exit_code"] == 0 and c["timed_out"] is False for c in old["suites_validated"]),
                 "16 original V02 gates")
    base.require(project["evaluator_sha256"] == old["evaluator_sha256"], "same real evaluator")
    base.require(project["original_source_author_bytes_unchanged"] is True
                 and old["original_bytes_unchanged"] is True, "original preservation")
    for result in (project, old):
        base.require(all(result[k] == 0 for k in ("provider_calls", "ranking_runs", "protected_body_reads")),
                     "public validation scope")
    inventory = project["canonical_product_inventory"]
    inventory_by_path = {r["path"]: r for r in inventory}
    base.require(len(inventory_by_path) == len(inventory), "duplicate product file")
    for name, record in inventory_by_path.items():
        base.require(name.startswith(PRODUCT), "canonical product path")
        raw = base.read_relative(projection, name)
        base.require(len(raw) == record["bytes"] and base.sha(raw) == record["sha256"]
                     and oct((projection / name).stat().st_mode & 0o777) == record["mode"],
                     "canonical product byte/mode drift")
    found = set()
    for path in (projection / "crates").rglob("*"):
        base.require(not path.is_symlink(), "symlink in product tree")
        if path.is_file():
            found.add(path.relative_to(projection).as_posix())
    base.require(found == set(inventory_by_path), "complete canonical product inventory")
    check_raw = base.read_file(projection / "original-schema-and-v02.json")
    check = json.loads(check_raw)
    base.require(check["status"] == "format_checks_passed_not_source_gold_review" and check["errors"] == {}
                 and check["native_unique_query_ids"] == 327 and check["compat_unique_query_ids"] == 282
                 and check["global_family_components"] == 305, "original schema result")
    validations = check["actual_evaluator_checks"]
    base.require(len(validations) == 12 and all(c["exit_code"] == 0 for c in validations), "12 new V02 gates")
    command_raw = base.read_file(projection / "original-schema-and-v02-command.json")
    command = json.loads(command_raw)
    stdout = base.read_file(projection / "original-schema-and-v02-command.stdout")
    stderr = base.read_file(projection / "original-schema-and-v02-command.stderr")
    base.require(command["exit_code"] == 0 and command["timed_out"] is False
                 and command["stdout_sha256"] == base.sha(stdout) and command["stderr_sha256"] == base.sha(stderr),
                 "original checker command bytes")
    printed = json.loads(stdout)
    base.require(printed["error_count"] == 0 and printed["receipt_sha256"] == base.sha(check_raw)
                 and printed["status"] == check["status"], "checker stdout/result binding")
    evidence = {}
    for name, raw in (("projection-receipt.json", project_raw), ("original-replay-receipt.json", original_raw),
                      ("schema-v02.json", check_raw), ("schema-command.json", command_raw),
                      ("schema-command.stdout", stdout), ("schema-command.stderr", stderr)):
        evidence[name] = put(out, EVIDENCE + name, raw)
    for name, digest in FIXED_EVIDENCE.items():
        source = (HERE / "inputs" / name if name.endswith("-source-gold-review.json")
                  else projection / "evidence" / name)
        evidence[name] = put(out, EVIDENCE + name, pinned(source, digest))
    base.require(base.read_file(projection / "evidence/original-replay-receipt.json") == original_raw,
                 "projection cites exact original replay")
    categories, languages, query_languages = Counter(), Counter(), Counter()
    difficulties = Counter()
    group_counts = Counter()
    repositories, all_ids, graph_rows, no_answer = [], set(), 0, 0
    suite_hashes, query_hashes = [], {}
    for repo, expected in REPOS.items():
        prefix = PRODUCT + repo + "/reviewed-dev-20261008/"
        manifest_name = prefix + "source-manifest.json"
        manifest = json.loads(base.read_relative(projection, manifest_name))
        base.require(manifest["repository"] == repo and len(manifest["files"]) == expected[2], "source manifest")
        for record in manifest["files"]:
            item = inventory_by_path[prefix + "source/" + record["path"]]
            base.require(item["sha256"] == record["sha256"] and item["bytes"] == record["bytes"], "source binding")
        profiles, native = {}, None
        for profile in ("native", "compat"):
            query_name, suite_name = prefix + "queries." + profile + ".dev.jsonl", prefix + "suite." + profile + ".dev.json"
            raw = base.read_relative(projection, query_name)
            qs = [json.loads(line) for line in raw.decode().splitlines() if line.strip()]
            suite = json.loads(base.read_relative(projection, suite_name))
            base.require(len(qs) == expected[0 if profile == "native" else 1]
                         and all(q["split"] == "dev" and q["annotations"]["v19"]["review_status"] == "accepted" for q in qs),
                         "reviewed DEV rows")
            base.require(suite["queries"] == Path(query_name).name and suite["source"]["root"] == "source"
                         and suite["source"]["commit"] is None and all(suite[k] == v for k, v in BUDGET.items()),
                         "suite path/budget")
            base.require(suite["source"]["files"] == [r["path"] for r in manifest["files"]], "suite source subset")
            base.require(suite["scoring"] == ("codecortex-native-v1" if profile == "native" else "oce-compat-v1")
                         and suite["engine_config"] == {"auto_index": {"enabled": False}}, "scoring/config identity")
            profiles[profile] = {"rows": len(qs), "scoring": suite["scoring"],
                                 "queries": inventory_by_path[query_name], "suite": inventory_by_path[suite_name]}
            suite_hashes.append(inventory_by_path[suite_name]["sha256"])
            query_hashes[(repo, profile)] = inventory_by_path[query_name]["sha256"]
            if profile == "native":
                native = qs
        for q in native:
            base.require(q["id"] not in all_ids, "duplicate native ID")
            all_ids.add(q["id"])
            categories[q["category"]] += 1
            languages[q["language"]] += 1
            query_languages[q["annotations"]["v19"]["query_language"]] += 1
            difficulties[str(q["difficulty"])] += 1
            no_answer += int(q["no_answer"])
            graph_rows += int(bool(q["annotations"]["v19"]["graph_constraints"]))
            for group in q["answers"]:
                group_counts["primary" if group["primary"] else "supporting"] += 1
                group_counts["alternative_spans"] += len(group["alternatives"])
        actual = project["repositories"][repo]
        base.require((actual["native"], actual["compat"], actual["source_files"], actual["gold_spans"]) == expected,
                     "receipt repository counts")
        repositories.append({"repository": repo, "upstream_commit": manifest["upstream_commit"],
                             "source_scope": manifest["scope"], "source_files": expected[2],
                             "gold_alternative_spans": expected[3], "no_answer": actual["no_answer"],
                             "source_manifest": inventory_by_path[manifest_name], "profiles": profiles})
    base.require(Counter(c["suite_sha256"] for c in validations) == Counter(suite_hashes), "12 actual suite byte bindings")
    actual_queries = {(r["repo"], r["profile"]): r["sha256"] for r in check["query_files"] if "repo" in r}
    base.require(actual_queries == query_hashes, "checker actual query byte bindings")
    base.require(len(all_ids) == 327 and no_answer == 45 and graph_rows == 58
                 and group_counts == {"primary": 282, "supporting": 225, "alternative_spans": 509},
                 "transparent denominator counts")
    index = {"schema_version": 1, "dataset_id": "public-dev-20261008",
             "status": "source_gold_reviewed_public_dev_with_actual_schema_and_v02_validation",
             "task_completion": "not decided by this registration helper",
             "canonical_source_root": PRODUCT, "totals": TOTALS, "no_answer_native_only": no_answer,
             "answer_groups": dict(group_counts), "categories": dict(sorted(categories.items())),
             "language_labels_as_authored": dict(sorted(languages.items())),
             "query_languages": dict(sorted(query_languages.items())), "difficulty": dict(sorted(difficulties.items())),
             "optional_graph_constraint_rows": graph_rows, "original_rows": 301, "new_rows": 26,
             "source_gold_scope": {"original_301": "Exact historical independently accepted source/gold bytes and review chain; no new semantic rereview.",
                                  "new_26": "Current separate nonauthor source/gold reviews, including versioned Serde alternatives and Vite SPA wording correction."},
             "global_component_scope": "305 known correlated components; not 305 proven independent samples.",
             "repositories": repositories, "evidence": evidence, "budget": BUDGET,
             "evaluator_sha256": project["evaluator_sha256"],
             "validation": {"original_suites": 16, "new_suites": 12, "schema_errors": 0,
                            "registration_runs_no_evaluator": True, "project_helper_sha256": PROJECT_SHA256},
             "gaps": ["All 327 queries are English; Chinese/paraphrase and zero lexical overlap coverage is not certified.",
                      "327 public DEV rows are below the original approximate 600 planning target; that target is not a new admission threshold.",
                      "Only 132 selected source files from six public repositories, not full checkouts or complete build dependencies.",
                      "Private data and protected holdout bodies are excluded; 6 Serde and 8 Vite reserved slots remain undrafted and unread.",
                      "The JS-to-TS cross_language item has explicit source facets; file/span scoring does not certify graph precision or recall.",
                      "58 historical optional graph constraints are preserved; this corpus registration does not execute or certify graph retrieval.",
                      "Native groups/spans and compatibility any-expected-file scoring are distinct views; 45 native no-answer rows have no compatibility projection.",
                      "Metadata source files and repository indexing policies need explicit runtime capability coverage; admission does not prove every source is indexed.",
                      "No retrieval ranking or semantic-benefit claim, provider run, release qualification, or protected-holdout custody certification."]}
    index_record = put(out, INDEX, json_bytes(index))
    table = "\n".join("| {repository} | {n} | {c} | {source_files} | {gold_alternative_spans} |".format(
        n=r["profiles"]["native"]["rows"], c=r["profiles"]["compat"]["rows"], **r) for r in repositories)
    readme = """# Reviewed public DEV dataset

The dataset index is [public-dev-20261008.dataset-index.json](../../manifests/public-dev-20261008.dataset-index.json).
It records exact query, suite and source-manifest SHA256 values and byte-bound evidence.

| Repository | Native rows | Compatibility rows | Source files | Gold alternative spans |
| --- | ---: | ---: | ---: | ---: |
""" + table + """

There are 327 native questions, 282 compatibility projections and 509 alternative spans across 132 selected files.
The 45 no-answer questions remain in native evaluation. The 305 component groups describe known correlation;
they are not a claim of statistical independence. Six real repositories cover Rust, Python, Go, JavaScript,
TypeScript and a JavaScript/TypeScript monorepo. The index keeps code-language labels, metadata-format labels
and English query language visible rather than treating translated questions as new independent samples.

The original 301 questions retain their fixed historical source/gold evidence. The 26 new Serde/Vite questions
have separate nonauthor reviews. Author inputs and canonical-only intermediates remain preserved; these
reviewed copies only apply the independently approved global-family mapping and explicit review metadata.
No question or gold was changed based on retrieval ranking.

From the repository root, validate a frozen native suite using the evaluator:
    
    cargo run --locked -p cc-eval --bin cc-eval -- validate --suite crates/cc-eval/benchmarks/public-v19/serde/reviewed-dev-20261008/suite.native.dev.json

Use the corresponding suite.compat.dev.json for compatibility validation. The index lists all twelve suites.
Actual admission evidence used the exact evaluator SHA recorded in the index, sixteen original V02 validations,
twelve new V02 validations and the unchanged V19 schema checker. A current local build is not that archived
binary; its new validation result must retain its own source/build identity. These validation commands do not
run retrieval or establish ranking quality.

All questions are public DEV and English. The source folders are selected byte-locked subsets, not full
upstream checkouts. Private data, protected held-out questions, Chinese/paraphrase coverage, zero-overlap
quality, graph retrieval quality and release qualification remain outside this certification. The original
approximately 600-question planning target remains a transparent coverage gap. Six Serde and eight Vite
reserved slots remain undrafted and unread.

Native required facets and spans differ from compatibility any-expected-file matching. A JS-to-TS question
records explicit source edges, while 58 inherited optional graph-constraint rows are retained without a graph
precision/recall claim. Metadata and source indexing capabilities need explicit runtime validation. See the
dataset index's gaps and source_gold_scope fields for the exact limits.
"""
    readme_record = put(out, README, readme.encode())
    result = {"schema_version": 1, "status": "registration_created_from_fixed_actual_validation_receipts",
              "projection_receipt_sha256": args.projection_receipt_sha256,
              "original_receipt_sha256": args.original_receipt_sha256,
              "dataset_index": index_record, "readme": readme_record, "evidence": evidence,
              "canonical_product_files_unchanged": True, "task_status_not_changed": True}
    base.write_json(out / "receipt.json", result)
    return result

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--projection-output", type=Path, required=True)
    p.add_argument("--original-output", type=Path, required=True)
    p.add_argument("--projection-receipt-sha256", required=True)
    p.add_argument("--original-receipt-sha256", required=True)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    a.projection_output = a.projection_output.resolve()
    a.original_output = a.original_output.resolve()
    a.output = a.output.resolve()
    for source in (a.projection_output, a.original_output):
        base.require(a.output != source and source not in a.output.parents and a.output not in source.parents,
                     "registration output must be separate from both immutable input trees")
    base.require(not a.output.exists(), "fresh registration output required")
    a.output.mkdir(parents=True)
    try:
        result = run(a)
    except Exception as exc:
        base.write_json(a.output / "failure.json", {"status": "failed", "exception": type(exc).__name__,
                        "message": str(exc), "not_accepted": True})
        raise
    print(json.dumps({"status": result["status"], "dataset_index": result["dataset_index"]}))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
