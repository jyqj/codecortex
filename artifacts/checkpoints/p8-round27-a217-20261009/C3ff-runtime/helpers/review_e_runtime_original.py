import pathlib,json,hashlib,sys,subprocess,shutil,tempfile,collections
from e_raw_cache_audit import HEAD, RUN, PRODUCT_MANIFEST, file_hash, load_runtime_rows, confirmed_cleanup, audit_cache
assert len(sys.argv)==6, 'manifest artifact-root exact-head exact-frozen-checkout review-output-root required'
base=pathlib.Path(sys.argv[5]).resolve(strict=True)

repo=pathlib.Path(sys.argv[4]).resolve(strict=True)
head=sys.argv[3]
assert head==HEAD, 'helper frozen for exact fixed C3ff execution only'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head, 'frozen checkout HEAD differs'
assert len(head)==40 and all(x in '0123456789abcdef' for x in head)
def blob(path):return subprocess.check_output(['git','show',head+':'+path],cwd=repo)
# Load only observer bytes from the exact executed Git revision, not the evolving checkout.
frozen_matches=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
modules=repo if frozen_matches else base/'runtime-review'/('observer-replay-'+head)
for name in ['scripts/p8_runtime.py','scripts/p8_runtime_build.py','scripts/p7_build_identity.py','scripts/p8_cold_build.py','scripts/p8_rollback.py','scripts/p7_stdio_build_receipt.py','scripts/p8_backfill.py','scripts/resource_harness/__init__.py','scripts/resource_harness/runtime.py']:
 path=modules/name;data=blob(name)
 if frozen_matches:assert path.is_file() and path.read_bytes()==data
 else:path.parent.mkdir(parents=True,exist_ok=True)
 if path.exists():assert path.read_bytes()==data
 else:path.write_bytes(data);path.chmod(0o444)
