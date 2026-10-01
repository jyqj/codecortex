import pathlib,json,copy,shutil,subprocess
r=pathlib.Path('artifacts/benchmarks/p5e-v6-control-functional-20261001');driver=pathlib.Path('artifacts/benchmarks/p5e-harness-20261001/final-v6/binaries/p5e-evidence-driver');plan=r/'CONTROL-PLAN-v2.json';receipts=[]
for case in ['exact_disabled_when_on','path_disabled_when_on','selector_locate_when_fix','missing_source_support','freshness_partial','packing_body_omitted','scope_outside','missing_graph_receipt','missing_required_body','duplicate_query','fake_off_registration','forced_termination']:
 out=r/('negative-'+case);assert not out.exists();shutil.copytree(r/'product-allon-v2',out);p=out/'observations.json';rows=json.load(open(p));index=1 if case=='exact_disabled_when_on' else 2 if case in ['selector_locate_when_fix','missing_source_support'] else 0;raw=rows[index]['raw'];ret=raw['evidence_summary']['retrieval'];lanes=ret.get('lane_receipts',ret.get('lanes'))
 if case in ['exact_disabled_when_on','path_disabled_when_on']:next(l for l in lanes if l['lane_id']==('exact_symbol' if case.startswith('exact') else 'path')).update(status='disabled',candidate_count=0)
 elif case=='selector_locate_when_fix':raw['evidence_summary']['selection']['intent']='locate'
 elif case=='missing_source_support':raw['evidence_summary']['selection']['source_support_anchors']=[]
 elif case=='freshness_partial':raw['evidence_summary']['source_freshness']['partial']=True
 elif case=='packing_body_omitted':raw['evidence_summary']['packing']['omitted_hits']=1
 elif case=='scope_outside':ret['scope']['hard']['path_prefix']='outside'
 elif case=='missing_graph_receipt':lanes[:]=[l for l in lanes if l['lane_id']!='graph']
 elif case=='missing_required_body':raw['machine_pack']['hits'][0]['text']=''
 elif case=='duplicate_query':rows[2]=copy.deepcopy(rows[0])
 elif case=='fake_off_registration':
  file=out/'input-lock.json';v=json.load(open(file));v['registered_enabled']=[];file.write_text(json.dumps(v)+'\n')
 else:
  file=out/'termination.json';v=json.load(open(file));v['forced']=True;file.write_text(json.dumps(v)+'\n')
 p.write_text(json.dumps(rows,indent=2)+'\n');result=r/(case+'-replay.json');log=r/(case+'-replay.log')
 with log.open('w') as stream:rc=subprocess.call([str(driver),'replay-controls',str(plan),str(out),str(result)],stdout=stream,stderr=subprocess.STDOUT)
 assert rc==2,(case,rc);receipts.append({'case':case,'exit_code':rc,'errors':json.load(open(result))['errors']})
(r/'NEGATIVE-RECEIPT.json').write_text(json.dumps({'status':'all12 actual compiledcontrolreplay_mutations_rejected','checks':receipts,'scope':'sameactualcandidate publicraw+normalownedchild;notformal8cellbinding itself'},indent=2)+'\n');print('12 actual control replay negatives rejected')
