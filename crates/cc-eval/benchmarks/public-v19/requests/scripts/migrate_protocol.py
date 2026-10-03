"""One-time, pre-ranking protocol metadata migration; public output excludes holdout.

Requires preserved preprotocol local inputs. Those inputs have NO access-control
boundary and are custody-blocked; do not pass this directory to a tuning owner.
"""
import copy
import hashlib
import json
from collections import Counter
from author import ROOT, SHA, digest, reference, write

PREFIX = b"codecortex-public-v19-split-v1\n"


def split(component):
    return "holdout" if int.from_bytes(hashlib.sha256(PREFIX + component.encode()).digest()[:8], "big") < 2**62 else "dev"


def main():
    old = ROOT / ".preprotocol-local"
    specs = json.loads((old / "questions/specifications.json").read_text())
    specs += json.loads((old / "questions/extension-specifications.json").read_text())
    assert len(specs) == 100
    oldrows = [json.loads(l) for sp in ("dev", "holdout") for l in (old / "questions" / (sp + ".jsonl")).read_text().splitlines()]
    oldgold = [g for sp in ("dev", "holdout") for g in json.loads((old / "gold" / (sp + ".json")).read_text())]
    byrow = {r["id"]:r for r in oldrows}
    bygold = {g["family_id"]:g for g in oldgold}
    reserved = {s["id"]:"v19.requests.f%04d" % (i + 1) for i,s in enumerate(specs)}
    # Preserve pre-existing related-task clusters conservatively. Global reviewer
    # must adjudicate whether a shared helper really makes tasks equivalent.
    clusters = {}
    for s in specs:
        clusters.setdefault(s["cluster"], []).append(reserved[s["id"]])
    canonical = {f:min(members) for members in clusters.values() for f in members}
    aliases, native, compat, gold, devspec = [], [], [], [], []
    hnative, hgold, hspec = [], [], []
    for s in specs:
        original = s["id"]
        family = reserved[original]
        component = canonical[family]
        final = split(component)
        q = copy.deepcopy(byrow[original])
        q["id"] = family + ".en01"
        q["query_family"] = family
        q["split"] = final
        q["category"] = {"exact_api":"symbol_location", "behavior":"semantic_feature",
                         "architecture_facets":"architecture_understanding", "crossfile":"call_chain",
                         "config_error":"error_handling", "hardnegative":"semantic_feature"}[s["category"]]
        if original == "requests.config.ca-env-precedence":
            q["category"] = "configuration_lookup"
        for i,g in enumerate(q["answers"]):
            g["primary"] = i == 0
            g["grade"] = 3 if i == 0 else 2
        v19 = dict(protocol_version=1, repo_id="requests", source_sha=SHA,
            global_family=component, intent=s["question"], query_language="en",
            hard_scope=dict(path_prefix=q["path_prefix"]),
            facets=[dict(id=g["id"],group_id=g["id"],required=True) for g in q["answers"]],
            graph_constraints=[], mutation_profile="none", review_status="pending",
            author_id="requests-independent-author", task_category=s["category"],
            component_status="author_proposal_pending_global_review")
        g = copy.deepcopy(bygold[original])
        g["family_id"], g["split"] = family, final
        g["global_family"] = component
        for edge in g["edges"]:
            v19["graph_constraints"].append(dict(direction="caller_to_callee",
                caller=edge["caller"],callee=edge["callee"],kind=edge["kind"],
                evidence=dict(path=edge["path"],span=dict(start=edge["start_byte"],end=edge["end_byte"]))))
        if q["no_answer"]:
            v19["absence_scope"] = s["scope"]
        q["annotations"] = dict(v19=v19)
        changed = copy.deepcopy(s)
        changed["id"] = family
        changed["global_family"] = component
        changed["protocol_split"] = final
        aliases.append(dict(original_family_sha256=digest(original.encode()),
            new_family=family, global_family=component, draft_ordinal=int(family[-4:]),
            old_split=byrow[original]["split"], expected_protocol_split=final,
            old_query_row_sha256=digest(json.dumps(byrow[original],ensure_ascii=False).encode()),
            old_gold_record_sha256=digest(json.dumps(bygold[original],ensure_ascii=False,sort_keys=True).encode()),
            split_changed=byrow[original]["split"] != final,
            provenance="original draft order; reserved once, no ranking or split-dependent ID search",
            custody_status="contaminated_shared_workspace_holdout_custody_blocked" if final == "holdout" else "dev_pending_review",
            prior_public_body=specs.index(s) < 20))
        if final == "dev":
            native.append(q);gold.append(g);devspec.append(changed)
            if not q["no_answer"]:
                c = copy.deepcopy(q)
                c["expected_files"] = list(dict.fromkeys(a["path"] for group in q["answers"] for a in group["alternatives"]))
                c["answers"] = []
                c["annotations"]["v19"]["facets"] = []
                c["annotations"]["v19"]["graph_constraints"] = []
                compat.append(c)
        else:
            hnative.append(q);hgold.append(g);hspec.append(changed)
    native_bytes = lambda rows: "".join(json.dumps(r,ensure_ascii=False)+"\n" for r in rows).encode()
    (ROOT / "queries.native.dev.jsonl").write_bytes(native_bytes(native))
    (ROOT / "queries.compat.dev.jsonl").write_bytes(native_bytes(compat))
    write(ROOT / "gold/dev.json", gold)
    write(ROOT / "questions/specifications.json", devspec)
    write(ROOT / "review/id-migration.json", dict(protocol_commit="03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6",
        ranking_seen=False, old_family_count=100, aliases=aliases,
        original_material_files_sha256={str(p.relative_to(old)):digest(p.read_bytes()) for p in old.rglob("*") if p.is_file()},
        prior_public_commit="cb6c7ae3fee951940a2e679bd06f048c4c0ba7f1",
        reason="New protocol before ranking; threshold split replaces author quota; publicly exposed and shared-workspace expected holdouts blocked, deletion does not restore secrecy."))
    write(ROOT / "relations.json", dict(status="proposed_pending_global_reviewer_adjudication",
        components=[dict(global_family=min(m),members=sorted(m)) for m in clusters.values()]))
    holdout = dict(status="holdout_custody_blocked", proposed_families=len(hnative),
        separately_held_families=0, independently_reviewed=0,
        contaminated_shared_workspace=len(hnative), prior_public_body_families=sum(a["prior_public_body"] and a["expected_protocol_split"] == "holdout" for a in aliases),
        prospective_native_file_sha256=digest(native_bytes(hnative)),
        prospective_gold_file_sha256=digest((json.dumps(hgold,ensure_ascii=False,indent=2)+"\n").encode()),
        prospective_specifications_sha256=digest((json.dumps(hspec,ensure_ascii=False,indent=2)+"\n").encode()),
        bodies_written=False, access_boundary_exists=False,
        commitment_kind="computed in memory from migrated prior drafts; NOT a restricted stored-file custody proof",
        required_action="Independent custodian access/provenance adjudication; contaminated families cannot certify confirmatory holdout. Preserve history, never relabel as untouched.")
    write(ROOT / "review/holdout-commitments.json", holdout)
    inventory = json.loads((ROOT / "provenance/inventory.json").read_text())
    files = sorted(r["path"] for r in inventory["files"] if r["included"] and r["path"] not in ("LICENSE","NOTICE"))
    for profile in ("native", "compat"):
        write(ROOT / ("suite-" + profile + "-dev.json"), dict(schema_version=1,
            name="V19 Requests protocol-v1 candidate dev " + profile,
            source=dict(root="source",commit=None,digest="AUTHORING_PENDING",files=files),
            queries="queries."+profile+".dev.jsonl", queries_digest="AUTHORING_PENDING",
            scoring="codecortex-native-v1" if profile == "native" else "oce-compat-v1",
            repetitions=3,warmup=0,seed=20261003,timeout_ms=30000,top_k=10,
            engine_config=dict(auto_index=dict(enabled=False))))
    category_counts = Counter(s["category"] for s in specs)
    write(ROOT / "corpus-receipt.json", dict(protocol_version=1,
        protocol_commit="03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6",
        protocol_author_start_sha256=digest((ROOT/"provenance/protocol/AUTHOR-START.md").read_bytes()),
        repo_id="requests",source_sha=SHA,author_id="requests-independent-author",reviewer_id=None,
        candidate_families=100, accepted_independent_families=0,
        candidate_global_components=len(clusters), candidate_dev_families=len(native),
        candidate_holdout_families=len(hnative), separately_held_holdout=0,
        holdout_custody_status="holdout_custody_blocked", formal_scored_holdout=0,
        native_dev_rows=len(native),compat_dev_rows=len(compat),native_dev_no_answer=sum(q["no_answer"] for q in native),
        no_answer_families=sum(r["no_answer"] for r in oldrows),
        categories=dict(category_counts),
        family_set_sha256=digest("\n".join(reserved.values()).encode()),
        component_split_sha256=digest(json.dumps({k:split(k) for k in sorted(set(canonical.values()))},sort_keys=True).encode()),
        review_status="pending_independent_source_and_global_equivalence_review",
        ranking_seen=False,global_split_frozen=False,
        old_semantics_preserved=True,
        public_holdout_bodies="first20 preprotocol Git history exposure preserved; new public projections contain dev only",
        holdout=holdout))
    print(json.dumps({k:v for k,v in json.loads((ROOT/'corpus-receipt.json').read_text()).items() if k in ('candidate_families','candidate_global_components','candidate_dev_families','candidate_holdout_families','native_dev_rows','compat_dev_rows','holdout_custody_status','family_set_sha256','component_split_sha256')}))


if __name__ == "__main__":
    main()