sys.dont_write_bytecode=True
sys.path.insert(0,str(modules/'scripts'));import p8_runtime_build as owner; import p8_runtime as runtime
h=file_hash
g=blob
artifacts=json.loads(pathlib.Path(sys.argv[1]).read_text())
artifact_root=pathlib.Path(sys.argv[2])
all_reviews=[]
for c,artifact_id,size,sha in artifacts:
 root=artifact_root/str(artifact_id);zipfile=root/(str(artifact_id)+'.zip');assert zipfile.stat().st_size==size and h(zipfile)==sha
 original_metadata=json.loads((root/'artifact-metadata.json').read_text());metadata={'artifact_id':original_metadata['id'],'source':original_metadata['workflow_run']['head_sha'],'run_id':original_metadata['workflow_run']['id'],'size_in_bytes':original_metadata['size_in_bytes'],'digest':original_metadata['digest']};assert metadata['artifact_id']==artifact_id and metadata['source']==head and metadata['size_in_bytes']==size and metadata['digest']=='sha256:'+sha and metadata['run_id']==RUN
 p=root/'extracted';m=p/'p8-runtime';b=p/'p8-build';out=base/('artifact-'+str(artifact_id)+'-'+head[:8]+'-independent-final');out.mkdir(exist_ok=False)
 seals={str(x.relative_to(p)):len(owner.verify_output(x)['artifact_inventory']) for x in [b,m]}
 plan=json.loads((m/'plan.json').read_text());r=json.loads((m/'report.json').read_text());build=json.loads((b/'build-receipt.json').read_text());stats=json.loads((m/'statistics.json').read_text());parity=json.loads((m/'parity.json').read_text())
 n=3601 if plan['profile']=='soak' else 900
 assert plan['concurrency']==c and plan['operations']==n and plan['files']==1000 and plan['offer_interval_ms']==(1000 if plan['profile']=='soak' else 500)
 assert plan['source']['source_commit']==head and len(plan['source']['inputs'])==1092
 assert plan['source']['source_tree']==subprocess.check_output(['git','rev-parse',head+'^{tree}'],cwd=repo,text=True).strip()
 assert plan['source']['input_count']==1092 and plan['source']['manifest_sha256']==hashlib.sha256(owner.json_bytes(plan['source']['inputs'])).hexdigest()
 assert plan['source']['manifest_sha256']==PRODUCT_MANIFEST, 'combined product manifest differs'
 assert set(plan['source']['inputs'])==set(subprocess.check_output(['git','ls-tree','-r','--name-only',head,'--','Cargo.toml','Cargo.lock','crates'],cwd=repo,text=True).splitlines())
 assert all(hashlib.sha256(g(name)).hexdigest()==digest for name,digest in plan['source']['inputs'].items())
 assert r['status']=='passed_observation' and r['exit_code']==0 and r['failures']==[] and r['outcomes']=={'success':n}
 assert r['parity_exit_code']==0 and r['artifact_seal_status']=='sealed' and r['artifact_seal']=='seal.json'
 cleanup=confirmed_cleanup(r)
 assert all(cleanup[key] is True for key in ['sampler_stopped','product_stopped','comparison_product_stopped'])
 for component in ['product','full-product']:
  closed=json.loads((m/component/'process.json').read_text())
  assert closed['cleanup']=='completed' and closed['exit_code']==closed['expected_exit_code']==0 and closed['binary_sha256']==r['product_sha256']
 assert r['latency_by_operation']['read']['n']==n-(n+2)//3 and r['latency_by_operation']['build']['n']==(n+2)//3
 assert plan['profile']=='soak' or c==1 or r['actual_concurrency']['read_build_overlap'] is True
 assert r['actual_concurrency']['maximum']<=c
 assert h(m/'plan.json')==r['plan_sha256'] and h(m/'raw.jsonl')==r['raw_sha256'] and h(m/'parity.json')==r['parity_sha256']
 assert build['source_before']==build['source_after']==plan['source'] and build['observer_before']==build['observer_after']
 assert build['observer_before']['source_commit']==head, 'runtime observer manifest HEAD differs'
 assert set(build['observer_before']['files'])==set(owner.OBSERVER_FILES), 'runtime observer manifest inventory differs'
 assert build['target_initially_absent'] and build['toolchain_before']==build['toolchain_after'] and build['build_exit_code']==0
 assert build['status']=='passed' and build['target_dir']==build['build_dir']
 assert build['build_command']==['cargo','build','--release','--locked','--offline','--no-default-features','-p','cc-server','--bin','codecortex','-p','cc-eval','--bin','p8-oracle','--bin','p8-runtime-statistics','--message-format=json-render-diagnostics','--target-dir',build['target_dir']]
 assert set(build['artifacts'])=={'codecortex','p8-oracle','p8-runtime-statistics'}
 for name,a in build['artifacts'].items():
  assert h(b/name)==a['binary_sha256']==a['copy_source']['sha256'] and a['cargo_artifact']['fresh'] is False
  assert a['cargo_artifact']['features']==[] and a['cargo_artifact']['profile']['opt_level']=='3' and a['cargo_artifact']['profile']['debug_assertions'] is False
  assert a['cargo_artifact']['profile']==dict(debug_assertions=False,debuginfo=0,opt_level='3',overflow_checks=False,test=False)
  assert a['cargo_artifact']['target']['name']==name and a['cargo_artifact']['target']['kind']==['bin']
  assert a['copy_source']['path']==a['cargo_artifact']['executable'] and pathlib.PurePosixPath(a['copy_source']['path']).is_relative_to(build['target_dir'])
  assert a['binary_bytes']==a['copy_source']['bytes']==(b/name).stat().st_size
  package,src,prefix,event_field=owner.TARGETS[name]
  producer=pathlib.PurePosixPath(build['cargo_artifact']['manifest_path']).parents[2]
  assert a['cargo_artifact']['manifest_path']==str(producer/'crates'/package/'Cargo.toml')
  assert a['cargo_artifact']['target']['src_path']==str(producer/'crates'/package/src)
  assert a['cargo_artifact']['package_id'].rpartition('#')[0]=='path+file://'+str(producer/'crates'/package)
  assert a['cargo_artifact']['target']['crate_types']==['bin']
  assert build[event_field]==a['cargo_artifact'] and build[prefix+'_sha256']==a['binary_sha256'] and build[prefix+'_path']==a['binary_path']
 cargo_rows=[json.loads(line) for line in (b/'product-build.jsonl').open()]
 assert h(b/'product-build.jsonl')==build['cargo_log_sha256'] and h(b/'product-build.stderr')==build['stderr_sha256']
 assert [v for v in cargo_rows if v.get('reason')=='build-finished']==[dict(reason='build-finished',success=True)]
 for name,a in build['artifacts'].items():
  assert [v for v in cargo_rows if v.get('reason')=='compiler-artifact' and v.get('target',{}).get('name')==name]==[a['cargo_artifact']]
 for name,a in build['observer_before']['files'].items():
  assert h(b/'observer-source'/name)==a['sha256']==hashlib.sha256(g(name)).hexdigest()
 assert set(plan['retained_build_evidence'])=={'build-receipt.json','seal.json','source-before.json','source-after.json','product-build.jsonl','product-build.stderr'}|{'observer-source/'+name for name in owner.OBSERVER_FILES}
 assert r['final_build_verification']==plan['build_identity']
 for name,a in plan['retained_build_evidence'].items():
  assert h(m/'build-evidence'/name)==a['sha256']==h(b/name)
 assert h(m/'statistics.json')==h(m/'statistics-replay.json')==r['statistics']['sha256']
 assert r['latency']==stats['legacy_ns']['latency'], 'runtime legacy ns overall copy differs from bound Rust statistics'
 assert r['latency_by_operation']==stats['legacy_ns']['latency_by_operation'], 'runtime legacy ns operation copy differs from bound Rust statistics'
 assert stats['recorded_samples']==stats['expected_samples']==n and stats['missing_samples']==stats['unexpected_samples']==0
 raw,raw_offsets,raw_event_counts=load_runtime_rows(m/'raw.jsonl');ops=[x for x in raw if x['kind']=='operation'];assert len(ops)==n and {x['id'] for x in ops}==set(range(n))
 assert collections.Counter(x['operation'] for x in ops)=={'read':n-(n+2)//3,'build':(n+2)//3} and all(x['status']=='success' for x in ops)
 resources=[x for x in raw if x['kind']=='resources']
 assert runtime.rss_trend(resources)==r['rss']
 coverage=r['resource_time_coverage']
 assert runtime.sample_coverage(resources,coverage['work_start_ns'],coverage['work_end_ns'])==coverage
 assert r['observed_work_ns']<=coverage['work_end_ns']-coverage['work_start_ns']
 # The source reads monotonic_ns separately for span/end; preserve the actual small difference.
 coverage_clock_read_delta=coverage['work_end_ns']-coverage['work_start_ns']-r['observed_work_ns']
 assert len({x['server']['pid'] for x in resources})==1 and all(x['server']['pid']!=x['runner']['pid'] for x in resources)
 assert all(x['query_execution']['cpu_in_flight']<=x['query_execution']['cpu_limit'] and x['query_execution']['async_in_flight']<=x['query_execution']['async_limit'] for x in resources)
 for key in ['user_cpu_ns','system_cpu_ns']:
  values=[x['server'][key] for x in resources];assert values==sorted(values)
 assert r['real_branch_switches']==sum(x.get('mutation',{}).get('action')=='real_git_branch_switch' for x in ops)
 assert r['observed_catalog_compactions']==(m/'product/product-stderr.log').read_text().count(runtime.COMPACTION_EVENT)
 if plan['profile']=='soak':
  assert r['observed_work_ns']>=3600000000000 and r['rss']['passed'] and coverage['passed']
  assert r['real_branch_switches']>=2 and r['observed_catalog_compactions']>0
 assert parity['exit_code']==0 and parity['error'] is None and parity['comparison']['equal'] and len(parity['tables'])==15 and parity['comparison']['different_tables']==[]
 with tempfile.TemporaryDirectory(prefix='runtime-review-',dir=base) as tmp:
  temp=pathlib.Path(tmp)
  for binary in ['p8-runtime-statistics','p8-oracle']:
   shutil.copyfile(b/binary,temp/binary);(temp/binary).chmod(0o555);assert h(temp/binary)==h(b/binary)
  args=[str(temp/'p8-runtime-statistics'),'--plan',str(m/'plan.json'),'--raw',str(m/'raw.jsonl'),'--output',str(out/'statistics.json')]
  result=subprocess.run(args,capture_output=True,text=True,timeout=300);(out/'statistics.stdout').write_text(result.stdout);(out/'statistics.stderr').write_text(result.stderr);assert result.returncode==0
  assert h(out/'statistics.json')==h(m/'statistics.json')
  for name in ['project','fresh-full']:
   shutil.copytree(m/name,temp/name)
   for q in (temp/name).rglob('*'):
    if q.is_file():q.chmod(0o600)
  args=[str(temp/'p8-oracle'),'--left',str(temp/'project'),'--right',str(temp/'fresh-full'),'--output',str(out/'parity.json')]
  result=subprocess.run(args,capture_output=True,text=True,timeout=300);(out/'parity.stdout').write_text(result.stdout);(out/'parity.stderr').write_text(result.stderr);assert result.returncode==0,result.stderr
  repeated=json.loads((out/'parity.json').read_text());assert repeated['comparison']==parity['comparison'] and repeated['binary_digest']==parity['binary_digest'] and repeated['scope']==parity['scope'] and repeated['tables']==parity['tables']
 # Independent interval arithmetic, additional to the original replay implementation.
 ordered=sorted(ops,key=lambda x:x['id'])
 assert all(x['scheduled_ns']<=x['offered_ns']<=x['started_ns']<=x['call_started_ns']<=x['finished_ns'] for x in ordered)
 assert r['observed_work_ns']>=max(x['finished_ns'] for x in ops)-coverage['work_start_ns']
 def interval_peak(start,end):
  events=sorted([(x[start],1) for x in ops]+[(x[end],-1) for x in ops])
  active=peak=0
  for _,delta in events:active+=delta;peak=max(peak,active);assert active>=0
  assert active==0
  return peak
 queue_peak=interval_peak('offered_ns','started_ns')
 executor_peak=interval_peak('started_ns','finished_ns')
 call_peak=interval_peak('call_started_ns','finished_ns')
 assert executor_peak<=c and r['actual_concurrency']['maximum']<=call_peak<=c
 # Raw call_started/finished bracket bookkeeping around the live counter; this interval maximum may conservatively exceed the reported live RPC maximum.
 assert queue_peak<=plan['queue_capacity']+c
 rss_values=[x['server']['resident_bytes'] for x in resources];q=len(rss_values)//4
 upper_median=lambda vals:sorted(vals)[len(vals)//2]
 warm=upper_median(rss_values[q:2*q]);tail=upper_median(rss_values[3*q:]);allowed=warm+warm//4+32*1024*1024
 assert (warm,tail,allowed)==(r['rss']['warmed_median_bytes'],r['rss']['tail_median_bytes'],r['rss']['allowed_bytes'])
 endpoint=next(x['response'] for x in raw if x['kind']=='endpoint_status')
 assert endpoint['process_resources']['pid']==resources[0]['server']['pid']
 assert endpoint['query_execution']['cpu_in_flight']==endpoint['query_execution']['async_in_flight']==0 and endpoint['retrieval']['query_pins']==0
 assert endpoint['retrieval']['resolution_freshness']['complete'] is True
 for key in ['completed','rejected']:
  values=[x['query_execution'][key] for x in resources]+[endpoint['query_execution'][key]];assert values==sorted(values)
 # admitted counters are permit occupancy gauges (cc-search execution.rs), not cumulative totals.
 for lane in ['cpu','async']:
  assert all(0<=x['query_execution'][lane+'_admitted']<=x['query_execution'][lane+'_limit']+x['query_execution']['queue_limit'] for x in resources)
  assert endpoint['query_execution'][lane+'_admitted']==0
 intervals={'observed_work_seconds':r['observed_work_ns']/1e9,'first_to_last_scheduled_seconds':(ordered[-1]['scheduled_ns']-ordered[0]['scheduled_ns'])/1e9,'first_schedule_to_last_terminal_seconds':(max(x['finished_ns'] for x in ops)-ordered[0]['scheduled_ns'])/1e9,'coverage_clock_read_delta_ns':coverage_clock_read_delta,'maximum_waiting_for_executor_start':queue_peak,'maximum_executor_operations':executor_peak,'maximum_recorded_call_intervals':call_peak,'reported_live_workload_maximum':r['actual_concurrency']['maximum'],'observed_query_execution_maxima':{key:max(x['query_execution'][key] for x in resources) for key in ['cpu_admitted','cpu_in_flight','async_admitted','async_in_flight']},'endpoint_execution':endpoint['query_execution'],'endpoint_query_pins':endpoint['retrieval']['query_pins'],'max_queue_wait_ns':max(x['started_ns']-x['offered_ns'] for x in ops),'rss_tail_minus_warmed_bytes':tail-warm,'rss_rule_recomputed_independently':True,'scope':'observed occupancy and unchanged median growth rule; not a saturated queue or statistical leak-free proof'}
 if plan['profile']=='soak':
  assert ordered[0]['scheduled_ns']==coverage['work_start_ns']
  assert all(x['scheduled_ns']==ordered[0]['scheduled_ns']+x['id']*1_000_000_000 for x in ordered)
  assert intervals['first_to_last_scheduled_seconds']==3600 and intervals['first_schedule_to_last_terminal_seconds']>=3600
 cache_audit=audit_cache(runtime,m,plan,r,raw,raw_offsets) if plan['profile']=='soak' else None
 for x in [b,m]:owner.verify_output(x)
 assert zipfile.stat().st_size==size and h(zipfile)==sha, 'original ZIP changed during audit'
 summary={'head':plan['source']['source_commit'],'workflow_run':metadata['run_id'],'artifact_id':artifact_id,'zip_sha256':sha,'zip_bytes':size,'verdict':'accepted_scoped_original_'+head[:8]+'_runtime_observation','configured_concurrency':c,'actual_concurrency':r['actual_concurrency'],'offered':n,'outcomes':r['outcomes'],'terminal_ids_complete_unique':True,'source_inputs':1092,'observer_files':9,'sealed_artifact_counts':seals,'retained_build_metadata_matches_full_sealed_build':True,'product_sha256':r['product_sha256'],'oracle_sha256':r['oracle_sha256'],'statistics_sha256':plan['statistics_sha256'],'report_sha256':h(m/'report.json'),'statistics_independent_replay_byte_identical':True,'no_repair_oracle_replay_all15tables_identical':True,'observed_work_ns':r['observed_work_ns'],'profile':plan['profile'],'rss':r['rss'],'resource_time_coverage':r['resource_time_coverage'],'catalog_compactions':r['observed_catalog_compactions'],'branch_switches':r['real_branch_switches'],'latency_by_operation':r['latency_by_operation'],'soak_and_queue_independent_checks':intervals,'scope_limits':['fixed offered concurrency cap; actual overlap and maxima reported honestly','original executed HEAD preserved; not a later-head claim','descriptive empirical latency only; no performance improvement or stable p99 claim','default profile semantic backfill excluded; separate seeded original-worker run required']}
 summary.update(cache_audit=cache_audit,raw_event_counts=raw_event_counts,status='accepted_scoped',unresolved_blockers=[],owned_cleanup=cleanup,parity_exit_code=0,artifact_seal_status='sealed',helper=dict(path=str(pathlib.Path(__file__).resolve()),sha256=h(pathlib.Path(__file__))))
 (out/'independent-review.json').write_text(json.dumps(summary,indent=2,sort_keys=True)+'\n');all_reviews.append(summary);print(json.dumps({k:v for k,v in summary.items() if k!='latency_by_operation'}),flush=True)
(base/('runtime-'+head[:8]+'-independent-'+str(artifacts[0][1])+'.json')).write_text(json.dumps(all_reviews,indent=2,sort_keys=True)+'\n')

