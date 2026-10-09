"""Independent portable review of one exact-source actual lifecycle artifact.

Usage: python review_e_lifecycle_original.py manifest.json artifact-root 40hexHEAD exact-checkout review-output-root
The manifest supplies artifact_id, zip_bytes and zip_sha256 verified from GitHub.
Original extracted files are read-only; only source_root in a DERIVATIVE replay
input is relocated to the retained fixtures. This never rebuilds the product.
"""
import collections, copy, hashlib, json, pathlib, shutil, subprocess, sys, tempfile
from e_raw_cache_audit import HEAD,LIFECYCLE_RUN,PRODUCT_MANIFEST,file_hash
from lifecycle_cargo_audit import verify_original_cargo
assert len(sys.argv)==6, 'manifest artifact-root exact-head exact-checkout review-output-root required'
base=pathlib.Path(sys.argv[5]).resolve(strict=True);repo=pathlib.Path(sys.argv[4]).resolve(strict=True);head=sys.argv[3]
assert head==HEAD, 'helper frozen for exact fixed C3ff execution only'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head, 'frozen checkout HEAD differs'
assert len(head)==40 and all(x in '0123456789abcdef' for x in head)
manifest=json.loads(pathlib.Path(sys.argv[1]).read_text());aid=manifest['artifact_id']
archive=pathlib.Path(sys.argv[2])/str(aid)/(str(aid)+'.zip')
h=file_hash
assert archive.stat().st_size==manifest['zip_bytes'] and h(archive)==manifest['zip_sha256']
original_metadata=json.loads((archive.parent/'artifact-metadata.json').read_text());metadata={'artifact_id':original_metadata['id'],'source':original_metadata['workflow_run']['head_sha'],'run_id':original_metadata['workflow_run']['id'],'size_in_bytes':original_metadata['size_in_bytes'],'digest':original_metadata['digest']};assert metadata['artifact_id']==aid and metadata['source']==head and metadata['size_in_bytes']==manifest['zip_bytes'] and metadata['digest']=='sha256:'+manifest['zip_sha256'] and metadata['run_id']==LIFECYCLE_RUN
root=archive.parent/'extracted';m=root/'measurement';out=base/('lifecycle-'+str(aid)+'-'+head[:8]+'-independent');out.mkdir(exist_ok=False)
def blob(name):return subprocess.check_output(['git','show',head+':'+name],cwd=repo)
frozen_matches=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==head
modules=repo if frozen_matches else base/'runtime-review'/('observer-replay-'+head)
for name in ['scripts/p8_lifecycle.py','scripts/p8_resources.py','scripts/p8_rollback.py','scripts/p8_cold_build.py','scripts/p7_build_identity.py','scripts/resource_harness/__init__.py','scripts/resource_harness/runtime.py']:
 p=modules/name;data=blob(name)
 if frozen_matches:assert p.is_file() and p.read_bytes()==data
 else:p.parent.mkdir(parents=True,exist_ok=True)
 if p.exists():assert p.read_bytes()==data
 else:p.write_bytes(data);p.chmod(0o444)
