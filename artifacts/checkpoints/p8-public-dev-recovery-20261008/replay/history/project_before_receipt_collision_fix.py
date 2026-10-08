#!/usr/bin/env python3
"""Create a reviewed public DEV projection, then use the original schema/V02 tools.

This is a new deterministic projection. Original author snapshots and canonical-
only intermediate copies remain separate. Source/gold acceptance comes from the
pinned nonauthor receipts, not from this author-owned transformation.
"""
import argparse
from collections import Counter, defaultdict
import copy
import hashlib
import json
import os
from pathlib import Path
import re
import sys

import replay_original_public_dev as base

CFG = json.loads(r'''{
  "schema_version": 1,
  "repo_order": [
    "express",
    "requests",
    "gin",
    "typescript",
    "serde",
    "vite"
  ],
  "counts": {
    "express": [
      70,
      59,
      7,
      110
    ],
    "requests": [
      91,
      83,
      20,
      141
    ],
    "gin": [
      67,
      55,
      53,
      127
    ],
    "typescript": [
      73,
      59,
      20,
      98
    ],
    "serde": [
      14,
      14,
      15,
      18
    ],
    "vite": [
      12,
      12,
      17,
      15
    ]
  },
  "protocol": {
    "checker_sha256": "c3b6a715eb10cdd1b1a5bef61596552c845326719bea6c570b9b8d38f7cb89a6",
    "schema_sha256": "d71aa4f8cde53c1e09b465be857c402dd4c8411c3b08d23b0e61286f0a5782d0",
    "locks_sha256": "c17f10b6b289c427c1324a73b5251ed1985272882a073c1763e8fe774da174b4"
  },
  "reviews": {
    "serde": {
      "path": "serde-source-gold-review.json",
      "sha256": "b1c39414b705d660bf62711dc2283dabf43cd337ba4137a3fdf3694663272881",
      "reviewer": "/root/pr_audit"
    },
    "vite": {
      "path": "vite-source-gold-review.json",
      "sha256": "8938fdc4e3ad87e26418ffb411c2b608a6ace1d2dee3907810d1cd18e5b81d0a",
      "reviewer": "/root"
    }
  },
  "canonical": {
    "mapping_sha256": "1af23f71bf6af25ea48528acf7dd7276afddc8a7cccbcb9bef251b41e2f49939",
    "plan_sha256": "282b4358cde8a98a9b964370463cbb5f14aba32c2ce95d3352fad563f1790694",
    "review_sha256": "8972e6eae9af6c7d1bbf2d63a4f4a75de43eb4639914cc723e936398e6273c92"
  },
  "old_reviewers": {
    "express": "independent-source-review/cloud-express-dev-review",
    "requests": "requests-independent-dev-reviewer/root",
    "gin": "independent-source-review/cloud-gin-dev-review",
    "typescript": "independent-source-review/cloud-typescript-dev-review"
  },
  "new_inputs": {
    "serde": {
      "prefix": "crates/cc-eval/benchmarks/public-v19/serde/intake/recovery-authoring-20261008/",
      "native_sha256": "66bee463e8c81c3e805156d76f37607d51bcc76863745ba659560598614d408b",
      "compat_sha256": "c6f2cbc039fbb00de345155bb38a03e4d21233bfa199aed15136427902796401",
      "manifest_sha256": "4b59b02bead695c6ff8d78caad33bbd8dd515f3f013da3ed9a5d07347911651a"
    },
    "vite": {
      "prefix": "crates/cc-eval/benchmarks/public-v19/vite/intake/recovery-authoring-20261008/",
      "native_sha256": "c405329c344727d031dbd92b9a5d4428167350aa601326d81a9414ee22cff2f3",
      "compat_sha256": "86903dc383ff6f66a45b084d3475680dda0eeedef9f37d883b52b19ea7ebca59",
      "manifest_sha256": "9077e9268a10c72c07eacb7273dea91e75289e192646dd9b2dd1acf142ae1eac"
    }
  }
}''')
CORE_SHA256 = "b81aeef61130ac1344c5f011fea64c246868662457d5b7d64932037631e385e2"
HERE = Path(__file__).resolve().parent
CRATES = "crates/cc-eval/benchmarks/public-v19/"
RUN = "reviewed-dev-20261008"
ADMISSION_ENTRY = "5385f5a7a2a875c6d5cbd049bdde039bf71bbf32:" + CRATES + "protocol/global-dev-review/typescript-extension/admission.json"
ADMISSION_SHA256 = "b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425"

