import hashlib,json,pathlib,subprocess,tempfile
O=pathlib.Path(__file__).resolve().parent;R=O.parents[2];M=R/'crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json';Q=R/'crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl';B=R/'target/debug/cc-eval'
before=json.loads((O/'dev-old-manifest.json').read_text());after=json.loads(M.read_text());old=[json.loads(x) for x in (O/'dev-old-queries.jsonl').read_text().splitlines()];new=[json.loads(x) for x in Q.read_text().splitlines()]
assert len(old)==len(new)==14
for a,b in zip(old,new):
 assert {k:v for k,v in a.items() if k!='annotations'}=={k:v for k,v in b.items() if k!='annotations'}
 if a['id'] not in ['R05','R06']:assert a==b
 for p,h in b['annotations']['source_digests_sha256'].items():assert hashlib.sha256((R/'crates'/p).read_bytes()).hexdigest()==h
for k in before.keys()-{'source','queries_digest'}:assert before[k]==after[k]
assert {k:v for k,v in before['source'].items() if k!='digest'}=={k:v for k,v in after['source'].items() if k!='digest'}
for p in after['source']['files']:
 if p!='cc-db/src/index_migrate.rs':assert (R/'crates'/p).read_bytes()==subprocess.check_output(['git','show',f'5ffbadcf:crates/{p}'],cwd=R)
print('PASS exact 14 queries/answers/scoring/config preserved; only R05/R06 annotation changes; all other admitted source unchanged')
with tempfile.TemporaryDirectory(dir='/workspace/scratch') as td:
 t=pathlib.Path(td)
 for p in after['source']['files']:
  f=t/'source'/p;f.parent.mkdir(parents=True,exist_ok=True);f.write_bytes((R/'crates'/p).read_bytes())
 q=t/'queries.jsonl';q.write_bytes(Q.read_bytes());m=json.loads(M.read_text());m['source']['root']='source';m['queries']='queries.jsonl';suite=t/'suite.json';suite.write_text(json.dumps(m))
 def check(exit,label):
  x=subprocess.run([str(B),'validate','--suite',str(suite)],capture_output=True,text=True);assert x.returncode==exit,x.stdout+x.stderr
  if exit:assert 'source or query content lock drift' in x.stderr
  print(f'PASS {label}: exit={exit}')
 check(0,'unchanged isolated lock')
 f=t/'source/cc-db/src/index_migrate.rs';content=f.read_bytes();f.write_bytes(content+b'\n// negative control\n');check(2,'undeclared source drift');f.write_bytes(content);q.write_bytes(q.read_bytes()+b'\n');check(2,'undeclared query drift')
print('PASS bounded lock negatives; no retrieval/heldout execution')
