"""Materialize source-read candidates; never imports Requests or runs retrieval."""
import ast
import hashlib
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
SHA = "611c6162cbc4ac2020a2f91c7cfa4f3abf9bbb60"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2) + "\n")


def reference(key):
    module, symbol = key.split(":")
    path = "src/requests/" + module + ".py"
    raw = (ROOT / "source" / path).read_bytes()
    tree = ast.parse(raw)
    nodes = []

    def visit(node, parents=()):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)):
            parents = parents + (node.name,)
            if ".".join(parents) == symbol:
                nodes.append(node)
        for child in ast.iter_child_nodes(node):
            visit(child, parents)

    visit(tree)
    # Overload declarations are not implementation evidence.
    node = nodes[-1]
    lines = raw.splitlines(keepends=True)
    start = sum(map(len, lines[:node.lineno - 1])) + node.col_offset
    end = sum(map(len, lines[:node.end_lineno - 1])) + node.end_col_offset
    return dict(path=path, symbol=dict(name=node.name,
                qname="requests." + module + "." + symbol,
                kind="class" if isinstance(node, ast.ClassDef) else "function"),
                span=dict(start=start, end=end)), dict(source_sha=SHA,
                path=path, symbol=symbol, start_line=node.lineno,
                end_line=node.end_lineno, start_byte=start, end_byte=end,
                span_sha256=digest(raw[start:end]), file_sha256=digest(raw))


def main():
    specs = json.loads((ROOT / "questions/specifications.json").read_text())
    extension = ROOT / "questions/extension-specifications.json"
    if extension.exists():
        specs += json.loads(extension.read_text())
    if len(sys.argv) > 1:
        specs = specs[:int(sys.argv[1])]
    # Exact 75/25 for this candidate batch; related families are indivisible.
    # Choose a deterministic subset by hash order and subset-sum, not by scores.
    clusters = {}
    for s in specs:
        clusters.setdefault(s["cluster"], []).append(s["id"])
    ordered = sorted(clusters, key=lambda k: digest(k.encode()))
    possibilities = {0: []}
    for c in ordered:
        for n, chosen in list(possibilities.items()):
            possibilities.setdefault(n + len(clusters[c]), chosen + [c])
    target = len(specs) // 4
    assert target in possibilities, "cannot split related groups at target"
    holdout = set(possibilities[target])
    rows = []
    gold = []
    for s in specs:
        groups, evidence = [], []
        for i, key in enumerate(s["refs"]):
            answer, ev = reference(key)
            evidence.append(ev)
            if not s.get("no_answer"):
                groups.append(dict(id="facet-" + str(i + 1), primary=True,
                                   grade=3, alternatives=[answer]))
        edges = []
        for caller, callee, expression in s.get("edges", []):
            _, ev = reference(caller)
            raw = (ROOT / "source" / ev["path"]).read_bytes()
            begin = raw.index(expression.encode(), ev["start_byte"], ev["end_byte"])
            edges.append(dict(caller=caller, callee=callee, expression=expression,
                              source_sha=SHA, path=ev["path"], start_byte=begin,
                              end_byte=begin + len(expression.encode()),
                              line=raw[:begin].count(b"\n") + 1,
                              kind=s.get("edge_kind", "call")))
        annotations = dict(status="candidate_not_independently_reviewed",
                           source_sha=SHA, related_family_cluster=s["cluster"],
                           family_hash=digest(s["id"].encode()),
                           candidate_split_method="sha256_cluster_order_subset_sum_75_25_v1",
                           global_split_frozen=False)
        if s.get("no_answer"):
            annotations["absence_scope"] = s["scope"]
        row = dict(id=s["id"], query_family=s["id"], category=s["category"],
                   difficulty=3 if s["category"] in ("crossfile", "hardnegative") else 2,
                   language="python", split="holdout" if s["cluster"] in holdout else "dev",
                   query=s["question"], path_prefix=s.get("prefix"),
                   no_answer=bool(s.get("no_answer")), expected_files=[],
                   answers=groups, annotations=annotations)
        rows.append(row)
        gold.append(dict(family_id=s["id"], source_sha=SHA, split=row["split"],
                         status="candidate_not_independently_reviewed",
                         rationale=s["answer"], evidence=evidence, edges=edges,
                         absence=s.get("absence"), scope=s.get("scope")))
    for split in ("dev", "holdout"):
        selected = [r for r in rows if r["split"] == split]
        qpath = ROOT / "questions" / (split + ".jsonl")
        qpath.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in selected))
        write(ROOT / "gold" / (split + ".json"), [r for r in gold if r["split"] == split])
        files = sorted(str(p.relative_to(ROOT / "source")) for p in (ROOT / "source").rglob("*") if p.is_file())
        write(ROOT / ("suite-" + split + ".json"), dict(schema_version=1,
            name="V19 Requests candidate " + split, source=dict(root="source", commit=None,
            digest="AUTHORING_PENDING", files=files), queries="questions/" + split + ".jsonl",
            queries_digest="AUTHORING_PENDING", scoring="codecortex-native-v1",
            repetitions=1, warmup=0, seed=20261003, timeout_ms=30000, top_k=10,
            engine_config=dict(auto_index=dict(enabled=False))))
    write(ROOT / "review/preparation.json", dict(status="awaiting_independent_author_review",
        author_may_sign_independent_review=False, proposed_reviewer_owner="E/gin",
        family_count=len(rows), independently_reviewed=0,
        dev=sum(r["split"] == "dev" for r in rows), holdout=target,
        categories={c:sum(r["category"] == c for r in rows) for c in sorted({r["category"] for r in rows})},
        family_set_sha256=digest("\n".join(sorted(r["id"] for r in rows)).encode()),
        split_sha256=digest(json.dumps({r["id"]:r["split"] for r in rows},sort_keys=True).encode()),
        global_split_frozen=False, retrieval_run=False,
        notes="Candidate split is batch-local; integrator must audit global leakage before freezing. No holdout prose in progress reports."))
    print(json.dumps(json.loads((ROOT / "review/preparation.json").read_text())))


if __name__ == "__main__":
    raise SystemExit("Legacy quota authoring disabled after protocol-v1 migration. Use verify.py for public dev reproduction; migrate_protocol.py requires original custody-blocked draft inputs and is not a holdout custody solution.")
