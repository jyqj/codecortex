from pathlib import Path
import sys,json,hashlib,collections,datetime,math
sys.dont_write_bytecode=True
root=Path.cwd();source=(root/'source').resolve();sys.path.insert(0,str(source/'scripts'))
from p7_build_identity import source_snapshot,verify_release_profile
from p8_lifecycle import verify_artifact_inventory,cache_observation,index_identity,usage_delta
from p8_rollback import source_manifest
base=root/'raw/11591607348/extracted';m=base/'measurement'
read=lambda p:json.loads(p.read_text())
def sha(p):
 h=hashlib.sha256()
 with p.open('rb') as f:
  for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
 return h.hexdigest()
r=read(m/'receipt.json');p=read(m/'report.json');i=read(m/'replay-input.json')
b=read(base/'product/build-receipt.json');e=read(base/'replay-build-receipt.json')
errors=[];checks=collections.Counter()
def check(ok,name):
 checks[name]+=1
 if not ok and len(errors)<100:errors.append(name)
prefix=Path('/home/runner/work/_temp/p8-lifecycle')
def mapped(path):return base/Path(path).relative_to(prefix)
check(sha(root/'raw/11591607348/original.zip')=='63aadf94879ea86782e9759828e853c1a030d70014fdc3fbdc127a18ca2eb055','github_archive_digest')
verify_artifact_inventory(m,r);checks['original_inventory_verifier']=1
check(sha(m/'receipt.json')=='b1f1527d247036d2ec0faf7f270a79faf9bcda9f19f30b0f68527bd1bf9325d3','original_log_receipt_digest')
snap=source_snapshot(source);inputs=snap.pop('inputs')
check(snap['source_commit']=='a23bb72d3c954f385b99fe81ce9189885c208557','exact_source')
check(snap==b['source_before']==b['source_after']==e['source_before']==e['source_after']==r['product']['source'],'all_source_bindings')
check(inputs==read(base/'product/source-inputs.json'),'all_1087_source_inputs')
for path,digest in r['observer_sha256'].items():
 relative=Path(path).relative_to('/home/runner/work/codecortex/codecortex')
 check(sha(source/relative)==digest,'original_observer_hash')
