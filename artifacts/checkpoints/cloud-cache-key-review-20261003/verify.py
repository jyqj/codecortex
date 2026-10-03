#!/usr/bin/env python3
"""Read-only receipt checks for the frozen PR51 and independent observations."""
import hashlib
import json
import pathlib
import re
import subprocess

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
BASE = "a7efaae70cd0828b1a1b2d811e20176d855394b3"
rows = {}
for tool in ("msrv", "199"):
    query = json.loads((HERE / f"observations-{tool}/distinct-query-consumption.json").read_text())
    assert len(query["cases"]) == 11 and len(query["requests"]) == 9
    assert [c["consumer_value"][0] for c in query["cases"]] == [10,10,20,30,40,50,60,70,80,90,10]
    assert [c["expected_hit"] for c in query["cases"]] == [False,True,False,False,False,False,False,False,False,False,True]
    count = 0
    for c in query["cases"]:
        count += int(not c["expected_hit"])
        assert c["posts_after"] == c["factories_after"] == c["cache_entries"] == count
    assert not query["test_oracle_constructs_keys"]
    schema = json.loads((HERE / f"observations-{tool}/schema-reopen.json").read_text())
    old, new = schema["old_generation"], schema["after_mismatch"]
    assert old["incarnation"] != new["incarnation"]
    assert new["index_epoch"] > old["index_epoch"] and new["evidence_epoch"] > old["evidence_epoch"]
    assert schema["misses"] == schema["hits"] == 1
    assert any("222" in h["text"] for h in schema["new_hits"])
    assert all("111" not in h["text"] for h in schema["new_hits"])
    replay = json.loads((HERE / f"pr51-replay-{tool}/query-axes.json").read_text())
    assert len(replay["cases"]) == 13 and len(replay["requests"]) == 11
    for c in replay["cases"]:
        delta = int(not c["expected_hit"])
        assert c["transport_posts_after"] - c["transport_posts_before"] == delta
        assert c["factories_after"] - c["factories_before"] == delta
    for name, count in ((f"independent-{tool}.log",2),(f"pr51-replay-{tool}.log",3)):
        assert f"test result: ok. {count} passed; 0 failed" in (HERE / name).read_text()
    rows[tool] = {"new_tests_passed":2,"pr51_tests_passed":3,"distinct_value_cases":11,
                  "independent_adapter_posts":9,"pr51_axis_cases":13,"pr51_adapter_posts":11,
                  "schema_index_epochs":[old["index_epoch"],new["index_epoch"]],
                  "schema_evidence_epochs":[old["evidence_epoch"],new["evidence_epoch"]]}
files = ["Cargo.toml","Cargo.lock","crates/cc-server/tests/p7_v11_cache_key_matrix.rs",
         "crates/cc-server/src/semantic_query_encoding.rs","crates/cc-server/src/semantic_provider_factory.rs",
         "crates/cc-server/src/semantic_wiring.rs","crates/cc-server/src/engine.rs",
         "crates/cc-semantic/src/cache.rs","crates/cc-semantic/src/spec.rs",
         "crates/cc-semantic/src/providers/openai_compatible.rs","crates/cc-search/src/engine.rs",
         "crates/cc-search/src/engine_cache.rs","crates/cc-search/src/engine_graph.rs",
         "crates/cc-model/src/generation.rs","crates/cc-model/src/identity.rs",
         "docs/roadmap/code-index-v2/02-CONTRACTS.md","crates/cc-semantic/docs/ENCODING-SPACE.md"]
hashes = {}
for path in files:
    content = (ROOT/path).read_bytes()
    assert content == subprocess.check_output(["git","show",BASE+":"+path],cwd=ROOT), path
    hashes[path] = hashlib.sha256(content).hexdigest()
test = ROOT / "crates/cc-server/tests/cache_key_review_behavior.rs"
code = "\n".join(line for line in test.read_text().splitlines() if not line.lstrip().startswith("//"))
assert "QueryCacheKey" not in code and "DefaultHasher" not in code
assert "panic!(\"production cache consumer missed a populated key\")" in code
assert "Finished" in (HERE / "clippy-199.log").read_text()
summary = {"frozen_subject_sha":BASE,"status":"PASS bounded independent review; not full C12/V11 acceptance",
           "oracle":"provider/factory counts plus distinct production cache-consumer outputs; no test key construction",
           "observed":rows,"frozen_files_sha256":hashes,"new_test_sha256":hashlib.sha256(test.read_bytes()).hexdigest(),
           "production_and_subject_tests_unchanged":True,"strict_clippy_199":"PASS",
           "documentation_finding":"dimension is serialized into current SpaceDigest despite prose saying it is excluded",
           "not_run":["parser/project-model/rule version upgrades","compiled encoding/document/policy/selector versions",
                      "every immutable config field/repo tier/top_k-only graph variant","public semantic model/query-spec/document-template transition",
                      "adjacent21->22 migration/old live handles/dense final-context concurrent freshness"],
           "initial_attempts":"wrong fixture DB facet compile failure and missing JSON Content-Type fixture failure retained; not passing observations"}
(HERE / "receipt.json").write_text(json.dumps(summary,indent=2,sort_keys=True)+"\n")
print(json.dumps({"subject":BASE,"results":rows,"unchanged_sources":len(files),"status":summary["status"]},indent=2))
