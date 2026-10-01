import pathlib,json,subprocess,shutil,hashlib
root=pathlib.Path.cwd();out=root/'artifacts/benchmarks/p5e-formal-runs-20261001-v1';r=out/'offline-replay';h=root/'artifacts/benchmarks/p5e-harness-20261001/final-v5';driver=h/'binaries/p5e-evidence-driver';metrics=root/'artifacts/benchmarks/p5e-metrics-runner-sourcev2-20261001/binaries/cc-eval';receipts=[]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def run(kind,cmd,log,expected):
 with log.open('w') as stream:rc=subprocess.call([str(x) for x in cmd],stdout=stream,stderr=subprocess.STDOUT)
 receipts.append({'kind':kind,'argv':[str(x) for x in cmd],'exit_code':rc,'expected_exit':expected,'consistent':rc==expected,'log_sha256':sha(log)});print(kind,rc,flush=True)
for folder in sorted((out/'original51-baseline').glob('suite-*'))+sorted(p.parent for p in (out/'original51-factorial').glob('*/manifest.json')):
 name=folder.parent.name+'-'+folder.name;dest=r/'native-copies'/name;dest.parent.mkdir(exist_ok=True);assert not dest.exists();shutil.copytree(folder,dest)
 originals={p.relative_to(folder).as_posix():sha(p) for p in folder.rglob('*') if p.is_file()};gate=json.load(open(folder/'gate.json'))
 run('native-'+name,[metrics,'replay','--run',dest],r/(name+'.log'),gate['exit_code'])
 # Replay CLI rewrites reports, so run only on copied snapshots, never original evidence.
 differences=[name for name,digest in originals.items() if sha(dest/name)!=digest];receipts[-1]['recomputed_file_differences']=differences;receipts[-1]['consistent'] &= not differences
for cell in range(8):
 label=f'cell_{cell:03b}';run('facet-'+label,[driver,'replay-facets',h/'inputs',out/'facets'/label,r/(label+'-facet.json')],r/(label+'-facet.log'),0)
for label in ['baseline','candidate']:
 for mode,plan in [('typedgraph','graph-plan.json'),('fanout','fanout-plan.json')]:
  name=mode+'-'+label;run(name,[driver,'replay-graph',h/'inputs'/plan,out/name,r/(name+'.json')],r/(name+'.log'),0)
 for c in [1,4,8,16]:
  name=f'mixed-c{c}-{label}';run(name,[driver,'replay-mixed',h/f'inputs/mixed-c{c}.json',out/name,r/(name+'.json')],r/(name+'.log'),0)
 for name in ['fanout-'+label]+[f'mixed-c{c}-{label}' for c in [1,4,8,16]]:
  run('resource-'+name,['python3',h/'inputs/validate_resources.py','--samples',out/name/'resource-samples.json','--output',r/(name+'-resource.json')],r/(name+'-resource.log'),0)
(r/'REPLAY-RECEIPT.json').write_text(json.dumps({'status':'all_replays_and_resources_consistent' if all(x['consistent'] for x in receipts) else 'replay_or_resource_inconsistency','receipts':receipts,'limits':['native replay oncopies prevents reportoverwrite;strictgate red preserved','consistency not independent source/110span proof or G5 acceptance']},indent=2)+'\n')