check(sha(base/'product/codecortex')==b['binary_sha256']==r['product']['binary_sha256'],'product_binary_digest')
check(sha(base/'product/build-receipt.json')==r['product']['receipt_sha256'],'product_receipt_digest')
check(sha(base/'p8-measurements')==e['binary_sha256']==r['evaluator_sha256']==p['replay_binary_sha256'],'original_evaluator_digest')
verify_release_profile(b['actual_cargo_profile']);verify_release_profile(e['artifact']['profile']);checks['original_release_profile_verifier']=2
check(b['cargo_target_initially_absent'] is True and b['build_exit_code']==e['build_exit_code']==0,'fresh_successful_build')
for log,art,name in [(base/'product/cargo-build.jsonl',b['cargo_artifact'],'codecortex'),(base/'replay-build.jsonl',e['artifact'],'p8-measurements')]:
 events=[json.loads(line) for line in log.open()]
 rows=[x for x in events if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')==name and x.get('executable')]
 check(len(rows)==1 and rows[0]==art,'actual_cargo_artifact')
 check(any(x.get('reason')=='build-finished' and x.get('success') is True for x in events),'actual_cargo_completion')
for field in ['build_stdout','build_stderr']:
 check(sha(base/e[field]['name'])==e[field]['sha256'],'evaluator_build_log_digest')
check(sha(m/'replay-input.json')==r['replay']['input_sha256']==p['input_sha256'],'replay_input_digest')
check(sha(m/'report.json')==r['replay']['report_sha256'],'report_digest')
repeat=read(m/'repeat-replay-receipt.json')
check((m/'report.json').read_bytes()==(m/'report-replayed.json').read_bytes() and repeat['byte_identical_report'] is True and repeat['exit_code']==0,'original_native_double_replay')
check(r['status']==p['measurement_status']=='complete_observation' and r['exit_code']==p['exit_code']==r['replay']['exit_code']==0 and r['failures']==[],'actual_observation_success')
check(r['plan']=={'artifact_budget_bytes':536870912,'cold_samples':30,'files':32,'profile':'release','query_samples':400,'request_timeout_seconds':30,'run_timeout_seconds':3600},'unchanged_registered_plan')
check(r['sample_counts']=={'expected':1230,'recorded':1230} and i['expected_samples']==len(i['samples'])==1230,'all_1230_samples')
check(all(x['status']=='success' and isinstance(x['elapsed_us'],int) and x['elapsed_us']>=0 for x in i['samples']),'all_sample_outcomes')
queries={q['sample_index']:q for q in i['raw_queries']}
check(len(queries)==len(i['raw_queries'])==1200 and set(queries)==set(range(30,1230)),'all_1200_query_witnesses')
check(p['source_verified_queries']==p['retained_query_witnesses']==1200,'original_rust_source_validation_count')
raw_counts=collections.Counter();phases=collections.Counter();cache_counts=collections.Counter();members=collections.Counter()
sessions={};closed={};cold_index=0;resource_index=0;query_seen=set();pristine=0;final_storage=[]
for line in (m/'events.jsonl').open():
 event=json.loads(line);kind=event['kind'];raw_counts[kind]+=1
 if kind=='session_started':
  check(event['session'] not in sessions,'unique_session_start');sessions[event['session']]=event
 elif kind=='session_closed':
  closed[event['session']]=event
  check(event['process']['exit_code']==event['process']['expected_exit_code']==0 and event['process']['cleanup']=='completed','native_session_closed')
 elif kind=='pristine_fixture':
  check(event['index_and_persistent_parse_cache_absent'] is True,'pristine_index_parse_cache')
  check(event['source_manifest']==source_manifest(mapped(event['project'])),'authored_fixture_unchanged')
  check(len(event['source_manifest'])==32,'all_32_fixture_sources')
  pristine+=1
 elif kind=='cold_build':
  check(event['error'] is None and index_identity(event['before'])['indexed_files']==0 and index_identity(event['after'])['indexed_files']==32,'real_cold_build_0_to_32')
  check(i['samples'][cold_index]['elapsed_us']==(event['finished_ns']-event['started_ns'])//1000,'cold_timing_raw_reconstruction')
  cold_index+=1
 elif kind=='reopen_existing_index':
  check(event['build_calls_in_process']==0 and event['source_manifest_unchanged'] is True,'reopen_no_build_no_source_change')
 elif kind=='query_attempt':
  phase=event['phase'];phases[phase]+=1
  cache,control=cache_observation(event['before'],event['after']);cache_counts[cache]+=1
  check(event['error'] is None and cache==('hit' if phase=='cache_hit' else 'miss') and control==event['cache_control'],'original_cache_validator_on_raw')
  check(event['native_usage_delta']==usage_delta(event['before']['diagnostics']['process_resources'],event['after']['diagnostics']['process_resources']),'original_usage_delta_on_raw')
  check(event['elapsed_us']==(event['finished_ns']-event['started_ns'])//1000,'query_timing_raw_reconstruction')
  if event['sample_index'] is not None:
   idx=event['sample_index'];query_seen.add(idx);q=queries[idx];sample=i['samples'][idx]
   check(event['response']==q['response'] and event['elapsed_us']==sample['elapsed_us'] and sample['evidence']['result_cache']==cache,'raw_event_input_association')
   check(event['before']['diagnostics']['process_resources']['pid']==event['pid']==sessions[event['session']]['pid'],'owned_native_query_pid')
   hits=q['response'].get('machine_pack',{}).get('hits',[])
   expected=[hit for hit in hits if hit['file_path']==q['expected_path'] and hit.get('symbol_name')==q['expected_symbol']]
   check(len(expected)==1,'unique_expected_raw_hit')
   for hit in expected:
    data=(mapped(q['source_root'])/hit['file_path']).read_bytes();proof=hit['metadata']['source_evidence'];span=proof['span']
    check(data[span['start']:span['end']].decode()==hit['text'] and len(data)==proof['source']['byte_len'],'raw_source_span_bytes')
    check(hit['metadata']['source_freshness']['status']=='current_verified' and hit['metadata']['source_freshness']['disk_checked'] is True,'raw_current_source_witness')
 elif kind=='resource':
  ledger=event['ledger'];tree=event['owned_tree'];native=event['server_native'];owner=event['runner_native'];member_rows=tree['tree']['members'];members[len(member_rows)]+=1
  check(ledger==i['resources'][resource_index],'all_raw_resource_ledger_associations')
  check(native['pid']==ledger['server_pid']==tree['namespace_pid']==sessions[event['session']]['pid'],'native_server_tree_pid_attribution')
  check(owner['namespace_pid']==ledger['runner_pid'] and ledger['runner_pid']!=ledger['server_pid'],'native_runner_separate')
  check(tree['tree']['complete'] is True and tree['identity']['namespace_pids'][-1]==ledger['server_pid'],'complete_owned_tree')
  for metric in ['resident_bytes','user_cpu_ns','system_cpu_ns']:
   check(tree['tree'][metric]==sum(row[metric] for row in member_rows),'raw_tree_sum')
  check(ledger['server_rss_bytes']==native['resident_bytes'] and ledger['server_tree_rss_bytes']==tree['tree']['resident_bytes'],'rss_units_and_role_binding')
  resource_index+=1
 elif kind=='final_closed_fixture_storage':
  final_storage.append(event)
check(raw_counts=={'pristine_fixture':30,'session_started':431,'resource':1261,'cold_build':30,'session_closed':431,'closed_fixture_storage':30,'reopen_existing_index':400,'query_attempt':1201,'final_closed_fixture_storage':30},'complete_raw_event_population')
check(phases=={'process_reopen':400,'warmup':1,'warm_uncached':400,'cache_hit':400} and cache_counts=={'miss':801,'hit':400},'actual_cache_phase_population')
check(set(sessions)==set(closed) and len(sessions)==r['session_count']==431,'all_sessions_closed')
check(query_seen==set(range(30,1230)),'all_query_samples_reconstructed')
last_close=max(x['monotonic_ns'] for x in closed.values())
check(len(final_storage)==len(r['storage'])==30 and all(x['physical_snapshot']['started_monotonic_ns']>last_close for x in final_storage),'final_storage_after_all_sessions_closed')
objects=[o for storage in r['storage'] for o in storage['objects']]
check(len(objects)==90 and len({o['storage_id'] for o in objects})==90,'unique_physical_storage_ids')
for obj in objects:check(mapped(obj['path']).stat().st_size==obj['bytes'],'retained_physical_file_lengths')
check(sum(obj['bytes'] for obj in objects)==p['disk_ledger']['total_bytes']==128901120,'physical_disk_total_no_fts_double_count')
layers={}
for layer in p['latency_layers']['layers']:
 name=layer['stratum'];n=layer['samples']
 if name=='cold_build':values=[x['elapsed_us'] for x in i['samples'][:30]]
 elif name=='process_reopen':values=[x['elapsed_us'] for x in i['samples'][30:430]]
 elif name=='warm_uncached':values=[x['elapsed_us'] for x in i['samples'][430::2]]
 elif name=='cache_hit':values=[x['elapsed_us'] for x in i['samples'][431::2]]
 else:values=[]
 values.sort()
 check(n==len(values) and layer['completed']==n and all(layer[k]==0 for k in ['errors','timeouts','cancelled','partial','missing_timings']),'latency_population')
 if values:
  check(layer['all_attempt_elapsed']['p50_us']==values[math.ceil(n*.5)-1] and layer['all_attempt_elapsed']['p95_us']==values[math.ceil(n*.95)-1],'all_attempt_quantiles')
  for q in [0.95,0.99]:
   ci=layer['p95_completed_ci' if q==0.95 else 'p99_completed_ci']
   check(ci['estimate_us']==values[math.ceil(n*q)-1] and ci['samples']==n,'ci_estimate_population')
 layers[name]={'n':n,'p50_us':layer['all_attempt_elapsed']['p50_us'],'p95_us':layer['all_attempt_elapsed']['p95_us'],'p99_us':layer['p99_completed_ci']['estimate_us'] if n else None,'p95_ci':layer['p95_completed_ci'],'p99_ci':layer['p99_completed_ci'],'statistical_status':layer['statistical_status']}
memory={**{k:v for k,v in p['memory_ledger'].items() if k!='roles'},'roles':[{**{k:v for k,v in row.items() if k!='root_pids'},'root_pid_count':len(row['root_pids'])} for row in p['memory_ledger']['roles']]}
check(memory['total_rss_bytes'] is None and memory['sampling_interval_ms'] is None,'no_double_count_or_continuous_peak_claim')
check(i['costs']==[] and p['cost_ledger']['status']=='unavailable' and p['cost_ledger']['totals']==[] and r['provider_cost']['status']=='not_applicable_to_disabled_profile' and all(r['provider_cost'][k] is None for k in ['reported','estimated','input_tokens','output_tokens','requests_billed']),'disabled_provider_cost_not_zero')
out={'reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'artifact_id':11591607348,'run_id':37871838975,'source':snap,'review_status':'raw_and_receipt_review_passed' if not errors else 'review_failed','errors':errors,'checks':dict(checks),'measurement_status':r['status'],'sample_counts':r['sample_counts'],'sessions':len(sessions),'raw_event_counts':dict(raw_counts),'actual_cache_counts':dict(cache_counts),'tree_member_counts':dict(members),'latency_layers':layers,'memory_ledger':memory,'disk_ledger':{k:v for k,v in p['disk_ledger'].items() if k!='partitions'},'provider_cost':r['provider_cost'],'seal_file_count':len(r['artifact_inventory']),'scope':'Original Linux workload and original native evaluator ran twice there; this Mac review independently reran original source/inventory/release/cache/usage verifiers and raw associations. No primary workload was repeated. Original Linux ELF evaluator was not executed on macOS; original Rust BLAKE3 normalizer result retained and bound, raw source spans independently checked. No task closure or dependency bypass.'}
dest=root/'review-runtime';dest.mkdir(exist_ok=True);(dest/'lifecycle-11591607348-review.json').write_text(json.dumps(out,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
print(json.dumps({k:out[k] for k in ['review_status','errors','sample_counts','sessions','raw_event_counts','actual_cache_counts','tree_member_counts','seal_file_count','disk_ledger']},ensure_ascii=False))
print('review_path',str(dest/'lifecycle-11591607348-review.json'))
