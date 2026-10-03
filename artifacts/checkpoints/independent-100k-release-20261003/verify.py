#!/usr/bin/env python3
"""Verify evidence identity/completeness, not an overall product-pass assertion."""
import gzip,hashlib,json,pathlib,subprocess
def load_json(path):
    if path.exists():return json.loads(path.read_text())
    with gzip.open(str(path)+".gz","rt") as f:return json.load(f)

OUT=pathlib.Path(__file__).resolve().parent;ROOT=OUT.parents[2]
SHA="574f7598662334c63e020da136c87f4f7281554d"
build=json.loads((OUT/"build-receipt.json").read_text());summary=load_json(OUT/"summary.json")
assert build["source_sha"]==summary["source_sha"]==SHA
assert build["build_exit_code"]==0 and build["guard_stop"] is None
assert sorted(build["compiler_artifact"]["features"])==summary["features"]==["semantic","semantic-http"]
assert build["compiler_artifact"]["profile"]["opt_level"]=="3" and not build["compiler_artifact"]["profile"]["debug_assertions"]
assert hashlib.sha256(pathlib.Path(build["compiler_artifact"]["executable"]).read_bytes()).hexdigest()==summary["binary_sha256"]==build["binary_sha256"]
assert not summary["full_V20"] and not summary["valid_for_other_source_trees"]
subprocess.run(["git","diff","--exit-code",SHA,"--","crates","scripts","Cargo.toml","Cargo.lock","docs"],cwd=ROOT,check=True)
for prefix in [OUT/"attempt01",OUT/"attempt02",OUT]:
    if not (prefix/"summary.json").exists() and not (prefix/"summary.json.gz").exists():continue
    r=load_json(prefix/"summary.json");prereg=json.loads((prefix/"preregistration.json").read_text())
    assert r["timeouts"]==prereg["timeouts"]==summary["timeouts"]
    assert r["resource_limits"]==prereg["resource_limits"]==summary["resource_limits"]
    assert r["source_sha"]==SHA and r["binary_sha256"]==summary["binary_sha256"]
    if r["status"]=="failed":assert r["failures"] and (prefix/"run-failure.log").exists()
    h=hashlib.sha256();count=0;logical=0
    with gzip.open(prefix/"live/n100000/source-inputs.jsonl.gz","rt") as f:
        for line in f:
            h.update(line.encode());count+=1;row=json.loads(line);logical+=row["bytes"]
    generation=next(x["result"] for x in r["checks"] if x["name"]=="generation")
    assert count==100000 and h.hexdigest()==generation["ordered_manifest_sha256"] and logical==generation["logical_bytes"]
    assert (prefix/"live/n100000/resources.jsonl.gz").exists() and (prefix/"live/n100000/rpc.jsonl.gz").exists()
held=json.loads((OUT/"live/n100000/held-db.json").read_text())
assert held["counts"]["files"]==100000 and held["counts"]["edge_tables"]["co_change_edges"]==0
assert held["integrity"]=="ok" and held["foreign_key_errors"]==0
if summary["status"]=="passed_declared_100k_local_scope":
    assert summary["failures"]==[] and summary["not_run"]==[]
    final=json.loads((OUT/"live/n100000/final-db.json").read_text());reopened=json.loads((OUT/"live/n100000/reopen-db.json").read_text())
    assert final["counts"]["files"]==final["counts"]["document_manifest"]==final["counts"]["semantic_manifest"]==100000
    assert final["counts"]==reopened["counts"] and final["integrity"]==reopened["integrity"]=="ok"
result={"evidence_identity_and_retention":"passed","product_outcome":summary["status"],"source_sha":SHA,"binary_sha256":summary["binary_sha256"],"full_V20":False}
(OUT/"verification.json").write_text(json.dumps(result,indent=2)+"\n");print(json.dumps(result))
