from pathlib import Path
import sys,json,hashlib,collections,datetime,math
sys.dont_write_bytecode=True
artifact_id=int(sys.argv[1]);expected_zip=sys.argv[2]
root=Path.cwd();source=(root/'candidate-combined').resolve();sys.path.insert(0,str(source/'scripts'))
from p7_build_identity import source_snapshot
from p8_runtime_build import observer_snapshot,verify_output
base=root/'raw-pr178-9f7b16f'/str(artifact_id)/'extracted';read=lambda p:json.loads(p.read_text())
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for block in iter(lambda:f.read(1048576),b''):h.update(block)
 return h.hexdigest()
r=read(base/'receipt.json');errors=[];checks=collections.Counter()
def check(ok,name):
 checks[name]+=1
 if not ok and len(errors)<100:errors.append(name)
check(sha(root/'raw-pr178-9f7b16f'/str(artifact_id)/'original.zip')==expected_zip,'github_archive_digest')
seal=verify_output(base);checks['original_inventory_verifier']=1
snap=source_snapshot(source);observer=observer_snapshot(source)
check(snap==r['source_before']==r['source_after']==r['source_final']==read(base/'source-before.json')==read(base/'source-after.json'),'all_exact_source_snapshots')
check(snap['source_commit']=='9f7b16f0758eb79f306cf44605b84550f02de441','exact_G9f_source')
check(observer==r['observer_before']==r['observer_after']==r['observer_final'],'all_original_observer_snapshots')
for path,item in observer['files'].items():check(sha(base/'observer-source'/path)==item['sha256'],'retained_observer_copy')
for path,digest in r['files'].items():check(sha(base/path)==digest,'receipt_file_digest')
check(sha(base/'p7_worker_contention')==r['executable_sha256']==r['executable_sha256_after']==r['copy_source']['sha256'],'fixed_original_executable_digest')
check(r['build_exit_code']==r['execution_exit_code']==r['exit_code']==0 and r['status']=='passed_observation','actual_execution_success')
check(r['target_initially_absent'] is True and r['target_dir']==r['build_dir'] and r['toolchain_before']==r['toolchain_after']==r['toolchain_final'],'original_fresh_target_toolchain_stability')
check(r['compiler_environment']=={'RUSTC':r['toolchain_before']['rustc']['invocation'],'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''},'original_compiler_environment')
messages=[json.loads(line) for line in (base/'build.jsonl').open()];a=r['cargo_artifact']
selected=[x for x in messages if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')=='p7_worker_contention' and x.get('executable')]
check(selected==[a] and a['features']==['semantic'] and a['fresh'] is False and a['profile']['test'] is True and a['profile']['opt_level']=='3' and a['profile']['debug_assertions'] is False,'original_exact_release_semantic_cargo_artifact')
check(any(x.get('reason')=='build-finished' and x.get('success') is True for x in messages),'original_cargo_success')
check(r['execution_command'][1:]==['--exact','real_slow_backfill_preserves_local_progress_and_records_every_request','--nocapture'],'exact_original_test_selection')
stdout=(base/'execution.stdout').read_text()
check('1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out' in stdout and 'real_slow_backfill_preserves_local_progress_and_records_every_request ... ok' in stdout,'one_actual_selected_test')
check(sorted(x.name for x in (base/'raw').iterdir())==['seed-19','seed-43','seed-7'],'three_original_seeds')
cells=[];total=0;resources=[];seed_summaries=[]
for seed in [7,19,43]:
 d=base/'raw'/('seed-'+str(seed));s=read(d/'summary.json');proto=read(d/'protocol.json');held=read(d/'held-before.json')
 check(sorted(p.name for p in d.iterdir())==['held-before.json','held-requests.json','protocol.json','quiet-requests.json','summary.json'],'no_missing_or_failure_seed_file')
 check(proto['seed']==seed and proto['concurrency_cells']==[1,4,8,16] and proto['samples_per_cell']==32 and proto['phases']==['quiet','held'] and proto['query_watchdog_ms']==2000 and proto['progress_watchdog_ms']==5000 and proto['writer_busy_timeout_ms']==100 and proto['worker_local_attempt_width']==4 and proto['worker_claim_round_cap']==16,'original_protocol_unchanged')
 check(held['provider']['active']==held['provider']['waiting']==held['provider']['maximum_active']==4 and held['queue']['claimed']==4 and held['queue']['pending']>0,'actual_held_provider_work')
 for phase in ['quiet','held']:
  rows=read(d/(phase+'-requests.json'));total+=len(rows)
  check(len(rows)==128 and {(x['concurrency'],x['ordinal']) for x in rows}=={(c,n) for c in [1,4,8,16] for n in range(32)},'all_original_request_ids')
  for row in rows:
   check(any(hit.get('file_path')=='stable.rs' for hit in row['hits']),'real_unchanged_source_hit')
   parts=sum(row[k] for k in ['caller_schedule_us','capture_admission_us','retrieval_us'])
   check(parts<=row['offered_to_api_return_us']<=parts+2,'complete_offered_timing_denominator')
   check(0<=row['offered_to_api_return_us']<2000000,'original_local_progress_bound')
  for c in [1,4,8,16]:
   values=sorted(x['offered_to_api_return_us'] for x in rows if x['concurrency']==c);original=next(x for x in s[phase] if x['concurrency']==c)
   calc={f'p{q}_us':values[math.ceil(q*len(values)/100)-1] for q in [50,95,99]}
   check(original['n']==len(values)==32 and original['max_us']==values[-1] and all(original[k]==v for k,v in calc.items()),'original_all_sample_quantile_reconstruction')
   cells.append({'seed':seed,'phase':phase,'concurrency':c,'n':32,**calc,'max_us':values[-1]})
 check(s['old_held_input_publications']==0 and s['new_provider_calls']>0 and s['old_provider']['active']==s['old_provider']['waiting']==0 and s['old_provider']['maximum_active']<=4,'model_retirement_drained_no_stale_publication')
 check(s['queue_final']['claimed']==s['queue_final']['pending']==s['queue_final']['uncovered']==0 and s['queue_final']['published']>0,'original_final_queue_drained')
 control=s['db_availability'];check(control['generation_before']==control['generation_after'] and control['generation_unchanged'] is True and control['writer_busy_timeout_ms']==100 and control['writer_acquire_and_rollback_us']<100000,'original_independent_db_control')
 check(s['fixed_watchdogs']=={'local_query_ms':2000,'progress_ms':5000} and all(0<s[k]<5000000 for k in ['initial_build_us','held_build_us','write_delete_us','switch_and_drain_us']),'original_build_and_model_progress_bounds')
 for resource in [s['quiet_resources'],held['resources'],s['held_after_resources'],s['final_resources']]:
  usage=resource['shared_process_owner']['usage']
  check(resource['resource_gate']=='attributed_combined_process' and resource['shared_process_owner']['components']==['test_runner','CodeIndex','fake_provider'],'combined_in_process_resource_attribution')
  check(resource['server']['separate_pid'] is None and resource['server_tree']['child_process_usage'] is None and resource['no_zero_fill'] is True,'unavailable_child_usage_not_zero')
  check(usage['pid']>0 and usage['peak_resident_bytes']>0,'native_self_resource_witness')
  resources.append({'seed':seed,'stage':resource['stage'],'pid':usage['pid'],'current_rss_bytes':usage['resident_bytes'],'lifetime_peak_rss_bytes':usage['peak_resident_bytes'],'user_cpu_ns':usage['user_cpu_ns'],'system_cpu_ns':usage['system_cpu_ns']})
 before=s['quiet_resources']['shared_process_owner']['usage'];after=s['held_after_resources']['shared_process_owner']['usage']
 check(before['pid']==after['pid'] and all(after[k]>=before[k] for k in ['user_cpu_ns','system_cpu_ns']),'same_native_owner_monotonic_cpu')
 seed_summaries.append({'seed':seed,'old_held_input_publications':s['old_held_input_publications'],'new_provider_calls':s['new_provider_calls'],'final_queue':s['queue_final'],'max_provider_active':s['old_provider']['maximum_active'],'write_delete_us':s['write_delete_us'],'switch_and_drain_us':s['switch_and_drain_us']})
check(total==768,'all_768_requests')
check(r['paid_provider_requests']==0 and r['task_complete'] is False and r['release_approval'] is False,'fake_provider_scope_preserved')
out={'reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'artifact_id':artifact_id,'run_id':37896539291,'source_sha':snap['source_commit'],'source_input_count':snap['input_count'],'observer_count':len(observer['files']),'review_status':'raw_and_receipt_review_passed' if not errors else 'review_failed','errors':errors,'checks':dict(checks),'requests':total,'cells':cells,'seeds':seed_summaries,'resource_observations':resources,'seal_file_count':len(seal['artifact_inventory']),'execution_elapsed_ns':r['execution_elapsed_ns'],'executable_sha256':r['executable_sha256'],'scope':'Original released Linux integration test executed once with the original worker and fake provider. Mac reused original source/observer/inventory verifiers, independently reconstructed all request populations/quantiles and original gate facts. No primary rerun, no external provider certification, no standalone task completion or new tail-performance SLA. Original Linux compiler binaries were not re-executed on macOS.'}
dest=root/'review-pr/pr178-ci-observation/raw-review';dest.mkdir(exist_ok=True);path=dest/('backfill-'+str(artifact_id)+'-review-v3.json');path.write_text(json.dumps(out,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
print(json.dumps({k:out[k] for k in ['review_status','errors','requests','source_input_count','observer_count','seal_file_count','seeds']},ensure_ascii=False));print(str(path))
