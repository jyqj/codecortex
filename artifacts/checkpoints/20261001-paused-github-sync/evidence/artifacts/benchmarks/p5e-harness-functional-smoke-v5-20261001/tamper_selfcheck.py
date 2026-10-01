import pathlib,json,shutil,subprocess
r=pathlib.Path('artifacts/benchmarks/p5e-harness-functional-smoke-v5-20261001');h=pathlib.Path('artifacts/benchmarks/p5e-harness-20261001/final-v5');driver=h/'binaries/p5e-evidence-driver';plan=r/'fanout-3-functional-plan.json';results=[]
for case in ['missing_graph_receipt','disabled_graph','zero_graph_work','partial_graph','bad_freshness','budget_exhausted','omitted_file','missing_seed_body','wrong_CALLS_endpoint','wrong_source_callsite_proof']:
 out=r/('tamper-'+case);assert not out.exists();shutil.copytree(r/'candidate-fanout3',out)
 f=out/('observations.json' if case.startswith('wrong_') else 'whole-query-observations.json');rows=json.load(open(f))
 if case=='wrong_CALLS_endpoint':rows[0]['raw']['results'][0]['target_name']='foreignsame_name'
 elif case=='wrong_source_callsite_proof':rows[0]['check']['source_witness']['proofs'][0]['actual_call_direction']=False
 else:
  raw=rows[0]['raw'];receipts=raw['evidence_summary']['retrieval']['lane_receipts'];graph=next(x for x in receipts if x['lane_id']=='graph');fresh=raw['evidence_summary']['source_freshness']
  if case=='missing_graph_receipt':receipts.remove(graph)
  elif case=='disabled_graph':graph['weight']=0
  elif case=='zero_graph_work':graph['candidate_count']=0
  elif case=='partial_graph':graph['status']='partial';graph['coverage']['complete']=False;graph['truncation_reason']='graph_source_unmapped'
  elif case=='bad_freshness':fresh['partial']=True
  elif case=='budget_exhausted':fresh['budget_exhausted']=True
  elif case=='omitted_file':fresh['omitted_files']={'group00.rs':'unknown'}
  elif case=='missing_seed_body':
   for hit in raw['machine_pack']['hits']:hit['text']=''
 f.write_text(json.dumps(rows,indent=2)+'\n');replay=r/(case+'-replay.json');log=r/(case+'-replay.log')
 with log.open('w') as stream:rc=subprocess.call([str(driver),'replay-graph',str(plan),str(out),str(replay)],stdout=stream,stderr=subprocess.STDOUT)
 assert rc==2,(case,rc);results.append({'case':case,'exit_code':rc,'result':json.load(open(replay))})
(r/'NEGATIVE-SELFChecks.json').write_text(json.dumps({'status':'10_actual_v5_replay_negative_mutations_rejected','cases':results,'scope':'functional3wholequery;not formal300/G5'},indent=2)+'\n');print('10 actual replay negative cases rejected')
