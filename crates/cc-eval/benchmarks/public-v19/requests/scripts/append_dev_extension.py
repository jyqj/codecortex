"""Append the pre-reserved dev-only block; no holdout body creation or ranking."""
import copy
import json
from collections import Counter
from author import ROOT, SHA, reference, digest, write
from migrate_protocol import split


def main():
    reservation = json.loads((ROOT / "review/serial-reservation-0101-0120.json").read_text())
    allowed = {r["global_family"]:r for r in reservation["families"]}
    specs = json.loads((ROOT / "questions/dev-extension-0101-0120.json").read_text())
    assert len(specs) == 16 and len({s["id"] for s in specs}) == 16
    # Undrafted holdout serials remain allocated; they are neither replaced nor
    # counted as source-backed candidate families. Real custody is required first.
    assert {s["id"] for s in specs} == {k for k,v in allowed.items() if v["split"] == "dev"}
    main_specs = json.loads((ROOT / "questions/specifications.json").read_text())
    main_gold = json.loads((ROOT / "gold/dev.json").read_text())
    relation = json.loads((ROOT / "relations.json").read_text())
    native_path, compat_path = ROOT / "queries.native.dev.jsonl", ROOT / "queries.compat.dev.jsonl"
    old_native, old_compat = native_path.read_bytes(), compat_path.read_bytes()
    native_ids = {json.loads(line)["query_family"] for line in old_native.splitlines()}
    added_n, added_c, added_g = [], [], []
    for s in specs:
        assert split(s["global_family"]) == s["protocol_split"] == "dev"
        groups, evidence, edges = [], [], []
        for i,key in enumerate(s["refs"]):
            alt, ev = reference(key)
            evidence.append(ev)
            groups.append(dict(id="facet-"+str(i+1),primary=i==0,grade=3 if i==0 else 2,alternatives=[alt]))
        for caller,callee,expr in s.get("edges",[]):
            _,ev=reference(caller)
            raw=(ROOT/"source"/ev["path"]).read_bytes()
            start=raw.index(expr.encode(),ev["start_byte"],ev["end_byte"])
            edges.append(dict(caller=caller,callee=callee,expression=expr,source_sha=SHA,path=ev["path"],
                start_byte=start,end_byte=start+len(expr.encode()),line=raw[:start].count(b"\n")+1,kind="call"))
        ann=dict(protocol_version=1,repo_id="requests",source_sha=SHA,global_family=s["global_family"],
            intent=s["question"],query_language="en",hard_scope=dict(path_prefix=None),
            facets=[dict(id=g["id"],group_id=g["id"],required=True) for g in groups],
            graph_constraints=[dict(direction="caller_to_callee",caller=e["caller"],callee=e["callee"],kind=e["kind"],
                evidence=dict(path=e["path"],span=dict(start=e["start_byte"],end=e["end_byte"]))) for e in edges],
            mutation_profile="none",review_status="pending",author_id="requests-independent-author",
            task_category=s["category"],component_status="new_singleton_proposal_pending_global_review")
        q=dict(id=s["id"]+".en01",query_family=s["id"],category={"behavior":"semantic_feature",
            "architecture_facets":"architecture_understanding","config_error":"error_handling",
            "hardnegative":"semantic_feature","crossfile":"call_chain"}[s["category"]],difficulty=3 if s["category"] in ("crossfile","hardnegative") else 2,
            language="python",split="dev",query=s["question"],path_prefix=None,no_answer=False,expected_files=[],answers=groups,annotations=dict(v19=ann))
        g=dict(family_id=s["id"],source_sha=SHA,split="dev",status="candidate_not_independently_reviewed",
            global_family=s["global_family"],rationale=s["answer"],evidence=evidence,edges=edges,absence=None,scope=None,
            independence_proposal=s["independence"])
        if s["id"] in native_ids:
            existing=next(json.loads(line) for line in old_native.splitlines() if json.loads(line)["query_family"]==s["id"])
            assert existing==q
            continue
        added_n.append(q);added_g.append(g);main_specs.append(s)
        relation["components"].append(dict(global_family=s["id"],members=[s["id"]]))
        c=copy.deepcopy(q);c["expected_files"]=list(dict.fromkeys(a["path"] for group in groups for a in group["alternatives"]));c["answers"]=[]
        c["annotations"]["v19"]["facets"]=[];c["annotations"]["v19"]["graph_constraints"]=[];added_c.append(c)
    if not added_n:
        print(json.dumps(dict(status="dev_extension_already_materialized",new_families=16)))
        return
    encoded=lambda rows:"".join(json.dumps(r,ensure_ascii=False)+"\n" for r in rows).encode()
    native_path.write_bytes(old_native+encoded(added_n));compat_path.write_bytes(old_compat+encoded(added_c))
    write(ROOT/"gold/dev.json",main_gold+added_g);write(ROOT/"questions/specifications.json",main_specs);write(ROOT/"relations.json",relation)
    write(ROOT/"review/dev-extension-receipt.json",dict(status="candidate_pending_independent_review",new_families=len(added_n),new_global_components=len(added_n),
        accepted=0,original_native_prefix_sha256=digest(old_native),original_compat_prefix_sha256=digest(old_compat),old_gold_records_preserved=True,
        new_ids=[s["id"] for s in specs],source_sha=SHA,reservation_sha256=digest((ROOT/"review/serial-reservation-0101-0120.json").read_bytes()),
        deferred_holdout_serials=[k for k,v in allowed.items() if v["split"]=="holdout"],deferred_holdout_bodies_drafted=0,
        deferred_holdout_status="reserved_not_drafted_pending_real_custody",prior_25_holdout_status="holdout_custody_blocked_unchanged",ranking_seen=False))
    print(json.dumps(dict(status="dev_only_extension_materialized",new_families=16,undrafted_reserved_holdout_serials=4,holdout_bodies_written=0)))


if __name__=="__main__":
    main()
