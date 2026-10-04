"""Safe diagnostic projection of fixed raw; mirrors predicate only, checks stored top1.
Unmodified Rust scorer separately verifies all scores. Never emits query/source text.
"""
import collections,hashlib,json,subprocess,sys,importlib.util
from pathlib import Path
ROOT=Path('/workspace/codecortex'); OUT=ROOT/'artifacts/diagnostics/gin-native-regression-20261004'; E=ROOT/'artifacts/checkpoints/public-dev-paired-gin-20261004'
def rows(p): return list(map(json.loads,p.read_text().splitlines()))
def save(n,x): (OUT/n).write_text(json.dumps(x,indent=2,sort_keys=True)+'\n')
def sha(x):return hashlib.sha256(x).hexdigest()
def git(*a):return subprocess.check_output(['git',*a],cwd=ROOT)
def predicate(h,a):
 s=a.get('symbol');g=a.get('span');v=h.get('span')
 return dict(path=h['path']==a['path'],evidence=h['evidence_valid'] is not False,name=s is None or h['symbol_name']==s['name'],qname=s is None or s.get('qname') is None or h['qname']==s['qname'],kind=s is None or s.get('kind') is None or h['kind']==s['kind'],span=g is None or v is not None and max(v['start'],g['start'])<min(v['end'],g['end']))
def hit(h):return {k:h[k] for k in ['path','symbol_name','qname','kind','span','evidence_valid']}|{'boundary':h['source_evidence']['boundary'],'owner':h['source_evidence']['owner']}
runroot=Path(sys.argv[1]);bind={};arms={};manifest=json.loads((E/'raw-artifact-manifest.json').read_text());assert sha((E/'public-development-paired-raw.tar.gz').read_bytes())==manifest['archive_sha256']
for path,record in manifest['files'].items(): assert sha((runroot/path).read_bytes())==record['sha256']
for arm in ['baseline','candidate']:
 plan=json.loads((E/arm/'plan.json').read_text());build=json.loads((E/arm/'build/build-receipt.json').read_text());source=plan['source_sha'];src={n:d for n,d in build['source_file_sha256'].items() if '/src/' in n or n in ['Cargo.toml','Cargo.lock']}
 assert all(sha(git('show',source+':'+n))==d for n,d in src.items())
 gin_sources={n:d for n,d in plan['actual_source_sha256'].items() if '/public-v19/gin/source/' in n};assert len(gin_sources)==53;assert all(sha(git('show',n))==d for n,d in gin_sources.items())
 bind[arm]={'gin_actual_source_files_rehashed':53,'source_sha':source,'source_receipt_files_rehashed_against_git':len(src),'binary_pins':{n:a['sha256'] for n,a in build['artifacts'].items()},'input_lock':plan['input_lock'],'git_src_verified':True}
 arms[arm]={}
 for profile in ['native','compat']:
  p=runroot/arm/'runs'/profile;qs=rows(p/'queries.jsonl');rs=rows(p/'normalized.jsonl');ss=rows(p/'scores.jsonl');assert len(rs)==len(qs)*3==len(ss)
  lock=plan['input_lock']; original=git('show',lock[profile+'_entry'])
  if profile=='native':
   receipt=json.loads((ROOT/'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003/admission-receipt.json').read_text())
   change=json.loads(git('show',receipt['change_manifest_entry']))
   spec=importlib.util.spec_from_file_location('selector',ROOT/'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003/selector.py');loader=importlib.util.module_from_spec(spec);spec.loader.exec_module(loader)
   original=loader.apply_native(original,[c for c in change['changes'] if c['binding']['repo']=='gin'],lock['native_after_sha256'])
  else: assert sha(original)==lock['compat_sha256']
  assert list(map(json.loads,original.splitlines()))==qs
  assert all(r['status']=='partial' for r in rs);assert len({(r['case_id'],r['repetition']) for r in rs})==len(rs)
  arms[arm][profile]={'queries':{q['id']:q for q in qs},'rows':{(r['case_id'],r['repetition']):(r,s) for r,s in zip(rs,ss)},'path':p}