def pinned(path, digest):
    raw = base.read_file(path)
    base.require(base.sha(raw) == digest, "fixed input drift: " + path.name)
    return raw

def put(root, name, raw, mode=0o644):
    base.require(not name.startswith("/") and "\\" not in name and
                 all(p not in ("", ".", "..", ".git") for p in name.split("/")), "output path")
    path = root / name
    base.require(not path.exists(), "output collision")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    path.chmod(mode)
    return path

def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode()

def rows(raw):
    result = [json.loads(line) for line in raw.decode("utf-8").splitlines() if line.strip()]
    base.require(result and all(r["split"] == "dev" for r in result), "only explicit DEV rows")
    return result

def canonical_copy(raw, spec, members):
    base.require(base.sha(raw) == spec["input_sha256"], "canonical input hash")
    base.require(hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
                 == spec["input_git_blob"], "canonical input Git identity")
    changes = {r["id"]: r for r in spec["changed_rows"]}
    base.require(len(changes) == len(spec["changed_rows"]), "duplicate change ID")
    used, projected = set(), []
    for line in raw.decode("utf-8").splitlines(keepends=True):
        if not line.strip():
            projected.append(line)
            continue
        q = json.loads(line)
        base.require(q["split"] == "dev", "protected row")
        original = copy.deepcopy(q)
        c = changes.get(q["id"])
        after = line
        if c:
            base.require(q["query_family"] == c["query_family"] and
                         q["annotations"]["v19"]["global_family"] == c["old"], "canonical old binding")
            pattern = r'("global_family"\s*:\s*")' + re.escape(c["old"]) + r'(")'
            base.require(len(re.findall(pattern, line)) == 1, "single literal canonical field")
            after = re.sub(pattern, lambda m: m[1] + c["new"] + m[2], line)
            reverse = re.sub(r'("global_family"\s*:\s*")' + re.escape(c["new"]) + r'(")',
                             lambda m: m[1] + c["old"] + m[2], after)
            base.require(reverse == line, "canonical byte inverse")
            base.require(base.sha(line.rstrip("\r\n").encode()) == c["original_row_sha256"]
                         and base.sha(after.rstrip("\r\n").encode()) == c["projected_row_sha256"],
                         "canonical row hash")
            q = json.loads(after)
            restored = copy.deepcopy(q)
            restored["annotations"]["v19"]["global_family"] = c["old"]
            base.require(restored == original, "noncanonical field changed")
            used.add(q["id"])
        base.require(q["annotations"]["v19"]["global_family"] == members[q["query_family"]],
                     "component membership annotation")
        projected.append(after)
    result = "".join(projected).encode()
    base.require(used == set(changes) and len(rows(result)) == spec["rows"], "canonical row inventory")
    base.require(len(result) == spec["expected_utf8_bytes"]
                 and base.sha(result) == spec["expected_output_sha256"], "canonical output identity")
    return result

def reviewed_row(q, repo, reviews):
    r = copy.deepcopy(q)
    a = r["annotations"]["v19"]
    prior = copy.deepcopy(a)
    if repo in reviews:
        review, digest = reviews[repo]
        reviewer = review["reviewer"]
        basis = "current separately authored source/gold review of this exact revision"
    else:
        reviewer, digest = CFG["old_reviewers"][repo], ADMISSION_SHA256
        basis = "historical independent source/gold acceptance chain, exact original bytes replayed; not a new semantic review"
    base.require(prior.get("author_id") and prior["author_id"] != reviewer
                 and prior["review_status"] == "pending", "independent source/gold roles")
    a["review_status"], a["reviewer_id"], a["review_receipt_sha256"] = "accepted", reviewer, digest
    a["review_projection_provenance"] = {
        "prior_review_status": prior["review_status"], "basis": basis,
        "component_review_sha256": CFG["canonical"]["review_sha256"],
        "original_author_annotation_preserved_in_input": True}
    # Restore only this explicitly allowed annotation delta and compare every
    # remaining value, including query, ID, gold, facets, source and scope.
    inverse = copy.deepcopy(r)
    inverse["annotations"]["v19"] = prior
    base.require(inverse == q, "review annotation escaped allowed object")
    return r

