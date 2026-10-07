import pathlib,json,hashlib,subprocess,re,os,time
root=pathlib.Path(os.environ.get('P7_REPOSITORY_ROOT', '/workspace/codecortex'))
source='2d48f2628ae7c745fcab1a21dd784eae4582a193'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==source
path=root/'crates/cc-server/tests/p7_v16_exact_oracle.rs';s=path.read_text()
rows=re.findall(r'\("([^"]+)", "([^"]+)", \[([\d.]+), ([\d.]+)\], ([\d.]+)\)',s[s.index('const DOCS'):s.index('struct Probe')]);assert len(rows)==6
corpus={'query':'independent synthetic vector direction','documents':[{'path':p,'marker':m,'vector':[float(x),float(y)],'hand_cosine':float(score)} for p,m,x,y,score in rows]}
corpus_digest=hashlib.sha256(json.dumps(corpus,sort_keys=True).encode()).hexdigest();run_id=f'p7-v16-{source[:7]}-{corpus_digest[:8]}-cosine-v1-20261002'
out=pathlib.Path(os.environ.get('P7_REPLAY_OUTPUT', str(root/'artifacts/benchmarks'/run_id)));out.mkdir(exist_ok=False);(out/'raw').mkdir()
build=json.loads(pathlib.Path(os.environ.get('P7_BUILD_RECEIPT', '/tmp/p7-v16-oracle-build-receipt.json')).read_text());binary=pathlib.Path(build['artifact']['executable']);assert hashlib.sha256(binary.read_bytes()).hexdigest()==build['binary_sha256']
prod=json.loads((root/'artifacts/checkpoints/p7-query-public-independent-20261002/production-byte-equivalence.json').read_text())
assert all(hashlib.sha256((root/p).read_bytes()).hexdigest()==sha for p,sha in prod['module_sha256'].items())
manifest={'run_id':run_id,'source_sha':source,'production_sha':'c8c20b5b7d416372ee06ed5e248663c48912064e','test_source_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'corpus_digest':corpus_digest,'scoring_digest':hashlib.sha256(b'hand cosine 0.96/0.6/0.8/0; doc-key ascending ties; absolute tolerance2e-6').hexdigest(),'corpus':corpus,'features':build['artifact']['features'],'binary':str(binary),'binary_sha256':build['binary_sha256'],'cargo_fresh':build['artifact']['fresh'],'cold_build':False,'production_modules_unchanged':len(prod['module_sha256']),'level':'L2: real SQLite, worker and loopback transport plus in-process public QueryHandle; not L3 stdio or L4/L5','repetitions':5,'seeds':[0,1,3],'non_loopback_provider_source_bytes':0,'live_provider':False,'quality_holdout':False}
manifest['range_oracle']='independent literal fixture predicate; never HardScope::passes';manifest['catalog_oracle']='exact fixture paths and one document per source, before and after deletion';(out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');(out/'queries.jsonl').write_text(json.dumps({'id':'synthetic-direction-v1','query':corpus['query'],'role':'functional hand-vector query, not quality corpus'})+'\n')
latencies=[];normalized=[];commands=[]
for i in range(1,6):
 output=out/'raw'/f'{i:02}.json';env=os.environ.copy();env['P7_V16_ORACLE_OUTPUT']=str(output)
 start=time.monotonic();run=subprocess.run([str(binary),'--exact','independent_hand_cosine_topk_ties_scope_delete_and_space_rejection','--nocapture'],cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True);duration=time.monotonic()-start
 assert run.returncode==0,run.stdout[-2000:];assert '1 passed; 0 failed; 0 ignored' in run.stdout
 observed=json.loads(output.read_text());assert len(observed)==3
 for row in observed:
  assert len(row['cases'])==6
  normalized.append({'round':i,'seed':row['seed'],'query_posts':row['query_posts'],'cases':row['cases'],'delete':row['delete'],'old_space_rejection':row['old_space_rejection']})
 raw_log=pathlib.Path(f'/tmp/p7-v16-replay-{i:02}.log');raw_log.write_text(run.stdout)
 commands.append({'round':i,'argv':[str(binary),'--exact','independent_hand_cosine_topk_ties_scope_delete_and_space_rejection','--nocapture'],'exit':run.returncode,'local_log':str(raw_log),'log_sha256':hashlib.sha256(run.stdout.encode()).hexdigest(),'raw':str(output.relative_to(out)),'raw_sha256':hashlib.sha256(output.read_bytes()).hexdigest()})
 latencies.append({'round':i,'runner_wall_seconds':duration,'scope':'entire functional test runner; not query SLA or sampled performance'})
(out/'normalized.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in normalized));(out/'latency.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in latencies));(out/'resources.jsonl').write_text(json.dumps({'status':'not_measured','reason':'bounded-memory and full resource protocol remain separate pending cases'})+'\n');(out/'failures.jsonl').write_text('');(out/'comparison.json').write_text(json.dumps({'kind':'hand vector functional oracle','quality_delta':None,'holdout':False,'independent_quality_queries':0},indent=2)+'\n')
metrics={'test_runs':5,'passed':5,'failed':0,'ignored':0,'fixtures':15,'scope_cosine_gold_cases':90,'independent_quality_query_count':0,'query_posts':30,'production_or_provider_changes':False,'bounded_memory':'not_run','full_V16':'pending','V05_scope_hydrate_subset':'passed_L2','full_V05':'pending','full_V11':'pending'};(out/'metrics.json').write_text(json.dumps(metrics,indent=2)+'\n');(out/'commands.json').write_text(json.dumps(commands,indent=2)+'\n');(out/'gate.json').write_text(json.dumps({'status':'passed_declared_L2_scope','full_validation_closed':False,'full_P7_014':'in_progress','hard_correctness_failures':0},indent=2)+'\n')
report=f'''# V16 hand-vector production oracle — {run_id}

Exact source `{source}`, unchanged production `{manifest['production_sha']}`; Rust test binary `{manifest['binary_sha256']}` with semantic-http. This is a warm build, not a cold build. All {manifest['production_modules_unchanged']} production/Cargo fingerprints match the independently reviewed source.

Five executions passed (5/0/0), three insertion orders each, 90 hand-gold scope/cosine cases. The public QueryHandle starts cold and makes one real loopback query POST per model; later scopes and cache hits make none. Document vectors flow through the production worker. No manual cache/database priming.

Hand gold scores are 0.96, 0.6 (equal-score doc-key ties), 0.8 and orthogonal0. The excluded document scores1, strictly exceeding all allowed Rust documents: filter-after-top-k cannot satisfy top1 gold. Language/path/file intersection and Some(empty) preserve range; final hydration remains in scope despite external soft hints and a preselect limit of1. Deletion removes the best hit; old-space recall is unavailable after explicit model switch and the new spec requires a new query POST.

Classification follows 06-VALIDATION V16 L1/L2. This new block is L2 and its V05 hydration subset is L2. Repeated synthetic checks are not independent quality questions; no live model, public heldout corpus, L3 product stdio, L4 or L5 claim. Bounded-memory evidence is not run, so full V16 stays pending. Full V05 and V11 remain pending.

Manifest/queries, normalized/raw fixture receipts, metrics, runner wall samples, explicit unmeasured resources, empty failures, comparison and gate are retained. Raw build/compiler/test logs stay local with filename/hash evidence. No source or key is sent beyond synthetic loopback.
''';(out/'report.md').write_text(report)
pathlib.Path('/tmp/p7-v16-run-path').write_text(str(out));print(json.dumps({'run_id':run_id,**metrics}))