sys.dont_write_bytecode=True;sys.path.insert(0,str(modules/'scripts'))
import p8_lifecycle as life
r=json.loads((m/'receipt.json').read_text());plan=r['plan'];life.verify_artifact_inventory(m,r)
assert r['status']=='complete_observation' and r['exit_code']==0 and r['failures']==[]
assert plan['profile']=='release' and plan['cold_samples']==30 and plan['query_samples']==400 and plan['files']==32
assert r['sample_counts']=={'expected':1230,'recorded':1230}
assert r['product']['source']['source_commit']==head
assert r['product']['source']['source_tree']==subprocess.check_output(['git','rev-parse',head+'^{tree}'],cwd=repo,text=True).strip()
assert r['product']['source']['input_count']==1092 and r['product']['source']['manifest_sha256']==PRODUCT_MANIFEST
product=json.loads((root/'product/build-receipt.json').read_text());inputs=json.loads((root/'product/source-inputs.json').read_text())
assert product['source_before']==product['source_after']==r['product']['source']
assert h(root/'product/build-receipt.json')==r['product']['receipt_sha256']
assert h(root/'product/source-inputs.json')==r['product']['source']['manifest_sha256']
assert len(inputs)==1092 and set(inputs)==set(subprocess.check_output(['git','ls-tree','-r','--name-only',head,'--','Cargo.toml','Cargo.lock','crates'],cwd=repo,text=True).splitlines())
assert all(hashlib.sha256(blob(n)).hexdigest()==v for n,v in inputs.items())
assert h(root/'product/codecortex')==r['product']['binary_sha256']==product['binary_sha256']
assert product['build_exit_code']==0 and product['build_profile']=='release' and product['cargo_target_initially_absent']
assert product['cargo_artifact']['fresh'] is False and product['cargo_artifact']['features']==[]
assert product['cargo_artifact']['profile']['opt_level']=='3' and product['cargo_artifact']['profile']['test'] is False
assert product['builder_sha256']==hashlib.sha256(blob('scripts/p7_stdio_build_receipt.py')).hexdigest()
assert product['identity_helper_sha256']==hashlib.sha256(blob('scripts/p7_build_identity.py')).hexdigest()
for name,expected in r['observer_sha256'].items():
 relative='scripts/'+name.split('/scripts/',1)[1]
 assert hashlib.sha256(blob(relative)).hexdigest()==expected
