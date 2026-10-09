import datetime,hashlib,json,re
from pathlib import Path
P=Path('/dev/shm/a217aaae3bde/platform-review/round19-main-M-push')
HEAD='b21cce4c8661589267ad5719f850accbec088d2f'
def read(p):return json.loads(p.read_bytes())
def identity(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
run_ids=[37883123545,37883123606,37883123576,37883123562,37883123577];runs=[];alljobs=[]
for rid in run_ids:
 r=read(P/f'run-{rid}.json');one=read(P/f'run-{rid}-jobs-page1.json');two=read(P/f'run-{rid}-jobs-page2.json')
 assert r['head_sha']==HEAD and r['event']=='push' and r['status']=='completed' and r['conclusion']=='success'
 assert len(one['jobs'])==one['total_count'] and two['jobs']==[]
 assert all(j['head_sha']==HEAD and j['status']=='completed' and j['conclusion']=='success' for j in one['jobs'])
 runs.append({'run_id':rid,'name':r['name'],'official_head':r['head_sha'],'event':r['event'],'conclusion':r['conclusion'],'job_count':len(one['jobs']),'run_capture':identity(P/f'run-{rid}.json'),'jobs_page1':identity(P/f'run-{rid}-jobs-page1.json'),'jobs_page2':identity(P/f'run-{rid}-jobs-page2.json')});alljobs.extend(one['jobs'])
assert len(alljobs)==19
logs={};jsonlines={}
for jid in [113667086735,113667086058,113667086499]:
 p=P/f'job-{jid}.log';lines=p.read_text().splitlines();job=next(x for x in alljobs if x['id']==jid)
 checkout=[{'line':i,'original_line':s} for i,s in enumerate(lines,1) if s.endswith('Z '+HEAD)]
 assert checkout and job['run_id']==37883123545
 vals=[]
 for i,s in enumerate(lines,1):
  text=re.sub(r'^\ufeff?\d{4}-\d\d-\d\dT\S+Z ','',s)
  if text.startswith('{'):
   try:vals.append({'line':i,'value':json.loads(text)})
   except json.JSONDecodeError:pass
 jsonlines[jid]=vals
 logs[jid]={'job_id':jid,'job_name':job['name'],'official_job_conclusion':job['conclusion'],'original_log':identity(p),'actual_checkout':HEAD,'checkout_log_rows':checkout,'all_official_steps':job['steps'],'original_log_line_count':len(lines)}
check=(P/'job-113667086735.log').read_text().splitlines()
guards=[x for x in jsonlines[113667086735] if x['value'].get('source_version')=='p8-completion-source-20261009-v15']
assert len(guards)==1;g=guards[0]['value'];assert g['status']=='passed' and g['complete_inputs']==1087 and g['product_source']=='254009277688d64677361a0ca33e5dea73f295ef' and g['review_source']=='3a11f30f9f00a89fe3cd481b3b7609066728baad' and g['runtime_and_quality_claims']=='require_separate_execution_evidence'
task=[x for x in jsonlines[113667086735] if x['value'].get('task_count')==192 and 'states' in x['value']]
assert len(task)>=1
for x in task:assert x['value']['status']=='passed' and x['value']['states']=={'done':163,'blocked':1,'in_progress':16,'todo':12}
hist=[x for x in jsonlines[113667086735] if x['value'].get('verifier_version')=='historical-integrations-v2'];assert len(hist)==1 and hist[0]['value']['status']=='passed' and hist[0]['value']['quality_and_100k']=='not_inherited'
groups=[]
for i,s in enumerate(check,1):
 x=re.search(r'test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out',s)
 if x:
  state,passed,failed,ignored,measured,filtered=x.groups();assert state=='ok' and int(failed)==0
  groups.append({'line':i,'state':state,'passed':int(passed),'failed':int(failed),'ignored':int(ignored),'measured':int(measured),'filtered':int(filtered),'original_line':s})
assert groups
ignored=[{'line':i,'original_line':s} for i,s in enumerate(check,1) if re.search(r'\btest .* \.\.\. ignored(?:,|$)',s)]
originalE=read(P.parent/'pr167-execution/job-113631480871-independent-review.json')
names=[x['name'] for x in originalE['all_seven_new_inline_tests_actual_ok']]
inline=[]
for n in names:
 hits=[{'line':i,'original_line':s} for i,s in enumerate(check,1) if n+' ... ok' in s];assert hits;inline.append({'name':n,'actual_ok_occurrences':hits})
http=[{'line':i,'original_line':s} for i,s in enumerate(check,1) if re.search(r'test http::[^ ]+ \.\.\. ok$',s)]
assert http
semantic=[{'line':i,'original_line':s} for i,s in enumerate(check,1) if 'VERIFIED 17 existing tests (6 DB + 10 semantic-http status + 1 stdio)' in s];assert len(semantic)==1
py=[{'line':i,'original_line':s,'following_result':[v for v in check[i:i+4] if v.endswith('Z OK') or 'OK (' in v or 'FAILED (' in v]} for i,s in enumerate(check,1) if re.search(r'Ran \d+ tests? in ',s)]
assert all(not any('FAILED (' in v for v in x['following_result']) for x in py)
msrv=(P/'job-113667086058.log').read_text().splitlines()
assert any('installed - rustc 1.95.0 ' in s for s in msrv)
assert any('Run cargo check --workspace --all-targets --locked' in s for s in msrv)
assert any('Run cargo check -p cc-eval --all-targets --features eval-http --locked' in s for s in msrv)
audits=[x for x in jsonlines[113667086499] if 'vulnerabilities' in x['value']];assert len(audits)==1
assert audits[0]['value']['vulnerabilities']=={'found':False,'count':0,'list':[]} and audits[0]['value']['warnings']=={}
proof=P.parent/'round19-actual-E-merge/independent-review.json';assert hashlib.sha256(proof.read_bytes()).hexdigest()=='74c586e09b79d958cc61d5650e6a6882c630891d351123423c85840a735dc37c'
out={'schema':'actual-main-M-push-CI-scoped-independent-review-v1','reviewer':'/root/pr_audit','reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'decision':'accepted_actual_M_main_push_CI_and_official_five_run_completion','actual_source_commit':HEAD,'actual_source_tree':'54b3227bb9f0b36b38083edee3c76817561bb44c','actual_merge_tree_proof':identity(proof),'run_inventory':runs,'all_jobs':[{k:j[k] for k in ['id','run_id','name','head_sha','status','conclusion','started_at','completed_at']} for j in alljobs],'official_job_count':19,'official_completed_success_count':19,'three_CI_original_log_reviews':list(logs.values()),'v15_original_output':guards,'task_guard_original_outputs':task,'historical_v2_original_output':hist,'rust_result_groups':groups,'rust_result_occurrence_totals':{k:sum(x[k] for x in groups) for k in ['passed','failed','ignored','measured']},'ignored_test_occurrences_preserved':ignored,'ignored_not_counted_as_passed':True,'all_seven_inline_original_ok':inline,'python_unittest_summaries':py,'http_contract_tests_actual_ok':http,'semantic_http_original_regression_output':semantic,'security_original_audit_output':audits,'MSRV_actual_command_scope':['Rust1.95.0 cargo check --workspace --all-targets --locked','Rust1.95.0 cargo check -p cc-eval --all-targets --features eval-http --locked'],'limits':['These are actual push executions at merged M, not E/75649 re-labeled logs.','Only CI three complete job logs are independently reviewed here; other four runs are official full-pagination job-state checks, not repeated native artifact acceptance.','HTTP contract and loopback regression passes do not certify live service latency or throughput.','Rust summary totals are execution occurrences across targets/commands, not a unique-test count. Ignored tests remain ignored.','v15 grants reviewed source admission only; quality/runtime/scale acceptance requires separate original evidence.','No E scale run was modified or canceled; 150-shard completion is not implied.'],'source_or_remote_mutations':False,'new_native_invocations':0,'formal_task_completion':False,'task_counts':{'done':163,'remaining':29},'unresolved_CI_log_blockers':[]}
p=P/'independent-review.json';p.write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'path':str(p),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest(),'rust':out['rust_result_occurrence_totals'],'groups':len(groups),'ignored_lines':len(ignored)}))
