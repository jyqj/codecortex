#!/usr/bin/env python3
"""Mandatory 9 actual stdio witnesses+replay BEFORE measurements, not a score proxy.
All cells use fresh independent targets; full111 must be product policy equivalent.
"""
import argparse,hashlib,json,pathlib,subprocess,os

def stable_projection(row):
    raw=row['raw'];ret=raw['evidence_summary']['retrieval'];selection=raw['evidence_summary']['selection'];lanes=ret.get('lane_receipts',ret.get('lanes'));hits=raw['machine_pack']['hits']
    return {'policy':ret['policy'],'selection':{k:selection[k] for k in ['spec','intent','facets','intent_facet_anchors','source_support_anchors','original_ranks','source_support_scope']},'lanes':[{k:l[k] for k in ['lane_id','weight','status','candidate_count','coverage','truncation_reason']} for l in lanes],'hits':[{k:h.get(k) for k in ['chunk_id','file_path','text','start_line','end_line','score_trace','rerank_score'] }|{'evidence_priority':h.get('metadata',{}).get('evidence_priority'),'source_evidence':h.get('metadata',{}).get('source_evidence')} for h in hits]}

def main():
    p=argparse.ArgumentParser();p.add_argument('--prepared',type=pathlib.Path,required=True);p.add_argument('--driver',type=pathlib.Path,required=True);p.add_argument('--candidate',type=pathlib.Path,required=True);p.add_argument('--witness-plan',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args();assert not a.output.exists();a.output.mkdir();plan=json.load(open(a.prepared/'plan.json'));targets=set();registered=[]
    for cell in plan['variants']:
        receipt=json.load(open(a.prepared/cell['build_receipt']));target=pathlib.Path(receipt['build_options']['CARGO_TARGET_DIR']).resolve();assert target not in targets,'sharedtarget invalidates source->binary binding';targets.add(target)
        assert receipt['exit_code']==0 and hashlib.sha256((a.prepared/cell['binary']).read_bytes()).hexdigest()==receipt['binary_sha256'];registered.append((cell['id'],a.prepared/cell['binary'],cell['enabled']))
    witness=json.load(open(a.witness_plan));assert len(witness['queries'])==3 and {q['factor'] for q in witness['queries']}=={'path','exact','intent_aware_facet_reservation'} and {q['id'] for q in witness['queries']}=={'path_behavior','exact_behavior','intent_behavior'}
    registered.append(('product_candidate',a.candidate,[c['id'] for c in plan['controls']]));receipts=[]
    for name,binary,enabled in registered:
        out=a.output/name;log=a.output/(name+'.log');argv=[str(a.driver),'control-witness',str(binary),str(a.witness_plan),json.dumps(enabled),str(out)]
        with log.open('w') as stream:rc=subprocess.call(argv,stdout=stream,stderr=subprocess.STDOUT)
        receipts.append({'name':name,'argv':argv,'exit_code':rc,'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest()})
        if rc!=0:(a.output/'BLOCKED.json').write_text(json.dumps({'status':'actual_registered_control_witness_failed','receipts':receipts},indent=2)+'\n');raise SystemExit(2)
        replay=a.output/(name+'-replay.json');replayrc=subprocess.call([str(a.driver),'replay-controls',str(a.witness_plan),str(out),str(replay)])
        if replayrc!=0:raise SystemExit(2)
    full=json.load(open(a.output/'cell_111/observations.json'));product=json.load(open(a.output/'product_candidate/observations.json'));assert [stable_projection(r) for r in full]==[stable_projection(r) for r in product],'full111 actual policy/body/proof/rank/roles differs from candidate'
    (a.output/'CONTROL-BINDING-RECEIPT.json').write_text(json.dumps({'status':'all8actual_threefactor_controls_and_full111product_equivalence_verified','fresh_independent_targets':[str(t) for t in sorted(targets)],'controls_per_cell':3,'actual_witness_receipts':receipts,'full111_candidate_stable_policy_body_source_score_equal':True,'scope':'premeasurement required binarybinding mechanism,not quality/performance or fullsourceattestation'},indent=2)+'\n')
if __name__=='__main__':main()
