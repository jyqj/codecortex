from pathlib import Path
import sys,json,hashlib,collections,datetime,math,subprocess
sys.dont_write_bytecode=True
artifact_id=int(sys.argv[1]);expected_zip=sys.argv[2];expected_profile=sys.argv[3];expected_c=int(sys.argv[4])
root=Path.cwd();source=(root/'source').resolve();sys.path.insert(0,str(source/'scripts'))
from p7_build_identity import source_snapshot
from p8_runtime_build import verify_output,observer_snapshot
from p8_runtime import rss_trend,sample_coverage,latency_summary,require_stable_symbol,soak_cache_summary,COMPACTION_EVENT
base=root/'raw'/str(artifact_id)/'extracted';build=base/'p8-build';m=base/'p8-runtime'
read=lambda p:json.loads(p.read_text())
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for chunk in iter(lambda:f.read(1048576),b''):h.update(chunk)
 return h.hexdigest()
def payload_key(x):return hashlib.sha256(json.dumps(x,sort_keys=True,separators=(',',':')).encode()).hexdigest()
errors=[];checks=collections.Counter()
def check(ok,name):
 checks[name]+=1
 if not ok and len(errors)<100:errors.append(name)
r=read(m/'report.json');plan=read(m/'plan.json');b=read(build/'build-receipt.json');stats=read(m/'statistics.json');parity=read(m/'parity.json')
check(sha(root/'raw'/str(artifact_id)/'original.zip')==expected_zip,'github_archive_digest')
runtime_seal=verify_output(m);build_seal=verify_output(build);checks['original_inventory_verifier']=2
cli=subprocess.run([sys.executable,'-B',str(source/'scripts/p8_runtime.py'),'verify','--output',str(m),'--build-output',str(build)],capture_output=True,text=True,timeout=60)
check(cli.returncode==0,'original_runtime_verify_cli')
snap=source_snapshot(source);observer=observer_snapshot(source)
check(snap['source_commit']=='a23bb72d3c954f385b99fe81ce9189885c208557','exact_a23_source')
check(snap==b['source_before']==b['source_after']==plan['source']==plan['build_identity']['source']==r['final_build_verification']['source'],'all_exact_source_snapshots')
check(observer==b['observer_before']==b['observer_after']==plan['build_identity']['observer']==r['final_build_verification']['observer'],'all_observer_snapshots')
check(plan['build_identity']==r['final_build_verification'],'original_before_after_build_verification')
check(sha(build/'build-receipt.json')==plan['build_identity']['receipt_sha256'],'original_build_receipt_digest')
check(sha(build/'seal.json')==plan['build_identity']['build_seal_sha256'],'original_build_seal_binding')
for f in (m/'build-evidence').rglob('*'):
 if f.is_file():check(sha(f)==sha(build/f.relative_to(m/'build-evidence')),'copied_original_build_evidence')
