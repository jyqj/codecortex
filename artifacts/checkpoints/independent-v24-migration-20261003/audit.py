"""Read fixed Git evidence, independently derive a tiny oracle, never run product writes.

Author receipt checks are evidence auditing, not an independent product execution.
Only this script's Python/SQLite immutable observations and model checks are run here.
"""
import ast
import gzip
import hashlib
import io
import json
import pathlib
import re
import sqlite3
import subprocess
import tarfile

ROOT = pathlib.Path(__file__).resolve().parent
REPO = ROOT.parents[2]
FINAL = "6db4d396e5d994388ada1e97c3d28947ffdb9c81"
V22 = "574f7598662334c63e020da136c87f4f7281554d"
V23 = "57bedbaf193e28be34271a290d28c90dc662b613"
V24 = "9ebdb155c64e094b3d774be41dbe7ab8c4e222c1"
SAME = "bf10b6477612f32d991bbfea3ed7791f468acc78"
BASE = "artifacts/checkpoints/index-fix-integration-20261003/"


def git(*args):
    return subprocess.check_output(["git", "-C", str(REPO), *args])


def blob(path, sha=FINAL):
    return git("show", sha + ":" + path)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def load(path):
    return json.loads(blob(BASE + path))


def report(item):
    r = item["response"]
    assert "error" not in r and not r["result"].get("isError"), r
    return json.loads(r["result"]["content"][0]["text"])["result"]


def source_oracle(source, suffix):
    """Derive consumer/target from syntax, never from receipt expected fields."""
    if suffix == ".py":
        tree = ast.parse(source)
        classes = [n.name for n in tree.body if isinstance(n, ast.ClassDef)]
        functions = [n for n in tree.body if isinstance(n, ast.FunctionDef)]
        assert len(classes) == len(functions) == 1
        f = functions[0]
        if f.returns is not None:
            assert isinstance(f.returns, ast.Name) and f.returns.id == classes[0]
        assert any(isinstance(a.annotation, ast.Name) and a.annotation.id == classes[0]
                   for a in f.args.args)
        return f.name, classes[0]
    # Deliberately only the minimal ASCII TypeScript fixture grammar, not a TS parser.
    ident = r"[$A-Za-z_][$A-Za-z_0-9]*"
    classes = re.findall(r"\bclass\s+(" + ident + r")\s*\{", source)
    f = re.search(r"\bfunction\s+(" + ident + r")\s*\(\w+\s*:\s*(" + ident
                  + r")\s*\)\s*:\s*(" + ident + r")", source)
    assert len(classes) == 1 and f and f[2] == classes[0]
    assert f[3] in (classes[0], "void")
    return f[1], classes[0]


def model_checks():
    # Independently chosen consumer and sources. Uses Python's actual parser.
    def reopen(state, wanted):
        if state["schema"] == wanted:
            return state.copy()
        return dict(schema=wanted, incarnation="replacement", epoch=state["epoch"] + 1,
                    files=0, edges=())

    def incremental(state, target):
        if state["files"]:
            return state.copy(), 0
        return dict(state, epoch=state["epoch"] + 1, files=1,
                    edges=(("review_use", target),)), 1

    rows = []
    for name in ["_", "__", "℘", "℮", "$"]:
        suffix = ".ts" if name == "$" else ".py"
        src = (f"class {name} {{}}\nfunction review_use(x: {name}): {name} {{ throw 0; }}\n"
               if suffix == ".ts" else
               f"class {name}:\n    pass\ndef review_use(x: {name}) -> {name}:\n    pass\n")
        consumer, target = source_oracle(src, suffix)
        assert consumer == "review_use" and target == name
        # Migration state machine contract independent of implementation details.
        for old in (22, 23):
            before = dict(schema=old, incarnation="old", epoch=9, files=1, edges=())
            # Negative control: same version leaves the stale edge set untouched.
            stale, parsed = incremental(reopen(before, old), target)
            assert stale == before and parsed == 0 and not stale["edges"]
            opened = reopen(before, 24)
            assert opened["files"] == 0 and opened["incarnation"] != before["incarnation"]
            assert opened["epoch"] > before["epoch"]
            indexed, parsed = incremental(opened, target)
            assert parsed == 1 and indexed["edges"] == ((consumer, target),)
            noop, parsed = incremental(reopen(indexed, 24), target)
            assert parsed == 0 and noop == indexed
            rows.append(dict(schema=old, name=target, source_sha256=digest(src.encode()),
                             expected_relation="uses_type", expected_bucket=target.lower()))
    # Manifest acceptance matrix; this specifies the oracle, not the product result.
    matrix = {str(reader): {str(payload): reader == payload for payload in (1, 2, 3)}
              for reader in (1, 2, 3)}
    assert not matrix["1"]["3"] and not matrix["2"]["3"]
    assert not matrix["3"]["1"] and not matrix["3"]["2"]
    return dict(status="MODEL_ONLY_PASS", fixtures=rows, manifest_matrix=matrix)


