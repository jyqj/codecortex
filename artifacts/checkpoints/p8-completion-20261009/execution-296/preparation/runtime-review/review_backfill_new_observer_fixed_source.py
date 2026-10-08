import pathlib,json,hashlib,sys,subprocess,math
base=pathlib.Path('/dev/shm/a217aaae3bde')
assert len(sys.argv)==5, 'manifest artifact-root exact-head exact-frozen-checkout are required'
repo=pathlib.Path(sys.argv[4]).resolve(strict=True)
manifest=json.loads(pathlib.Path(sys.argv[1]).read_text());head=sys.argv[3]
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head, 'frozen checkout HEAD differs'
assert len(head)==40 and all(x in '0123456789abcdef' for x in head)
artifact_id=manifest['artifact_id'];archive=pathlib.Path(sys.argv[2])/str(artifact_id)/(str(artifact_id)+'.zip')
assert archive.stat().st_size==manifest['zip_bytes'] and hashlib.sha256(archive.read_bytes()).hexdigest()==manifest['zip_sha256']
original_metadata=json.loads((archive.parent/'metadata.json').read_text());metadata={'artifact_id':original_metadata['id'],'source':original_metadata['workflow_run']['head_sha'],'run_id':original_metadata['workflow_run']['id'],'size_in_bytes':original_metadata['size_in_bytes'],'digest':original_metadata['digest']};assert metadata['artifact_id']==artifact_id and metadata['source']==head and metadata['size_in_bytes']==manifest['zip_bytes'] and metadata['digest']=='sha256:'+manifest['zip_sha256']
p=archive.parent/'extracted';out=base/'runtime-review'/('backfill-'+str(artifact_id)+'-'+head[:8]+'-independent');out.mkdir(exist_ok=False)
def blob(path):return subprocess.check_output(['git','show',head+':'+path],cwd=repo)
frozen_matches=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
modules=repo if frozen_matches else base/'runtime-review'/('observer-replay-'+head)
for name in ['scripts/p8_runtime.py','scripts/p8_runtime_build.py','scripts/p7_build_identity.py','scripts/p8_cold_build.py','scripts/p8_rollback.py','scripts/p7_stdio_build_receipt.py','scripts/p8_backfill.py','scripts/resource_harness/__init__.py','scripts/resource_harness/runtime.py']:
 path=modules/name;data=blob(name)
 if frozen_matches:assert path.is_file() and path.read_bytes()==data
 else:path.parent.mkdir(parents=True,exist_ok=True)
 if path.exists():assert path.read_bytes()==data
 else:path.write_bytes(data);path.chmod(0o444)
