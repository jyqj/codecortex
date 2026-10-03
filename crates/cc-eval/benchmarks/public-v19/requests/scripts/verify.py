"""Read-only source/gold integrity checks, distinct from independent gold review."""
import ast
import hashlib
import json
import pathlib
import subprocess
import sys
from author import ROOT, SHA, reference, digest, write


def main():
    inventory = json.loads((ROOT / "provenance/inventory.json").read_text())
    lock = json.loads((ROOT / "provenance/upstream-lock.json").read_text())
    assert inventory["source_sha"] == lock["source_sha"] == SHA
    admitted = {}
    for entry in inventory["files"]:
        if not entry["included"]:
            continue
        folder = "license" if entry["path"] in ("LICENSE", "NOTICE") else "source"
        path = ROOT / folder / entry["path"]
        assert not path.is_symlink()
        raw = path.read_bytes()
        assert digest(raw) == entry["sha256"] and len(raw) == entry["bytes"]
        if folder == "source":
            admitted[entry["path"]] = entry
    assert digest((ROOT / "license/LICENSE").read_bytes()) == lock["license_files"][0]["sha256"]
    specs = json.loads((ROOT / "questions/specifications.json").read_text())
    specmap = {s["id"]:s for s in specs}
    allrows, seen, clusters, scopes = [], set(), {}, []
    for split in ("dev",):
        rows = [json.loads(l) for l in (ROOT / "queries.native.dev.jsonl").read_text().splitlines()]
        gold = json.loads((ROOT / "gold" / (split + ".json")).read_text())
        byid = {g["family_id"]:g for g in gold}
        assert set(byid) == {r["query_family"] for r in rows}
        for row in rows:
            assert row["id"] not in seen
            seen.add(row["id"])
            s = specmap[row["query_family"]]
            assert row["split"] == split and row["id"] == row["query_family"] + ".en01"
            assert clusters.setdefault(s["global_family"], split) == split
            assert row["annotations"]["v19"]["global_family"] == s["global_family"]
            g = byid[row["query_family"]]
            assert g["source_sha"] == SHA and g["status"] == "candidate_not_independently_reviewed"
            for key, ev in zip(s["refs"], g["evidence"], strict=True):
                alt, expected = reference(key)
                assert ev == expected and ev["path"] in admitted
                # Never admit the explicitly adapted Werkzeug helper spans as gold.
                notices = json.loads((ROOT / "license/inclusion-review.json").read_text())
                for excluded in notices["adapted_third_party_helpers"]:
                    assert ev["path"] != excluded["path"] or ev["end_byte"] <= excluded["start_byte"] or ev["start_byte"] >= excluded["end_byte"]
                if not row["no_answer"]:
                    assert any(alt in group["alternatives"] for group in row["answers"])
            for edge in g["edges"]:
                raw = (ROOT / "source" / edge["path"]).read_bytes()
                assert raw[edge["start_byte"]:edge["end_byte"]].decode() == edge["expression"]
                # Every callee must resolve to a real symbol in pinned source.
                reference(edge["callee"])
                assert edge["source_sha"] == SHA
            if s["category"] == "crossfile":
                assert len({r.split(":")[0] for r in s["refs"]}) >= 2
                assert g["edges"]
            if row["no_answer"]:
                assert not row["answers"] and not row["expected_files"] and s["scope"]
                a = s["absence"]
                if a["type"] == "missing_class":
                    tree = ast.parse((ROOT / "source/src/requests" / (a["module"] + ".py")).read_bytes())
                    symbols = sorted(n.name for n in ast.walk(tree) if isinstance(n, ast.ClassDef))
                    assert a["name"] not in symbols
                    checked = dict(method="AST class enumeration", class_names=symbols)
                elif a["type"] == "assignment_not_member":
                    tree = ast.parse((ROOT / "source/src/requests" / (a["module"] + ".py")).read_bytes())
                    assignments = [n for n in ast.walk(tree) if isinstance(n, ast.AnnAssign) and isinstance(n.target, ast.Name) and n.target.id == a["name"]]
                    values = ast.literal_eval(assignments[0].value)
                    assert a["missing"] not in values
                    checked = dict(method="AST literal declaration and source-read consumers", values=values)
                elif a["type"] == "no_calls":
                    alt, ev = reference(a["ref"])
                    raw = (ROOT / "source" / ev["path"]).read_bytes()[ev["start_byte"]:ev["end_byte"]].decode()
                    tree = ast.parse(__import__('textwrap').dedent(raw))
                    calls = sorted({ast.unparse(n.func) for n in ast.walk(tree) if isinstance(n, ast.Call)})
                    assert not any(c in calls for c in a["missing"])
                    assert a["anchor"] in raw
                    checked = dict(method="AST complete scoped call enumeration plus author semantic source read", calls=calls, implementation_sha256=digest(raw.encode()))
                else:
                    alt, ev = reference(a["ref"])
                    raw = (ROOT / "source" / ev["path"]).read_bytes()[ev["start_byte"]:ev["end_byte"]].decode()
                    assert a["literal"] in raw and a["also"] in raw
                    checked = dict(method="literal guard plus author source-read exhaustive scoped implementation", implementation_sha256=digest(raw.encode()))
                scopes.append(dict(family_hash=digest(row["id"].encode()), scope=s["scope"], checked=checked))
        allrows.extend(rows)
    assert not any(r["split"] == "holdout" for r in allrows)
    # Candidate SHA-256 locks bind review artifacts as well as evaluator BLAKE3 locks.
    paths = list(ROOT.glob("queries.*.dev.jsonl")) + list((ROOT / "questions").glob("*specifications.json")) + list((ROOT / "gold").glob("*.json")) + [ROOT / "suite-native-dev.json", ROOT / "suite-compat-dev.json"]
    receipt = dict(status="author_integrity_verified_not_independent_review",
        source_sha=SHA, admitted_source_files=len(admitted), license_verified=True,
        families=len(allrows), dev=sum(r["split"] == "dev" for r in allrows),
        holdout_bodies_in_public_projection=0, holdout_custody_status="holdout_custody_blocked",
        absence_checks=scopes, retrieval_executed=False,
        sha256={str(p.relative_to(ROOT)):digest(p.read_bytes()) for p in paths})
    if len(sys.argv) > 1:
        binary = pathlib.Path(sys.argv[1]).resolve()
        outcomes = []
        for profile in ("native", "compat"):
            command = [str(binary), "validate", "--suite", str(ROOT / ("suite-" + profile + "-dev.json"))]
            proc = subprocess.run(command, capture_output=True, text=True)
            outcomes.append(dict(command=command, exit_code=proc.returncode,
                                 diagnostics_sha256=digest((proc.stdout+proc.stderr).encode())))
            assert proc.returncode == 0, proc.stderr
        receipt["evaluator"] = dict(binary_sha256=digest(binary.read_bytes()), validations=outcomes)
    write(ROOT / "provenance/validation.json", receipt)
    print(json.dumps({k:v for k,v in receipt.items() if k not in ("absence_checks","sha256","evaluator")}))


if __name__ == "__main__":
    main()
