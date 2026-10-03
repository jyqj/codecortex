import pathlib,json,subprocess,hashlib,datetime
root=pathlib.Path('/workspace/codecortex');out=pathlib.Path('/tmp/v19-current-6f3-local-20261003');out.mkdir(exist_ok=False)
product=json.load(open('/tmp/v19-current-6f3-build/build-receipt.json'))
artifacts=[]
for line in open('/tmp/v19-current-eval-build.jsonl'):
 x=json.loads(line)
 if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')=='cc-eval' and x.get('executable'):artifacts.append(x)
assert len(artifacts)==1
artifact=artifacts[0];ev=artifact['executable'];commands=[]
assert subprocess.check_output(['git','status','--porcelain'],cwd=root)==b''
identity={'run_id':out.name,'source_sha':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'source_tree':subprocess.check_output(['git','rev-parse','HEAD^{tree}'],cwd=root,text=True).strip(),'started_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'product_build_receipt':product,'eval_cargo_artifact':artifact,'eval_binary_sha256':hashlib.sha256(pathlib.Path(ev).read_bytes()).hexdigest(),'git_clean_before':True,'new_run':True,'old306_raw_reused':False,'paid_or_live_provider':False,'holdout_read':False,'scorer_or_gold_changed':False}
(out/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
def call(label,args):
 p=subprocess.run(args,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 (out/(label+'.stdout')).write_bytes(p.stdout);(out/(label+'.stderr')).write_bytes(p.stderr)
 commands.append({'label':label,'command':args,'exit_code':p.returncode})
 (out/'commands.json').write_text(json.dumps(commands,indent=2)+'\n')
 print(label,p.returncode,flush=True);return p.returncode
# Preserve the exact unsupported example as a command failure, then use the
# existing admitted smoke profile. No runner/scorer feature is invented.
call('unsupported-example',[ev,'run','--backend','mcp-stdio','--binary',product['binary_path'],'--suite','crates/cc-eval/benchmarks/manifests/p0-smoke.json','--output',str(out/'unsupported-baseline'),'--profile','baseline'])
for name in ['p0-smoke','p1b-exact','p1c-intents','p0-codecortex-subset','p0-rust-api','p0-python-api','p1c-softscope','p1c-bm25']:
 suite=f'crates/cc-eval/benchmarks/manifests/{name}.json';dest=out/name
 call(name+'-validate',[ev,'validate','--suite',suite])
 call(name+'-run',[ev,'run','--backend','mcp-stdio','--binary',product['binary_path'],'--suite',suite,'--output',str(dest),'--profile','smoke'])
 if (dest/'manifest.json').exists():
  before={str(p.relative_to(dest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in dest.rglob('*') if p.is_file()}
  code=call(name+'-replay',[ev,'replay','--run',str(dest)])
  after={str(p.relative_to(dest)):hashlib.sha256(p.read_bytes()).hexdigest() for p in dest.rglob('*') if p.is_file()}
  changes={p: {'before':before.get(p),'after':after.get(p)} for p in sorted(set(before)|set(after)) if before.get(p)!=after.get(p)}
  (out/(name+'-replay-digests.json')).write_text(json.dumps({'files':before,'changed':changes,'replay_exit':code},indent=2)+'\n')
assert subprocess.check_output(['git','status','--porcelain'],cwd=root)==b''
print('COMPLETE clean current run',flush=True)