def main():
    out = dict(candidate=V24, evidence_commit=FINAL,
               classification="AUTHOR_EVIDENCE_AUDIT; NOT PRODUCT_RERUN",
               model=model_checks())
    # Hash all final artifact bytes independently against their published manifest.
    manifest = load("final-v24/artifact-manifest.json")
    mismatches = [name for name, expected in manifest.items()
                  if digest(blob(BASE + "final-v24/" + name)) != expected]
    out["artifact_hashes"] = dict(checked=len(manifest), mismatches=mismatches)
    # Check actual Git source bytes against build receipts; do not read gold/heldout fixtures.
    historical = load("build-receipts.json")
    builds = load("final-v24/build-receipts.json")
    identities = []
    for label, receipt, sha in [("v22", historical["old101"], V22),
                                ("v23-actual-old", historical["fixed"], V23),
                                ("v23-repaired-sameversion", builds["same-v23"], SAME),
                                ("v24", builds["final-v24"], V24)]:
        files = receipt["source_file_sha256"]
        selected = {n: h for n, h in files.items()
                    if n in ("Cargo.toml", "Cargo.lock") or n.endswith((".rs", ".sql", "/Cargo.toml"))}
        data = git("archive", sha, "Cargo.toml", "Cargo.lock", "crates")
        with tarfile.open(fileobj=io.BytesIO(data)) as tar:
            bad = [n for n, h in selected.items() if digest(tar.extractfile(n).read()) != h]
        identities.append(dict(label=label, source_sha=sha, binary_sha256=receipt["binary_sha256"],
                               features=receipt["features"], checked_source_files=len(selected),
                               source_hash_mismatches=bad, binary_rehashed_here=False,
                               target=receipt.get("target")))
    out["author_build_identities"] = identities
    assert len({x["binary_sha256"] for x in identities}) == 4
    assert builds["final-v24"]["target"] != builds["same-v23"]["target"]
    intermediate = load("intermediate-v23/receipt.json")
    cases = {c["label"]: c for c in intermediate["cases"]}
    snapshots, oracles = {}, {}
    (ROOT / "runtime" / "evidence-snapshots").mkdir(parents=True, exist_ok=True)
    for label, c in cases.items():
        raw = blob(BASE + "intermediate-v23/" + label + "-" + c["input_file"])
        assert digest(raw) == c["input_sha256"]
        consumer, name = source_oracle(raw.decode(), pathlib.Path(c["input_file"]).suffix)
        oracles[label] = (consumer, name)
        dbbytes = gzip.decompress(blob(BASE + "intermediate-v23/" + label + ".sqlite3.gz"))
        assert digest(dbbytes) == c["snapshot_sha256"]
        path = ROOT / "runtime" / "evidence-snapshots" / (label + ".sqlite3")
        path.write_bytes(dbbytes)
        with sqlite3.connect(path.as_uri() + "?mode=ro&immutable=1", uri=True) as conn:
            schema = conn.execute("PRAGMA user_version").fetchone()[0]
            versions = [r[0] for r in conn.execute("SELECT DISTINCT version FROM resolution_manifests")]
            edges = list(conn.execute("SELECT source_symbol,target_symbol,target_symbol_uid FROM semantic_edges WHERE relation_kind='uses_type'"))
            generation = dict(conn.execute("SELECT key,value FROM metadata WHERE key IN ('index_incarnation','index_epoch','evidence_epoch')"))
            assert schema == 23 and versions == [2] and edges == []
            assert generation == c["full"]["after"]["generation"]
        snapshots[label] = dict(schema=schema, manifests=versions, type_edges=edges,
                                snapshot_sha256=digest(dbbytes), source_sha256=digest(raw),
                                recorded_mtime_ns=c["input_mtime_ns"], mtime_reobserved=False)
    out["immutable_v23_snapshot_reads"] = snapshots
    migration = load("final-v24/identifier-migrations.json")
    rows = []
    assert len(migration["paths"]) == 10
    for p in migration["paths"]:
        consumer, name = oracles[p["label"]]
        before, opened, after = p["before"], p["upgrade"]["opened"], p["upgrade"]["after"]
        old = 22 if p["origin"] == "pr101-v22" else 23
        assert before["schema"] == old and before["versions"] == [1 if old == 22 else 2]
        if old == 23:
            assert before == cases[p["label"]]["full"]["after"]
        assert opened["schema"] == 24 and opened["files"] == 0
        assert opened["generation"]["index_incarnation"] != before["generation"]["index_incarnation"]
        for epoch in ("index_epoch", "evidence_epoch"):
            assert int(opened["generation"][epoch]) > int(before["generation"][epoch])
        assert after["schema"] == 24 and after["versions"] == [3] and after["files"] == 1
        edge = p["after_types"]["edges"]
        assert len(edge) == 1 and edge[0][:2] == [consumer, name] and edge[0][2]
        assert name.lower() in p["after_types"]["buckets"]
        assert p["noop"]["opened"] == p["noop"]["after"] == after
        assert report(p["noop"])["files_parsed"] == 0 and report(p["noop"])["files_skipped"] == 1
        assert report(p["upgrade"])["files_parsed"] == 1
        rows.append(dict(origin=p["origin"], label=p["label"], independently_parsed_target=name,
                         old_schema=old, status="EVIDENCE_CONSISTENT", actual_product_rerun=False))
    assert len({(r["origin"], r["label"]) for r in rows}) == 10
    out["ten_migration_paths"] = rows
    same = load("same-v23-diagnostic/receipt.json")
    for c in same["cases"]:
        assert c["before"] == c["unchanged_noop"]["after"]
        assert report(c["unchanged_noop"])["files_parsed"] == 0
        assert c["after_types"]["edges"] == [] and len(c["fresh_types"]["edges"]) == 1
    out["sameversion_negative_control"] = dict(cases=len(same["cases"]), status="EVIDENCE_CONSISTENT")
    pinned = load("final-v24/pinned-boundaries.json")
    for r in pinned["readers"]:
        assert r["old_source_sha"] == (V22 if r["old_schema"] == 22 else V23)
        assert r["reader_source_sha"] == (V22 if r["old_schema"] == 22 else SAME)
        assert "unsupported version" in r["after"]["manifest_error"]
        assert r["after"]["generation"] != r["before"]["generation"]
        assert not r["after"]["query_error"] and "corrected_marker" in r["after"]["query"]
    out["pinned_boundary"] = dict(status="EVIDENCE_CONSISTENT", readers=len(pinned["readers"]),
                                  reader_v23_is_repaired_sameversion=True, actual_rerun=False)
    logs = load("final-v24/validation-log-manifest.json")
    data = gzip.decompress(blob(BASE + "final-v24/all-features-complete-no-fail-fast.log.gz"))
    assert digest(data) == logs["all-features-complete-no-fail-fast"]["raw_sha256"]
    summaries = re.findall(rb"test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored;", data)
    totals = tuple(sum(int(r[i]) for r in summaries) for i in range(3))
    assert totals == (2474, 4, 68) and len(summaries) == 148
    out["unchanged_all_features_boundary"] = dict(passed=totals[0], failed=totals[1], ignored=totals[2], suites=len(summaries), overall_acceptance=False)
    out["assertion_result"] = "EVIDENCE_AND_MODEL_CHECKS_PASS"
    (ROOT / "audit-results.json").write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"assertion_result": out["assertion_result"], "artifact_hash_mismatches": mismatches,
                      "migration_paths": len(rows), "product_reruns": 0}, ensure_ascii=False))


if __name__ == "__main__":
    main()