for path,item in observer['files'].items():check(sha(build/'observer-source'/path)==item['sha256'],'retained_observer_source')
check(b['status']=='passed' and b['build_exit_code']==0 and b['target_initially_absent'] is True and b['build_dir']==b['target_dir'] and b['toolchain_before']==b['toolchain_after'],'fresh_private_original_build')
check(b['compiler_environment']=={'RUSTC':b['toolchain_before']['rustc']['invocation'],'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':''},'original_compiler_selection')
messages=[json.loads(x) for x in (build/'product-build.jsonl').open()]
check(any(x.get('reason')=='build-finished' and x.get('success') is True for x in messages),'original_cargo_success')
check(sha(build/'product-build.jsonl')==b['cargo_log_sha256'] and sha(build/'product-build.stderr')==b['stderr_sha256'],'original_cargo_log_hashes')
check(set(b['artifacts'])=={'codecortex','p8-oracle','p8-runtime-statistics'},'complete_product_oracle_statistics_set')
for name,stored in b['artifacts'].items():
 selected=[x for x in messages if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')==name and x.get('target',{}).get('kind')==['bin']]
 check(selected==[stored['cargo_artifact']],'actual_unique_cargo_artifact')
 a=stored['cargo_artifact'];profile=a['profile']
 check(a['fresh'] is False and a['features']==[] and profile['opt_level']=='3' and profile['test'] is False and profile['debug_assertions'] is False,'actual_fresh_default_release_profile')
 check(sha(build/name)==stored['binary_sha256']==stored['copy_source']['sha256'] and (build/name).stat().st_size==stored['binary_bytes']==stored['copy_source']['bytes'],'retained_actual_binary_copy')
check(sha(build/'codecortex')==plan['product_sha256']==r['product_sha256'],'product_binary_binding')
check(sha(build/'p8-oracle')==plan['oracle_sha256']==r['oracle_sha256'],'oracle_binary_binding')
check(sha(build/'p8-runtime-statistics')==plan['statistics_sha256']==stats['replay_binary_sha256'],'statistics_binary_binding')
operations=3601 if expected_profile=='soak' else 900
expected_interval=1000 if expected_profile=='soak' else 500
check(plan['profile']==r['profile']==expected_profile and plan['concurrency']==expected_c and plan['operations']==operations and plan['files']==1000 and plan['offer_interval_ms']==expected_interval and plan['queue_capacity']==128 and plan['resource_interval_seconds']==1 and plan['request_timeout_seconds']==60,'original_fixed_workload_plan')
check(plan['offer_schedule']==('uniform' if expected_profile=='soak' else 'fixed_concurrency_sized_bursts'),'original_offer_schedule')
check(sha(m/'raw.jsonl')==r['raw_sha256']==stats['raw_sha256'] and sha(m/'plan.json')==r['plan_sha256']==stats['plan_sha256'],'complete_raw_plan_binding')
check(r['status']=='passed_observation' and r['exit_code']==0 and r['failures']==[] and r['outcomes']=={'success':operations},'actual_workload_outcome')
check(sha(m/'statistics.json')==r['statistics']['sha256'] and (m/'statistics.json').read_bytes()==(m/'statistics-replay.json').read_bytes() and r['statistics']['replay_identical'] is True,'original_statistics_double_replay')
execution=read(m/'statistics-execution.json')
check(len(execution)==2 and all(x['exit_code']==0 for x in execution) and stats['exit_code']==0 and stats['observation_status']=='all_original_terminal_outcomes_successful','original_statistics_execution')
check(stats['expected_samples']==stats['recorded_samples']==operations and stats['missing_samples']==stats['unexpected_samples']==0 and stats['configured_concurrency']==expected_c,'complete_statistics_denominator')
raw=[json.loads(x) for x in (m/'raw.jsonl').open()];rows=[x for x in raw if x['kind']=='operation'];resources=[x for x in raw if x['kind']=='resources'];kinds=collections.Counter(x['kind'] for x in raw)
check(len(rows)==operations and {x['id'] for x in rows}==set(range(operations)),'all_original_operation_ids')
actions=['bounded_symbol_churn','add','rename','delete','real_git_branch_switch','restore_api'];mutations=[];counts=collections.Counter()
for row in rows:
 counts[row['operation']]+=1
 check(row['operation']==('build' if row['id']%3==0 else 'read') and row['status']=='success','original_operation_outcome')
 check(row['scheduled_ns']<=row['offered_ns']<=row['started_ns']<=row['call_started_ns']<=row['finished_ns'],'all_original_monotonic_stage_timings')
 if row['operation']=='build':
  n=row['mutation_ordinal'];mutations.append(n)
  check(row['mutation']['action']==actions[n%6] and row['response']['parse_errors']==[] and row['response']['resolution_freshness']['complete'] is True,'original_complete_mutation_result')
 else:
  check('mutation' not in row and 'mutation_ordinal' not in row,'read_has_no_mutation')
  require_stable_symbol(row['response']);checks['original_stable_symbol_validator']+=1
check(sorted(mutations)==list(range((operations+2)//3)),'complete_serial_write_admission_sequence')
check(counts=={'build':(operations+2)//3,'read':operations-(operations+2)//3},'complete_read_build_population')
check(r['latency']==latency_summary(rows),'original_all_attempt_latency')
for kind in ['read','build']:check(r['latency_by_operation'][kind]==latency_summary([x for x in rows if x['operation']==kind]),'original_operation_latency')
check(r['rss']==rss_trend(resources),'original_rss_trend_validator')
coverage=r['resource_time_coverage']
check(coverage==sample_coverage(resources,coverage['work_start_ns'],coverage['work_end_ns']),'original_resource_coverage_validator')
check(sum(x.get('mutation',{}).get('action')=='real_git_branch_switch' for x in rows)==r['real_branch_switches'],'actual_branch_switch_count')
check((m/'product/product-stderr.log').read_text().count(COMPACTION_EVENT)==r['observed_catalog_compactions'],'actual_compaction_log_count')
check(r['actual_concurrency']['read']==r['actual_concurrency']['build']==0 and r['actual_concurrency']['maximum']<=expected_c,'drained_bounded_workload_concurrency')
timeline=[]
for row in rows:timeline.extend([(row['call_started_ns'],1,row['operation']),(row['finished_ns'],-1,row['operation'])])
active=collections.Counter();overlap=False;maximum=0
for when,delta,kind in sorted(timeline,key=lambda x:(x[0],x[1])):
 active[kind]+=delta;maximum=max(maximum,sum(active.values()));overlap|=active['read']>0 and active['build']>0
if expected_profile=='mixed' and expected_c>1:check(overlap and r['actual_concurrency']['read_build_overlap'] is True,'independent_real_call_read_build_overlap')
def ci(values,q):
 values=sorted(values);n=len(values)
 if not n:return None
 log_failure=math.log1p(-q);odds=math.log(q)-log_failure;lp=n*log_failure;probs=[]
 for k in range(n+1):
  probs.append(math.exp(lp))
  if k<n:lp+=math.log(n-k)-math.log(k+1)+odds
 total=sum(probs)
 def rank(target):
  cumulative=0
  for k,v in enumerate(probs):
   cumulative+=v/total
   if cumulative>=target:return k
  return n
 low=rank(.025);high=rank(.975)+1
 return {'quantile':q,'confidence':.95,'samples':n,'estimate_us':values[math.ceil(n*q)-1],'lower_us':values[low-1] if low>=1 else None,'upper_us':values[high-1] if high<=n else None,'method':'binomial_order_statistics_95pct_iid_assumption'}
group_summary=[]
for group in stats['by_operation']+stats['by_build_mutation']:
 selected=[x for x in rows if x['operation']==group['operation'] and (group['mutation_action'] is None or x.get('mutation',{}).get('action')==group['mutation_action'])]
 n=len(selected);check(group['recorded_samples']==group['expected_samples_in_group']==group['successful_samples']==n and group['non_success_samples']==0,'original_statistic_group_population')
 metrics={'all_attempt_offered_to_terminal':[(x['finished_ns']-x['offered_ns'])//1000 for x in selected],'successful_offered_to_terminal':[(x['finished_ns']-x['offered_ns'])//1000 for x in selected],'scheduled_to_terminal':[(x['finished_ns']-x['scheduled_ns'])//1000 for x in selected],'client_dispatch_wait':[(x['started_ns']-x['offered_ns'])//1000 for x in selected],'write_admission_and_preparation':[(x['call_started_ns']-x['started_ns'])//1000 for x in selected],'client_call_to_terminal_including_validation':[(x['finished_ns']-x['call_started_ns'])//1000 for x in selected]}
 for name,values in metrics.items():
  values.sort();entry=group[name];dist=entry['distribution']
  check(entry['recorded_samples']==n and entry['missing_samples']==0 and dist['samples']==n and dist['max_us']==values[-1] and dist['p50_us']==values[math.ceil(n*.5)-1] and dist['p95_us']==values[math.ceil(n*.95)-1],'all_attempt_distribution_reconstruction')
  check(entry['p95_ci']==ci(values,.95) and entry['p99_ci']==ci(values,.99),'full_original_iid_interval_reconstruction')
 group_summary.append({'operation':group['operation'],'mutation_action':group['mutation_action'],'n':n,'latency':group['all_attempt_offered_to_terminal']})
check(sha(m/'parity.json')==r['parity_sha256'] and parity['exit_code']==r['parity_exit_code']==0 and parity['error'] is None and parity['comparison']['equal'] is True and parity['comparison']['different_tables']==[],'original_endpoint_parity_result')
tables=parity['comparison']['tables']
check(len(tables)==15 and [x['table'] for x in tables]==parity['tables'] and all(x['equal'] is True and x['different_row_count']==0 and x['different_row_examples']==[] and x['full_rows']==x['incremental_rows'] and x['full_digest']==x['incremental_digest'] for x in tables),'all_original_15_table_comparisons')
check(len([x for x in raw if x['kind']=='oracle_process'])==1 and next(x for x in raw if x['kind']=='oracle_process')['exit_code']==0,'actual_original_oracle_process')
for kind in ['initial_build','full_control']:
 one=[x for x in raw if x['kind']==kind];check(len(one)==1 and one[0]['report']['parse_errors']==[] and one[0]['report']['resolution_freshness']['complete'] is True,'complete_original_initial_and_full_control')
public=next(x for x in raw if x['kind']=='endpoint_public')
require_stable_symbol(public['incremental']);require_stable_symbol(public['full']);checks['original_endpoint_public_validator']=2
def authored(path):
 return {f.relative_to(path).as_posix():sha(f) for f in path.rglob('*') if f.is_file() and not any(part in ['.git','.codecortex'] for part in f.relative_to(path).parts)}
left=authored(m/'project');right=authored(m/'fresh-full');check(left==right and len([p for p in left if p.endswith('.py')])==1000,'independent_full_fixture_authored_bytes')
rpc_summaries={};transport={}
for label in ['product','full-product']:
 events=[json.loads(x) for x in (m/label/'rpc.jsonl').open()]
 requests=[x for x in events if x.get('event')=='request' and isinstance(x.get('payload'),dict) and x['payload'].get('method')=='tools/call']
 responses={x['payload']['id']:x for x in events if x.get('event')=='response' and isinstance(x.get('payload'),dict)}
 index=[x for x in requests if x['payload']['params']['name']=='index'];symbol=[x for x in requests if x['payload']['params']['name']=='search' and x['payload']['params']['arguments'].get('mode')=='symbol'];hybrid=[x for x in requests if x['payload']['params']['name']=='search' and x['payload']['params']['arguments'].get('mode')=='hybrid'];status=[x for x in requests if x['payload']['params']['name']=='status']
 process=read(m/label/'process.json')
 check(process['exit_code']==process['expected_exit_code']==0 and process['cleanup']=='completed' and process['binary_sha256']==r['product_sha256'],'original_product_process_closed')
 for request in requests:check(request['payload']['id'] in responses and 'error' not in responses[request['payload']['id']]['payload'],'complete_transport_response_ids')
 transport[label]={'index':index,'symbol':symbol,'hybrid':hybrid,'status':status,'responses':responses,'process':process}
 rpc_summaries[label]={'index':len(index),'symbol':len(symbol),'hybrid':len(hybrid),'status':len(status),'pid':process['pid'],'exit_code':process['exit_code']}
main=transport['product'];full=transport['full-product'];build_n=counts['build'];read_n=counts['read']
check(len(main['index'])==build_n+1 and main['index'][0]['payload']['params']['arguments']['full'] is True and all(x['payload']['params']['arguments']['full'] is False for x in main['index'][1:]),'no_extra_incremental_repair_index_calls')
check(len(full['index'])==1 and full['index'][0]['payload']['params']['arguments']['full'] is True and len(full['symbol'])==1,'independent_single_full_control')
check(len(main['symbol'])==read_n+1 and len(main['hybrid'])==(read_n if expected_profile=='soak' else 0) and len(main['status'])==len(resources)+1+(2*read_n if expected_profile=='soak' else 0),'original_complete_stdio_rpc_denominator')
work=main['index'][1:]+main['symbol'][:-1]
def decoded(request):
 payload=main['responses'][request['payload']['id']]['payload']['result']
 check(payload.get('isError') is not True and isinstance(payload.get('structuredContent'),dict),'original_successful_mcp_tool_envelope')
 value=payload['structuredContent'];return value.get('result',value)
check(collections.Counter(payload_key(decoded(x)) for x in work)==collections.Counter(payload_key(x['response']) for x in rows),'all_raw_outcomes_match_original_stdio_payloads')
check(max(main['responses'][x['payload']['id']]['time_ns'] for x in work)<full['index'][0]['time_ns'],'full_control_started_after_original_work_drained')
check(r['owned_cleanup']=={'unfinished_work':0,'sampler_stopped':True,'product_stopped':True,'comparison_product_stopped':True,'product_construction_pending':False,'comparison_product_construction_pending':False},'all_owned_cleanup_drained')
for item in resources:
 check(item['server']['pid']==main['process']['pid'] and item['runner']['pid']!=item['server']['pid'] and item['server']['resident_bytes']>0,'native_resource_owner_and_units')
soak=None
if expected_profile=='soak':
 soak=soak_cache_summary(rows,operations,coverage['work_start_ns'],coverage['work_end_ns'],m/'project')
 check(soak==r['cache_reuse'] and soak['passed'] is True,'original_soak_cache_validator')
 check(r['observed_work_ns']>=3600000000000 and r['rss']['passed'] is True and coverage['passed'] is True and r['real_branch_switches']>=2 and r['observed_catalog_compactions']>0,'original_complete_long_soak_gates')
check(r['task_complete'] is False and r['release_approval'] is False and r['semantic_backfill']=='not_run_in_default_product_profile','scope_and_dependency_boundaries')
out={'reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'artifact_id':artifact_id,'run_id':37871838957,'source_sha':snap['source_commit'],'profile':expected_profile,'configured_concurrency':expected_c,'review_status':'raw_and_receipt_review_passed' if not errors else 'review_failed','errors':errors,'checks':dict(checks),'original_verify_cli':{'exit_code':cli.returncode,'stdout':cli.stdout,'stderr':cli.stderr},'source_input_count':snap['input_count'],'observer_count':len(observer['files']),'seal_file_count':len(runtime_seal['artifact_inventory']),'build_seal_file_count':len(build_seal['artifact_inventory']),'raw_kind_counts':dict(kinds),'outcomes':dict(counts),'actual_concurrency':r['actual_concurrency'],'independent_call_interval_maximum':maximum,'independent_call_interval_read_build_overlap':overlap,'observed_work_ns':r['observed_work_ns'],'branch_switches':r['real_branch_switches'],'catalog_compactions':r['observed_catalog_compactions'],'resource_time_coverage':coverage,'rss':r['rss'],'latency':r['latency'],'statistics_groups':group_summary,'parity_tables':[{'table':x['table'],'rows':x['full_rows'],'equal':x['equal'],'digest':x['full_digest']} for x in tables],'stdio_counts':rpc_summaries,'soak_cache_replay':soak,'scope':'Original a23 Linux product/oracle/statistics primary observations retained. Mac reran original Python source/observer/seal/runtime verify/RSS/coverage/latency/public-symbol/soak-cache verifiers and independently reconstructed every sample timing, original IID intervals, exact stdio payload associations and no-extra-index denominator. Original Rust Linux ELF binaries and original Linux compiler paths were not executed on macOS. Original native statistics double replay and original native 15-table oracle are bound by their bytes and receipts. No new primary run, task closure or performance improvement claim.'}
dest=root/'review-runtime';dest.mkdir(exist_ok=True);path=dest/('runtime-'+str(artifact_id)+'-review.json');path.write_text(json.dumps(out,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
print(json.dumps({k:out[k] for k in ['review_status','errors','profile','configured_concurrency','outcomes','raw_kind_counts','actual_concurrency','independent_call_interval_maximum','independent_call_interval_read_build_overlap','stdio_counts','source_input_count','observer_count','seal_file_count','build_seal_file_count']},ensure_ascii=False));print(str(path))
sys.exit(1 if errors else 0)
