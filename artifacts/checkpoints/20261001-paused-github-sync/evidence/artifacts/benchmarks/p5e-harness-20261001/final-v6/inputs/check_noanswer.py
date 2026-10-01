#!/usr/bin/env python3
"""Locked original query.no_answer obligations, separate from ranking N/A.
Reports observed executed-domain absence plus all notexecuted lanes explicitly.
Never certifies a cell's binary/required policy binding (control witnesses do).
"""
import argparse,json,pathlib,hashlib

def absence(row,raw):
    issues=[];e=raw.get('evidence_summary') or {};ret=e.get('retrieval') or {};lanes=ret.get('lane_receipts',ret.get('lanes'));fresh=e.get('source_freshness') or {};pack=e.get('packing') or {}
    if row.get('status')!='no_match' or row.get('hits')!=[]:issues.append('not_actual_NoMatch_or_nonzero_normalized_bodies')
    machine=(raw.get('machine_pack') or {}).get('hits')
    if machine!=[]:issues.append('raw_zero_body_evidence_missing_or_nonzero')
    if not(isinstance(lanes,list) and len(lanes)==5 and {l.get('lane_id') for l in lanes}=={'path','exact_symbol','lexical','grep','graph'}):issues.append('required_local5_receipts_missing_duplicate_or_unknown');lanes=lanes if isinstance(lanes,list) else []
    executed=[];notexecuted=[]
    for lane in lanes:
        if lane.get('status')=='disabled':notexecuted.append(lane);continue
        coverage=lane.get('coverage') or {};executed.append(lane)
        if not(lane.get('status')=='complete' and coverage.get('complete') is True and lane.get('candidate_count')==0 and coverage.get('total_lower_bound')==0 and lane.get('truncation_reason') is None):issues.append('executed_absence_incomplete:'+str(lane.get('lane_id')))
    if not executed:issues.append('no_executed_absence_domain')
    if not(fresh.get('partial') is False and fresh.get('budget_exhausted') is False and fresh.get('omitted_files')=={}):issues.append('source_freshness_absence_incomplete')
    if not(pack.get('partial') is False and pack.get('omitted_hits')==0 and pack.get('omitted_nodes')==0):issues.append('packing_cannot_hide_answers')
    if raw.get('invalidations')!=[]:issues.append('invalidations_missing_or_present')
    return {'observed_executed_domain_absence_complete':not issues,'issues':issues,'executed_lanes':executed,'not_executed_lanes':notexecuted,'hard_gate_requires':'independent registered requiredlane/policy/binary binding witnesses;disabled is recorded NOT proof that lane executed'}

def main():
    p=argparse.ArgumentParser();p.add_argument('--ablation-root',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args();assert not a.output.exists();plan=json.load(open(a.ablation_root/'plan.json'));checks=[];issues=[];populations={}
    for dataset in plan['datasets']:
        suite_path=pathlib.Path(dataset['suite']);suite=json.load(open(suite_path));query_path=(suite_path.parent/suite['queries']).resolve();rawqueries=query_path.read_bytes();queries=[json.loads(x) for x in rawqueries.decode().splitlines() if x.strip()];gold=[q for q in queries if q.get('no_answer') is True];populations[dataset['id']]={'queries_sha256':hashlib.sha256(rawqueries).hexdigest(),'no_answer_population':len(gold)}
        for variant in plan['variants']:
            folder=a.ablation_root/f"{dataset['id']}--{variant['id']}";rows=[json.loads(x) for x in (folder/'normalized.jsonl').read_text().splitlines() if x.strip()]
            for query in gold:
                observed=[r for r in rows if r['case_id']==query['id']];expected=list(range(suite['repetitions']))
                if sorted(r['repetition'] for r in observed)!=expected:issues.append({'dataset':dataset['id'],'variant':variant['id'],'id':query['id'],'field':'no_answer_unique_complete_repetitions'})
                for row in observed:
                    raw=json.load(open(folder/row['raw_path']));result=absence(row,raw);checks.append({'dataset':dataset['id'],'variant':variant['id'],'case_id':query['id'],'repetition':row['repetition'],'raw_path':str(folder/row['raw_path']),'raw_sha256':hashlib.sha256((folder/row['raw_path']).read_bytes()).hexdigest(),'absence':result})
                    if result['issues']:issues.append({'dataset':dataset['id'],'variant':variant['id'],'id':query['id'],'repetition':row['repetition'],'actual':result['issues']})
    a.output.write_text(json.dumps({'status':'invalid_absence_evidence' if issues else 'observed_executed_absence_consistent_pending_binary_requiredpolicy_binding','issues':issues,'locked_query_populations':populations,'checks':checks,'limits':['truth is locked original query.no_answer;never ID/null/rankzero','NoMatch+emptybodies alone insufficient;all executedcoverage/source/budget receipts required','notexecuted disabled domains explicit;full-on candidatecontrolwitness and independentrequiredpolicy audit mandatory','native rawdigest/scorer replays separately required;this checks source/absence structure','oldformalv1 stale binaries never certified by this report']},indent=2)+'\n')
if __name__=='__main__':main()