def run(repo, evaluator, evaluator_sha, original_receipt, out):
    pinned(HERE / "replay_original_public_dev.py", CORE_SHA256)
    binary = base.read_file(evaluator, 512 * 1024 * 1024)
    base.require(base.sha(binary) == evaluator_sha, "evaluator identity")
    original = json.loads(base.read_file(original_receipt))
    base.require(original["status"] == "original_public_dev_replay_passed"
                 and original["evaluator_sha256"] == evaluator_sha
                 and original["recovery_manifest_sha256"] == base.MANIFEST_SHA256
                 and original["original_bytes_unchanged"] is True
                 and len(original["suites_validated"]) == 16
                 and all(r["exit_code"] == 0 for r in original["suites_validated"]),
                 "original required replay")
    support, original_before = base.verify_archive(repo)
    entries = {r["entry"]: r for r in support["files"]}
    admission_raw = base.read_relative(repo, entries[ADMISSION_ENTRY]["restored_path"])
    base.require(base.sha(admission_raw) == ADMISSION_SHA256, "historical admission")
    admission = json.loads(admission_raw)
    base.require(admission["status"] == "development_admitted_snapshot_scope_only"
                 and admission["errors"] == {}, "historical source/gold verdict")
    inputs = HERE / "inputs"
    plan_raw = pinned(inputs / "canonical-projection-plan.json", CFG["canonical"]["plan_sha256"])
    mapping_raw = pinned(inputs / "global-components.json", CFG["canonical"]["mapping_sha256"])
    component_review_raw = pinned(inputs / "component-review.json", CFG["canonical"]["review_sha256"])
    plan, mapping = json.loads(plan_raw), json.loads(mapping_raw)
    base.require(len(plan["outputs"]) == 20 and len(mapping["components"]) == 305, "component count")
    members = {}
    for c in mapping["components"]:
        base.require(c["members"] and c["global_family"] == min(c["members"]), "canonical minimum")
        value = hashlib.sha256(b"codecortex-public-v19-split-v1\n" + c["global_family"].encode()).digest()
        base.require(int.from_bytes(value[:8], "big") >= 2**62, "component must remain DEV")
        for member in c["members"]:
            base.require(member not in members, "duplicate component member")
            members[member] = c["global_family"]
    reviews = {}
    new_before = {}
    for name, r in CFG["reviews"].items():
        raw = pinned(inputs / r["path"], r["sha256"])
        o = json.loads(raw)
        base.require(o["reviewer"] == r["reviewer"] and o["author"] == "/root/p7_wiring"
                     and o["reviewer"] != o["author"] and o["verdict"].startswith("accepted_scoped"),
                     "nonauthor review identity")
        reviews[name] = (o, r["sha256"])
        put(out, "evidence/" + r["path"], raw)
    for name, digest in [("check.py", CFG["protocol"]["checker_sha256"]),
                         ("evaluator-query.schema.json", CFG["protocol"]["schema_sha256"]),
                         ("source-locks.json", CFG["protocol"]["locks_sha256"])]:
        pinned(HERE / "protocol" / name, digest)
    canonical, reviewed = defaultdict(list), defaultdict(list)
    for spec in plan["outputs"]:
        name, profile = spec["repo"], spec["profile"]
        base.require(name in CFG["repo_order"] and profile in ("native", "compat"), "shard identity")
        if name in CFG["new_inputs"]:
            ni = CFG["new_inputs"][name]
            path = ni["prefix"] + "revision-2/queries." + profile + ".candidate.dev.jsonl"
            raw = base.read_relative(repo, path)
            base.require(base.sha(raw) == ni[profile + "_sha256"], "new source/gold reviewed shard")
            review_inputs = reviews[name][0]["fixed_inputs"]
            if isinstance(review_inputs, list):
                review_inputs = {r["role"]: r for r in review_inputs}
            base.require(review_inputs[profile]["sha256"] == base.sha(raw), "gold review input binding")
            new_before[path] = base.sha(raw)
        else:
            raw = base.read_relative(repo, entries[spec["input_id"]]["restored_path"])
        new_raw = canonical_copy(raw, spec, members)
        put(out, spec["output_path"], new_raw)
        parsed = rows(new_raw)
        canonical[(name, profile)].extend(parsed)
        reviewed[(name, profile)].extend(reviewed_row(q, name, reviews) for q in parsed)
    base.require({q["query_family"] for (n, p), rs in canonical.items() if p == "native"
                  for q in rs} == set(members), "component member coverage")
    for name, (review, _) in reviews.items():
        row_ids = {r["id"] for r in review["rows"]}
        base.require(row_ids == {r["id"] for r in canonical[(name, "native")]}, "reviewed row inventory")
        for r in review["rows"]:
            base.require(r.get("decision", r.get("verdict", "")).startswith("accepted"), "review row verdict")
    put(out, "evidence/global-components.json", mapping_raw)
    put(out, "evidence/canonical-projection-plan.json", plan_raw)
    put(out, "evidence/component-review.json", component_review_raw)
    put(out, "evidence/historical-admission.json", admission_raw)
    put(out, "evidence/original-replay-receipt.json", base.read_file(original_receipt))
    suites, shard_args, inventory, statistics = [], [], [], {}
    for name in CFG["repo_order"]:
        prefix = CRATES + name + "/" + RUN + "/"
        source, licenses = [], []
        if name in CFG["new_inputs"]:
            ni = CFG["new_inputs"][name]
            raw_manifest = base.read_relative(repo, ni["prefix"] + "source-manifest.json")
            base.require(base.sha(raw_manifest) == ni["manifest_sha256"], "new source manifest")
            nm = json.loads(raw_manifest)
            upstream = nm["source_sha"]
            for r in nm["source_files"]:
                raw = base.read_relative(repo, ni["prefix"] + "source/" + r["path"])
                base.require(base.sha(raw) == r["sha256"] and len(raw) == r["bytes"], "new source bytes")
                base.require(hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()
                             == r["git_blob"], "new upstream Git blob")
                mode = int(r["git_mode"][-3:], 8)
                put(out, prefix + "source/" + r["path"], raw, mode)
                source.append({"path": r["path"], "sha256": base.sha(raw), "bytes": len(raw),
                               "git_blob": r["git_blob"], "mode": r["git_mode"]})
                new_before[ni["prefix"] + "source/" + r["path"]] = base.sha(raw)
            for r in nm["licenses"]:
                raw = base.read_relative(repo, ni["prefix"] + "licenses/" + r["path"])
                base.require(base.sha(raw) == r["sha256"], "new license")
                put(out, prefix + "licenses/" + r["path"], raw)
                licenses.append({"path": r["path"], "sha256": base.sha(raw)})
        else:
            prior = admission["repo_results"][name]
            base.require(prior["current_content_hash_accept"] == CFG["counts"][name][0],
                         "historical source/gold count")
            upstream = prior["upstream_sha"]
            old_prefix = prior["author_sha"] + ":" + CRATES + name + "/"
            for r in support["files"]:
                if r["entry"].startswith(old_prefix + "source/"):
                    rel = r["entry"][len(old_prefix + "source/"):]
                    raw = base.read_relative(repo, r["restored_path"])
                    put(out, prefix + "source/" + rel, raw)
                    source.append({"path": rel, "sha256": r["sha256"], "bytes": r["bytes"],
                                   "git_blob": r["git_blob"], "mode": "100644",
                                   "mode_scope": "historical text archive; not an upstream-mode assertion"})
                elif r["entry"].startswith(old_prefix + "license/"):
                    rel = r["entry"][len(old_prefix + "license/"):]
                    put(out, prefix + "licenses/" + rel, base.read_relative(repo, r["restored_path"]))
                    licenses.append({"path": rel, "sha256": r["sha256"]})
        base.require(len(source) == CFG["counts"][name][2], "source inventory count")
        source.sort(key=lambda r: r["path"])
        put(out, prefix + "source-manifest.json", json_bytes({
            "schema_version": 1, "repository": name, "upstream_commit": upstream,
            "scope": "explicit source text subset; not a clean or complete upstream checkout",
            "files": source, "licenses": licenses, "ranking_inspected_for_authoring": False}))
        counts = []
        for profile in ("native", "compat"):
            qs = reviewed[(name, profile)]
            expected = CFG["counts"][name][0 if profile == "native" else 1]
            base.require(len(qs) == expected and len({q["id"] for q in qs}) == expected, "repo rows")
            content = b"".join((json.dumps(q, ensure_ascii=False, separators=(",", ":")) + "\n").encode() for q in qs)
            query_path = put(out, prefix + "queries." + profile + ".dev.jsonl", content)
            suite = {"schema_version": 1, "name": "Reviewed public DEV " + name + " " + profile,
                     "source": {"root": "source", "commit": None, "digest": "", "files": [r["path"] for r in source]},
                     "queries": query_path.name, "queries_digest": "",
                     "scoring": "codecortex-native-v1" if profile == "native" else "oce-compat-v1",
                     "repetitions": 3, "warmup": 0, "seed": 20261003,
                     "timeout_ms": 30000, "top_k": 10, "engine_config": {"auto_index": {"enabled": False}}}
            suite_path = put(out, prefix + "suite." + profile + ".dev.json", json_bytes(suite))
            base.command(out, "freeze-" + name + "-" + profile, [evaluator, "freeze", "--suite", suite_path])
            frozen = json.loads(base.read_file(suite_path))
            compare = copy.deepcopy(frozen)
            compare["source"]["digest"], compare["queries_digest"] = "", ""
            base.require(compare == suite, "freeze changed more than content digests")
            base.require(base.sha(base.read_file(query_path)) == base.sha(content), "freeze changed query")
            suites.append(suite_path)
            shard_args.extend(["--shard", name + ":" + profile + "=" + str(query_path)])
            counts.append(len(qs))
        native = reviewed[(name, "native")]
        spans = sum(len(g["alternatives"]) for q in native for g in q["answers"])
        base.require(spans == CFG["counts"][name][3], "gold span inventory")
        statistics[name] = {"native": counts[0], "compat": counts[1], "source_files": len(source),
                            "gold_spans": spans, "no_answer": sum(q["no_answer"] for q in native)}
    relations = out / "evidence/global-components.json"
    schema_cmd = [sys.executable, HERE / "protocol/check.py", *shard_args, "--relations", relations,
                  "--evaluator", evaluator, "--output", out / "original-schema-and-v02.json"]
    for suite in suites:
        schema_cmd.extend(["--suite", suite])
    base.command(out, "original-schema-and-v02", schema_cmd, 1800)
    check = json.loads(base.read_file(out / "original-schema-and-v02.json"))
    base.require(check["errors"] == {} and check["native_unique_query_ids"] == 327
                 and check["compat_unique_query_ids"] == 282
                 and check["global_family_components"] == 305
                 and len(check["actual_evaluator_checks"]) == 12
                 and all(c["exit_code"] == 0 for c in check["actual_evaluator_checks"]),
                 "original schema/V02 acceptance")
    base.require(base.verify_archive(repo)[1] == original_before, "original archives changed")
    for path, digest in new_before.items():
        base.require(base.sha(base.read_relative(repo, path)) == digest, "new candidate/source changed")
    base.require(base.sha(base.read_file(evaluator, 512 * 1024 * 1024)) == evaluator_sha, "binary changed")
    for path in sorted((out / "crates").rglob("*")):
        if path.is_file():
            raw = base.read_file(path)
            inventory.append({"path": path.relative_to(out).as_posix(), "bytes": len(raw),
                              "sha256": base.sha(raw), "mode": oct(path.stat().st_mode & 0o777)})
    result = {"schema_version": 1, "status": "reviewed_six_repository_public_dev_projection_validated",
              "evaluator_sha256": evaluator_sha, "repositories": statistics,
              "totals": {"native_rows": 327, "compat_rows": 282, "source_files": 132,
                         "gold_spans": 509, "correlated_components": 305},
              "changed_canonical_annotations": {"native": 9, "compat": 7},
              "schema_checker_sha256": CFG["protocol"]["checker_sha256"],
              "gold_review_receipts": CFG["reviews"], "historical_gold_admission_sha256": ADMISSION_SHA256,
              "component_review": CFG["canonical"],
              "original_source_author_bytes_unchanged": True, "provider_calls": 0, "ranking_runs": 0,
              "protected_body_reads": 0, "reserved_serde_vite_slots_undrafted": 14,
              "limits": ["Public DEV source subsets only; not full checkouts.",
                         "Components express known correlation, not proven statistical independence.",
                         "File compatibility and native facet/span scoring are different views.",
                         "No semantic ranking benefit, graph precision/recall, protected holdout or release claim.",
                         "Old author and canonical-only snapshots retain pending annotations; this separate reviewed projection cites independent source/gold evidence."],
              "canonical_product_inventory": inventory}
    base.write_json(out / "receipt.json", result)
    return result

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--repo-root", type=Path, required=True)
    p.add_argument("--evaluator", type=Path, required=True)
    p.add_argument("--evaluator-sha256", required=True)
    p.add_argument("--original-replay-receipt", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    base.require(not a.output.exists(), "output exists; preserve prior attempt")
    a.output.mkdir(parents=True)
    try:
        result = run(a.repo_root.resolve(), a.evaluator.absolute(), a.evaluator_sha256,
                     a.original_replay_receipt.absolute(), a.output.absolute())
    except Exception as exc:
        base.write_json(a.output / "failure.json", {"status": "failed", "exception": type(exc).__name__,
                        "message": str(exc), "not_accepted": True})
        raise
    print(json.dumps({"status": result["status"], "totals": result["totals"],
                      "receipt_sha256": base.sha(base.read_file(a.output / "receipt.json"))}))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