sourcepaths=[f'crates/{n}/src' for n in ['cc-model','cc-db','cc-parsers','cc-index','cc-search','cc-server','cc-eval','cc-semantic']]+['Cargo.toml','Cargo.lock']
assert not git('diff','90858afae647a513537bf118932a7ba5020ee98b','37dd042eaa1209a86e0cafdcd92ae77e036e76f5','--',*sourcepaths)
assert not git('diff','88f2cf0','37dd042','--','crates/cc-eval/src')
assert bind['baseline']['input_lock']==bind['candidate']['input_lock']
save('binding.json',{'archive_sha256':manifest['archive_sha256'],'candidate_product_eight_src_equal':True,'evaluator_src_equal':True,'arms':bind,'same_input':True,'binary_rehash_limitation':'archived binaries not included; recorded pins verified through build compiler receipts, no claim to rehash unavailable original executables'})
cases=[];counts=collections.Counter();failure=collections.Counter();raworder=collections.Counter();outrows=[]
for qid,q in arms['baseline']['native']['queries'].items():
 per=[]
 for rep in range(3):
  pair={}
  for arm in ['baseline','candidate']:
   info=arms[arm]['native'];r,s=info['rows'][qid,rep];rawbytes=(info['path']/r['raw_path']).read_bytes();assert sha(rawbytes)==manifest['files'][arm+'/runs/native/'+r['raw_path']]['sha256'];raw=json.loads(rawbytes);rh=raw['machine_pack']['hits'];assert len(rh)==len(r['hits'])
   predictions=[]
   for rank,(h,rawhit) in enumerate(zip(r['hits'],rh),1):
    assert h['path']==rawhit['file_path'];meta=rawhit.get('metadata',{});assert h['symbol_name']==rawhit.get('symbol_name',meta.get('symbol_name'));assert h['qname']==rawhit.get('qname',meta.get('qname'));assert h['kind']==rawhit.get('kind',rawhit.get('symbol_kind',meta.get('symbol_kind')))
    tests=[{'group':g['id'],'primary':g['primary'],'alternative':i,'predicates':predicate(h,a)} for g in q['answers'] for i,a in enumerate(g['alternatives'])]
    primary=any(t['primary'] and all(t['predicates'].values()) for t in tests)
    predictions.append(primary)
    outrows.append({'arm':arm,'case_id':qid,'repetition':rep,'rank':rank,'raw_ref':r['raw_path'],'raw_sha256':sha(rawbytes),'raw_original_digest':r['raw_digest'],'normalized':hit(h),'raw_projection':{k:rawhit.get(k) for k in ['chunk_id','symbol_name','symbol_kind','breadcrumb','rerank_score','score_trace']},'raw_qname':meta.get('qname'),'match_tests':tests,'primary_match':primary})
   if not q['no_answer']:assert s['top1']==float(bool(predictions and predictions[0]))
   pair[arm]={'scores':s,'top1':s['top1'],'mrr10':s['mrr10'],'first_primary_rank':next((i+1 for i,v in enumerate(predictions) if v),None),'hits':[hit(h)|{'chunk_id':rh[i]['chunk_id'],'rerank_score':rh[i]['rerank_score'],'score_trace':rh[i].get('score_trace')} for i,h in enumerate(r['hits'])],'packing':raw['evidence_summary']['packing'],'selection':raw['evidence_summary']['selection'],'raw_ref':r['raw_path'],'raw_sha256':sha(rawbytes),'raw_original_digest':r['raw_digest']}
  per.append(pair)
 noanswer=q['no_answer'];a=per[0]['baseline'];b=per[0]['candidate'];change='no_answer' if noanswer else 'up' if b['top1']>a['top1'] else 'down' if b['top1']<a['top1'] else 'unchanged';counts[change]+=1
 assert all(p['baseline']['top1']==a['top1'] and p['candidate']['top1']==b['top1'] for p in per)
 reason='unchanged_top1'
 if change=='down':
  assert a['first_primary_rank']==1 and b['first_primary_rank'] in [2,3]
  old=a['hits'][0];target=b['hits'][b['first_primary_rank']-1];assert old['span']==target['span'] and old['symbol_name']==target['symbol_name'] and old['kind']==target['kind']
  winner=b['hits'][0];prev=next((h for h in a['hits'] if h['chunk_id']==winner['chunk_id']),None)
  reason='new_declaration_name_displaces_retained_primary_by_rerank';raworder['score_change_before_packing']+=1
  tests=[predicate(winner,x) for g in q['answers'] if g['primary'] for x in g['alternatives']];failure.update(k for k,v in tests[0].items() if not v)
 elif change=='up':reason='new_correct_declaration_identity' if qid=='v19.gin.f0029.en01' else 'lexical_rank_swap_of_retained_chunks'
 elif noanswer:reason='strict_no_answer_failed_all_partial'
 cases.append({'case_id':qid,'change':change,'reason':reason,'repetitions':per,'qname_constraints':sum(a.get('symbol',{}).get('qname') is not None for g in q['answers'] for a in g['alternatives'])})
(OUT/'predicates.jsonl').write_text(''.join(json.dumps(r,sort_keys=True)+'\n' for r in outrows));save('cases.json',cases)
save('aggregate.json',{'answerable':55,'queries':67,'counts':dict(counts),'baseline_correct':27,'candidate_correct':21,'net_correct_delta':-6,'top1_delta':-6/55,'down_candidate_first_primary_ranks':dict(collections.Counter(str(c['repetitions'][0]['candidate']['first_primary_rank']) for c in cases if c['change']=='down')),'down_top1_predicate_failures_against_primary':dict(failure),'down_phase':dict(raworder),'qname_constrained_alternatives':sum(c['qname_constraints'] for c in cases),'repeated_top1_stable':True,'source_body_emitted':False,'ci':'not implemented; descriptive, not statistically significant','no_answer':'12 cases x3 each per arm fail unchanged; all Partial','quality':'FAIL unchanged'})
print(json.dumps({'counts':dict(counts),'down_failures':dict(failure),'raw_score_phase':dict(raworder)}))