assert len(r['observer_sha256'])==7
assert {name.split('/scripts/',1)[1] for name in r['observer_sha256']}==set(life.OBSERVER_FILES), 'lifecycle observer inventory differs'
replay=json.loads((root/'replay-build-receipt.json').read_text());assert h(root/'p8-measurements')==replay['binary_sha256']==r['evaluator_sha256']
assert replay['source_before']==replay['source_after']==r['product']['source']
assert replay['build_exit_code']==0 and replay['artifact']['fresh'] is False
assert replay['artifact']['profile']['opt_level']=='3' and replay['artifact']['profile']['test'] is False
assert replay['artifact']['features'] in ([],['default'])
producer=pathlib.Path(product['cargo_artifact']['manifest_path']).parents[2]
assert producer.is_absolute() and '..' not in producer.parts
assert product['cargo_artifact']['manifest_path']==str(producer/'crates/cc-server/Cargo.toml') and product['cargo_artifact']['target']['src_path']==str(producer/'crates/cc-server/src/main.rs')
assert replay['artifact']['manifest_path']==str(producer/'crates/cc-eval/Cargo.toml') and replay['artifact']['target']['src_path']==str(producer/'crates/cc-eval/src/bin/p8-measurements.rs')
command=replay['build_command'];target=pathlib.Path(command[command.index('--target-dir')+1])
assert target.is_absolute() and '..' not in target.parts and '--locked' in command and '--release' in command
assert replay['artifact']['executable']==str(target/'release/p8-measurements')==replay['copy_source']['path']
assert replay['copy_source']['sha256']==r['evaluator_sha256']
for field in ['build_stdout','build_stderr']:assert h(root/replay[field]['name'])==replay[field]['sha256']
messages=[json.loads(x) for x in (root/'replay-build.jsonl').read_text().splitlines()]
assert messages[-1]=={'reason':'build-finished','success':True}
assert [x for x in messages if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')=='p8-measurements']==[replay['artifact']]
product_messages=[json.loads(line) for line in (root/'product/cargo-build.jsonl').open()]
cargo_review=verify_original_cargo(product,replay,product_messages,messages)
assert set(r['observer_sha256'])=={str(producer/'scripts'/name) for name in life.OBSERVER_FILES}, 'lifecycle observer producer differs'
cargo_review.update(product_log_sha256=h(root/'product/cargo-build.jsonl'),evaluator_log_sha256=h(root/'replay-build.jsonl'))
inp=json.loads((m/'replay-input.json').read_text());report=json.loads((m/'report.json').read_text())
assert h(m/'report.json')==h(m/'report-replayed.json')==r['replay']['report_sha256']
assert h(m/'replay-input.json')==r['replay']['input_sha256']==report['input_sha256']
assert report['exit_code']==0 and report['measurement_status']=='complete_observation' and report['source_verified_queries']==1200
assert len(inp['samples'])==inp['expected_samples']==1230 and all(x['status']=='success' for x in inp['samples'])
assert len(inp['raw_queries'])==1200
layers=report['latency_layers'];assert layers['missing_samples']==layers['unexpected_samples']==0 and layers['recorded_samples']==layers['expected_samples']==1230
expected_counts={'cold_build':30,'process_reopen':400,'warm_uncached':400,'cache_hit':400,'unknown':0}
assert {x['stratum']:x['samples'] for x in layers['layers']}==expected_counts
assert all(x['completed']==x['samples'] and x['missing_timings']==x['errors']==x['timeouts']==x['partial']==x['cancelled']==0 for x in layers['layers'])
assert r['os_page_cache']=='not_cleared_unknown_not_cold_disk'
assert r['provider_cost']['reported'] is None and r['provider_cost']['estimated'] is None
assert all(r['provider_cost'][k] is None for k in ['input_tokens','output_tokens','requests_billed'])
assert inp['costs']==[] and report['cost_ledger']['status']=='unavailable'
events=[json.loads(x) for x in (m/'events.jsonl').read_text().splitlines()]
bykind=collections.defaultdict(list)
for e in events:bykind[e['kind']].append(e)
started={x['session']:x for x in bykind['session_started']};closed={x['session']:x for x in bykind['session_closed']}
assert len(started)==len(closed)==431==r['session_count'] and set(started)==set(closed)
for name,e in closed.items():
 p=e['process'];assert p['pid']==started[name]['pid'] and p['exit_code']==p['expected_exit_code']==0 and p['cleanup']=='completed' and p['binary_sha256']==r['product']['binary_sha256'] and p['tool_count']==14
assert len(bykind['pristine_fixture'])==len(bykind['cold_build'])==len(bykind['final_closed_fixture_storage'])==30
assert all(x['index_and_persistent_parse_cache_absent'] for x in bykind['pristine_fixture'])
assert len(bykind['reopen_existing_index'])==400
assert all(x['build_calls_in_process']==0 and x['source_manifest_unchanged'] and x['before_launch_db']['indexed_files']==32 for x in bykind['reopen_existing_index'])
queries=bykind['query_attempt'];assert collections.Counter(x['phase'] for x in queries)=={'process_reopen':400,'warm_uncached':400,'cache_hit':400,'warmup':1}
for e in queries:
 assert e['error'] is None and e['pid']==started[e['session']]['pid']
 cache,control=life.cache_observation(e['before'],e['after'])
 assert control==e['cache_control'] and cache==('hit' if e['phase']=='cache_hit' else 'miss')
 assert e['elapsed_us']==(e['finished_ns']-e['started_ns'])//1000
 if e['sample_index'] is not None:
  s=inp['samples'][e['sample_index']];assert s['elapsed_us']==e['elapsed_us'] and s['evidence']['result_cache']==cache
 for status in [e['before'],e['after']]:
  d=status['diagnostics'];assert d['auto_index_enabled'] is False and d['semantic_configured'] is False and d['retrieval']['semantic_state']=='not_configured' and d['retrieval']['dense_state']=='disabled'
resources=bykind['resource'];assert len(resources)==len(inp['resources'])==1261
assert [e['ledger'] for e in resources]==inp['resources']
for e in resources:
 native=e['server_native'];ledger=e['ledger'];assert native['pid']==started[e['session']]['pid']==ledger['server_pid'] and ledger['server_pid']!=ledger['runner_pid']
 assert ledger['server_rss_bytes']==native['resident_bytes'] and ledger['server_rss_bytes']>0
 tree=e['owned_tree'];assert ledger['server_tree_rss_bytes']==tree['tree']['resident_bytes']
# Memory role totals remain deliberately unavailable; sampled peaks are not summed.
assert report['memory_ledger']['total_rss_bytes'] is None and report['memory_ledger']['sampling_interval_ms'] is None
assert len(r['storage'])==30 and inp['disk_layout_complete'] is True
parts={}
for x in r['storage']:
 assert x['database']['integrity']=='ok' and x['database']['foreign_key_errors']==0 and x['database']['indexed_files']==32
 for row in x['objects']:
  identifier=row['storage_id'];value={k:row[k] for k in ['storage_id','component','bytes']}
  assert identifier not in parts or parts[identifier]==value
  parts[identifier]=value
assert sorted(parts.values(),key=lambda x:x['storage_id'])==report['disk_ledger']['partitions']
assert sum(x['bytes'] for x in parts.values())==report['disk_ledger']['measured_subtotal_bytes']
# Relocate only retained fixture roots in a separate replay input; do not touch originals.
old_input=pathlib.Path(r['replay']['command'][r['replay']['command'].index('--input')+1]);old_measurement=old_input.parent
relocated=copy.deepcopy(inp)
for q in relocated['raw_queries']:
 relative=pathlib.Path(q['source_root']).relative_to(old_measurement)
 q['source_root']=str(m/relative)
relocated_path=out/'relocated-replay-input.json';relocated_path.write_text(json.dumps(relocated,sort_keys=True,indent=2)+'\n')
with tempfile.TemporaryDirectory(prefix='lifecycle-replay-',dir=base) as tmp:
 binary=pathlib.Path(tmp)/'p8-measurements';shutil.copyfile(root/'p8-measurements',binary);binary.chmod(0o555);assert h(binary)==r['evaluator_sha256']
 result=subprocess.run([str(binary),'--input',str(relocated_path),'--output',str(out/'relocated-report.json')],capture_output=True,text=True,timeout=120)
 (out/'replay.stdout').write_text(result.stdout);(out/'replay.stderr').write_text(result.stderr);assert result.returncode==0,result.stderr
repeated=json.loads((out/'relocated-report.json').read_text());original=copy.deepcopy(report)
assert repeated.pop('input_sha256')==h(relocated_path);original.pop('input_sha256');assert repeated==original
life.verify_artifact_inventory(m,r)
assert archive.stat().st_size==manifest['zip_bytes'] and h(archive)==manifest['zip_sha256'], 'original ZIP changed during review'
review={'status':'accepted_scoped_actual_lifecycle_observation','head':head,'workflow_run':metadata['run_id'],'artifact_id':aid,'zip_sha256':manifest['zip_sha256'],'zip_bytes':manifest['zip_bytes'],'product_sha256':r['product']['binary_sha256'],'evaluator_sha256':r['evaluator_sha256'],'source_inputs_verified_against_fixed_git':len(inputs),'observer_inputs_verified_against_fixed_git':7,'sealed_files_verified':len(r['artifact_inventory']),'sample_counts':r['sample_counts'],'strata':layers['layers'],'source_verified_queries':1200,'closed_sessions':431,'owned_resource_samples':len(resources),'memory_ledger':report['memory_ledger'],'closed_storage_fixtures':30,'disk_subtotal_bytes':report['disk_ledger']['measured_subtotal_bytes'],'cost_scope':r['provider_cost'],'original_replay_byte_identical':True,'independent_relocated_replay':{'exit_code':0,'change':'only raw_queries.source_root in derivative input; original artifacts unmodified','report_identical_except_input_hash':True},'receipt_sha256':h(m/'receipt.json'),'unresolved_blockers':[],'scope_limits':[r['os_page_cache'],r['resource_scope'],'stage samples not continuous peak; never sum overlapping role peaks','shared SQLite physical bytes counted once; logical FTS already within that file','empirical distributions and original IID interval assumptions; no automatic performance improvement, task or release certification; original execution HEAD preserved']}
review.update(cargo_selection=cargo_review,helper={'path':str(pathlib.Path(__file__).resolve()),'sha256':h(pathlib.Path(__file__))})
(out/'independent-review.json').write_text(json.dumps(review,sort_keys=True,indent=2)+'\n')
print(json.dumps({k:v for k,v in review.items() if k not in ['memory_ledger','strata']},indent=2))