sys.dont_write_bytecode=True
sys.path.insert(0,str(modules/'scripts'));import p8_runtime_build as b
h=lambda x:hashlib.sha256(x.read_bytes()).hexdigest();g=blob
seal=b.verify_output(p);r=json.loads((p/'receipt.json').read_text());assert r['status']=='passed_observation' and r['exit_code']==r['build_exit_code']==r['execution_exit_code']==0
assert r['source_before']==r['source_after']==r['source_final'] and r['source_before']['source_commit']==head
assert set(r['source_before']['inputs'])==set(subprocess.check_output(['git','ls-tree','-r','--name-only',head,'--','Cargo.toml','Cargo.lock','crates'],cwd=repo,text=True).splitlines())
assert len(r['source_before']['inputs'])==1087 and all(hashlib.sha256(g(n)).hexdigest()==v for n,v in r['source_before']['inputs'].items())
assert r['observer_before']==r['observer_after']==r['observer_final'] and len(r['observer_before']['files'])==9
assert r['observer_before']['source_commit']==head, 'backfill observer manifest HEAD differs'
assert set(r['observer_before']['files'])==set(b.OBSERVER_FILES), 'backfill observer manifest inventory differs'
for name,v in r['observer_before']['files'].items():assert h(p/'observer-source'/name)==v['sha256']==hashlib.sha256(g(name)).hexdigest()
assert r['toolchain_before']==r['toolchain_after']==r['toolchain_final'] and r['target_initially_absent']
assert h(p/'p7_worker_contention')==r['executable_sha256']==r['executable_sha256_after']==r['copy_source']['sha256']
rows=[json.loads(l) for l in (p/'build.jsonl').open()];event=[x for x in rows if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')=='p7_worker_contention'];assert event==[r['cargo_artifact']]
a=event[0];assert a['features']==['semantic'] and a['fresh'] is False and a['profile']['test'] and a['profile']['opt_level']=='3' and a['profile']['debug_assertions'] is False
assert '--no-default-features' in r['build_command'] and r['build_command'][r['build_command'].index('--features')+1]=='semantic'
assert r['execution_command'][1:]==['--exact','real_slow_backfill_preserves_local_progress_and_records_every_request','--nocapture']
assert '1 passed; 0 failed; 0 ignored' in (p/'execution.stdout').read_text()
assert sorted(x.name for x in (p/'raw').iterdir())==['seed-19','seed-43','seed-7']
seeds=[];request_total=0
for seed in [7,19,43]:
 root=p/'raw'/('seed-'+str(seed));protocol=json.loads((root/'protocol.json').read_text());summary=json.loads((root/'summary.json').read_text());held=json.loads((root/'held-before.json').read_text())
 assert protocol['seed']==summary['seed']==seed and protocol['concurrency_cells']==[1,4,8,16] and protocol['samples_per_cell']==32 and protocol['phases']==['quiet','held']
 assert protocol['query_watchdog_ms']==2000 and protocol['progress_watchdog_ms']==5000 and protocol['writer_busy_timeout_ms']==100 and protocol['worker_local_attempt_width']==4
 assert held['provider']['waiting']==held['provider']['active']==held['queue']['claimed']==4 and held['queue']['pending']>0 and held['queue']['published']>0 and held['queue']['uncovered']>0
 assert len(held['provider']['held_inputs'])==4
 phase_reviews=[]
 for phase in ['quiet','held']:
  requests=json.loads((root/(phase+'-requests.json')).read_text());assert len(requests)==128;request_total+=len(requests)
  for c in [1,4,8,16]:
   cell=[x for x in requests if x['concurrency']==c];assert len(cell)==32 and sorted(x['ordinal'] for x in cell)==list(range(32))
   assert all(any(hit['file_path']=='stable.rs' for hit in x['hits']) for x in cell)
   assert all(x['caller_schedule_us']>=0 and x['capture_admission_us']>=0 and x['retrieval_us']>=0 and x['offered_to_api_return_us']>=x['caller_schedule_us']+x['capture_admission_us']+x['retrieval_us'] for x in cell)
   times=sorted(x['offered_to_api_return_us'] for x in cell);actual=[x for x in summary[phase] if x['concurrency']==c][0]
   assert actual['n']==32 and actual['p50_us']==times[math.ceil(.5*32)-1] and actual['p95_us']==times[math.ceil(.95*32)-1] and actual['p99_us']==times[math.ceil(.99*32)-1] and actual['max_us']==times[-1]
   assert times[-1]<2000000
   phase_reviews.append({'phase':phase,'concurrency':c,'requests':32,'unique_ordinals':32,'p50_us':actual['p50_us'],'p95_us':actual['p95_us'],'p99_us':actual['p99_us'],'max_us':actual['max_us']})
 assert summary['old_held_input_publications']==0 and summary['new_provider_calls']>0 and summary['old_provider']['active']==summary['old_provider']['waiting']==0
 assert summary['old_provider']['maximum_active']<=4 and summary['queue_final']['pending']==summary['queue_final']['claimed']==summary['queue_final']['uncovered']==0 and summary['queue_final']['published']>0
 db=summary['db_availability'];assert db['generation_before']==db['generation_after'] and db['generation_unchanged'] and db['writer_acquire_and_rollback_us']<100000 and db['read_rows']==26
 assert summary['resource_attribution_gate']=='attributed_combined_process'
 resources=[summary['quiet_resources'],held['resources'],summary['held_after_resources'],summary['final_resources']]
 usages=[x['shared_process_owner']['usage'] for x in resources];assert all(u is not None and u['peak_resident_bytes']>0 for u in usages)
 for key in ['user_cpu_ns','system_cpu_ns']:assert [u[key] for u in usages]==sorted(u[key] for u in usages)
 assert all(x['shared_process_owner']['components']==['test_runner','CodeIndex','fake_provider'] and x['server']['separate_pid'] is None and x['server_tree']['separate_server_or_provider_process'] is False for x in resources)
 seeds.append({'seed':seed,'cells':phase_reviews,'held_provider_active':held['provider']['active'],'held_claimed':held['queue']['claimed'],'held_pending':held['queue']['pending'],'old_held_input_publications':0,'new_provider_calls':summary['new_provider_calls'],'queue_final':summary['queue_final'],'db_availability':db,'shared_native_resource_stages':4,'initial_build_us':summary['initial_build_us'],'held_build_us':summary['held_build_us'],'write_delete_us':summary['write_delete_us'],'switch_and_drain_us':summary['switch_and_drain_us']})
assert request_total==768;b.verify_output(p)
review={'verdict':'accepted_scoped_original_'+head[:8]+'_backfill_observation','head':head,'workflow_run':metadata['run_id'],'artifact_id':artifact_id,'zip_sha256':manifest['zip_sha256'],'zip_bytes':manifest['zip_bytes'],'receipt_sha256':h(p/'receipt.json'),'sealed_files_verified':len(seal['artifact_inventory']),'fixed_source_inputs':1087,'fixed_source_verified_against_git_blobs':True,'fixed_observer_inputs':9,'actual_cargo_features':a['features'],'private_fresh_build':True,'actual_release_profile':a['profile'],'binary_sha256':r['executable_sha256'],'execution_elapsed_ns':r['execution_elapsed_ns'],'original_test_executed_passed':True,'request_total':request_total,'seeds':seeds,'unresolved_blockers':[],'scope_limits':['actual CodeIndex/post-index worker with synthetic in-process fake provider; no live provider or network throughput claim','existing 2s local/5s progress/100ms writer controls retained; no new SLA or performance improvement claim','resource CPU/RSS high-water belongs to one shared test runner/CodeIndex/provider process; no component sum or current RSS claim','independent audit of this exact executed head; historical observations preserved separately; no automatic TODO/release certification']}
review.update(helper=dict(path=str(pathlib.Path(__file__).resolve()),sha256=h(pathlib.Path(__file__))))
(out/'independent-review.json').write_text(json.dumps(review,indent=2,sort_keys=True)+'\n');print(json.dumps({k:v for k,v in review.items() if k!='seeds'},indent=2))

