import json,pathlib,hashlib,subprocess,os,time
root=pathlib.Path(os.environ.get('P7_REPOSITORY_ROOT','/workspace/codecortex'));source='715ab33e83ecb6c65228c18ed55e0fa6604ca0a6'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==source
cargo=pathlib.Path(os.environ.get('P7_CARGO_ARTIFACTS','/tmp/p7-v11-cargo-artifacts.jsonl'))
artifacts=[json.loads(line) for line in cargo.read_text().splitlines() if line.startswith('{')]
artifact=next(a for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')=='p7_v11_generation_public' and a.get('executable'))
binary=pathlib.Path(artifact['executable']);product=root/'target/debug/codecortex';test=root/'crates/cc-server/tests/p7_v11_generation_public.rs'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
corpus={'file':'needle.py','before':'def needle():\n    return 731\n','after':'def needle():\n    return 947\n','vectors':[1,0],'model':'synthetic-public-deadline','questions':['needle v18_live_generation_search','needle v18_live_generation_context'],'fence_mutations':'actual parsed incremental rebuilds at every attempt'}
corpus_digest=hashlib.sha256(json.dumps(corpus,sort_keys=True).encode()).hexdigest();run_id=f'p7-v11-{source[:7]}-{corpus_digest[:8]}-generation-v1-20261002'
out=pathlib.Path(os.environ.get('P7_REPLAY_OUTPUT',str(root/'artifacts/benchmarks'/run_id)));out.mkdir(exist_ok=False);(out/'raw').mkdir()
prod=json.loads((root/'artifacts/checkpoints/p7-query-public-independent-20261002/production-byte-equivalence.json').read_text());assert all(sha(root/p)==v for p,v in prod['module_sha256'].items())
manifest={'run_id':run_id,'source_sha':source,'production_sha':'c8c20b5b7d416372ee06ed5e248663c48912064e','production_fingerprints_unchanged':379,'test_source_sha256':sha(test),'test_binary_sha256':sha(binary),'product_binary_sha256':sha(product),'cargo_artifact':artifact,'build':'warm cached; not cold','corpus':corpus,'corpus_digest':corpus_digest,'scoring_digest':hashlib.sha256(b'functional generation/source fence, no retrieval quality scoring').hexdigest(),'levels':['L3: actual product subprocess MCP stdio with loopback HTTP','L2: production generation-fence entry with real parser/rebuild/SQLite'],'quality_holdout':False,'live_provider':False,'repetitions':5}
(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');(out/'queries.jsonl').write_text(''.join(json.dumps({'id':str(i),'query':q,'kind':'synthetic functional question'})+'\n' for i,q in enumerate(corpus['questions'])))
commands=[];latencies=[];normalized=[]
for i in range(1,6):
 raw=out/'raw'/f'{i:02}';raw.mkdir();env=os.environ.copy();env['P7_V11_EVIDENCE_DIR']=str(raw)
 start=time.monotonic();r=subprocess.run([str(binary),'--nocapture'],cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT);duration=time.monotonic()-start
 log=pathlib.Path(f'/tmp/p7-v11-replay-{i:02}.log');log.write_bytes(r.stdout);assert r.returncode==0,r.stdout[-1500:];assert b'2 passed; 0 failed; 0 ignored' in r.stdout
 receipts=[json.loads(p.read_text()) for p in raw.glob('*.json')];assert len(receipts)==4
 for receipt in receipts:
  if receipt['test']=='real_rebuild_fence':assert receipt['exhausted_attempts']==3 and receipt['recovered_attempts']==2
  else:
   assert receipt['query_calls']==1
   conflicts=[c for c in receipt['calls'] if c['kind']=='generation_conflict'];assert len(conflicts)==1;assert conflicts[0]['wire']['data']['retryable'] is True
  normalized.append({'round':i,**receipt})
 commands.append({'round':i,'argv':[str(binary),'--nocapture'],'exit':r.returncode,'local_log':str(log),'log_sha256':sha(log),'raw_files':{p.name:sha(p) for p in raw.glob('*.json')}})
 latencies.append({'round':i,'entire_runner_seconds':duration,'scope':'functional runner duration, not query SLA or performance sample'})
(out/'commands.json').write_text(json.dumps(commands,indent=2)+'\n');(out/'normalized.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in normalized));(out/'latency.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in latencies));(out/'resources.jsonl').write_text(json.dumps({'read_pool_connections':1,'memory_sampling':'not_measured','scope':'read pool configuration plus held-network rebuild/status progress assertions; not full resource gate'})+'\n');(out/'failures.jsonl').write_text('');(out/'comparison.json').write_text(json.dumps({'baseline':'held pre-rebuild generation vs actual committed rebuild generation','metric':'hard functional conflict and returned source bytes','quality':None,'independent_quality_queries':0},indent=2)+'\n')
(out/'metrics.json').write_text(json.dumps({'runs':5,'passed':10,'failed':0,'ignored':0,'L3_search_context_cases':10,'L2_churn_success_error_cases':10,'full_V11':'pending','full_V05':'pending','P7_014':'in_progress'},indent=2)+'\n');(out/'gate.json').write_text(json.dumps({'status':'passed_declared_V11_L2_L3_subset','full_gate_closed':False,'mixed_generation_returned':0,'bounded_retries':'3 on churn,2 on recovery,1 on control/stable error','cache_key_complete':'not_claimed'},indent=2)+'\n')
(out/'report.md').write_text(f'''# V11 production generation contract — {run_id}

Exact test source `{source}`; production remains c8c20b5, all379 reviewed production/Cargo fingerprints unchanged. Five warm-build executions:10 passed,0 failed,0 ignored. Actual built product binary and test binary hashes are in manifest. No live-model, cold-build, quality holdout or full gate claim.

L3 search and context each warm a local envelope containing731, hold cold query HTTP, perform a real parsed rebuild to947 and status through the product's public stdio with a one-connection read pool, and only then release a successful HTTP response. The pending RPC returns exact -32603 retryable=true generation-conflict after1 attempt and no envelope. Stable same-query semantic and local calls return947, never731; the semantic vector remains valid across document generations without another POST. These are client requests, not internal retry attempts.

L2 calls the production generation fence directly while real rebuilds change every attempt, with successful and failed work: exact3-attempt exhaustion; then a one-change window accepts only the second attempt's generation. Cancellation, deadline and stable corruption each execute once. This is an explicit production-fence entry test, not a claim of forcing three whole public requests or three provider attempts.

Compiler/test raw logs stay local by filename/hash. Two early preflight assertions incorrectly expected the word generation rather than the documented index-changed message, and source text in spans rather than machine_pack hits. They were corrected to exact public contracts without production edits; their log hashes are retained in integration receipt. Full V11 cache axes and formal row consolidation, V05 all lanes/DSL/BM25 at required levels remain pending. V16 memory/review belong to independent owners. D1/D2/live V19 restrictions remain.

Replay: checkout exact source, build this target with semantic-http --locked --offline --no-run --message-format=json; run replay.py with P7_REPOSITORY_ROOT, P7_CARGO_ARTIFACTS and a fresh P7_REPLAY_OUTPUT. The command needs synthetic loopback only.
''')
pathlib.Path('/tmp/p7-v11-run-path').write_text(str(out));print(json.dumps({'run_id':run_id,'passed':10,'failed':0,'ignored':0}))
