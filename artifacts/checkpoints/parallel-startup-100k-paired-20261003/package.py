import pathlib,json,gzip,shutil,hashlib,subprocess
O=pathlib.Path(__file__).resolve().parent
for label in ['baseline','candidate']:
 out=O/label
 for name in ['cargo-build.jsonl','build-resources.jsonl']:
  p=out/name
  if p.exists():
   with p.open('rb') as source,gzip.open(str(p)+'.gz','wb') as dest:shutil.copyfileobj(source,dest)
   p.unlink()
 assert (out/'verification.json').exists()
 files=[]
 for directory in [O/'runtime'/label,out/'live/n100000/repo',out/'live/n100000/cache']:
  count=logical=allocated=0
  for p in directory.rglob('*'):
   if p.is_file():
    st=p.stat();count+=1;logical+=st.st_size;allocated+=st.st_blocks*512
  files.append({'path':str(directory),'files':count,'logical_bytes':logical,'allocated_bytes':allocated,'git_excluded':True})
 (out/'retained-local-data.json').write_text(json.dumps(files,indent=2)+'\n')
repo=O.parents[2]
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()=='ff458bc591b4e7e444af4464d6eef2513cdb335c'
tracked=subprocess.check_output(['git','diff','--name-only'],cwd=repo,text=True).splitlines();assert not tracked,tracked
files=[]
for p in sorted(O.rglob('*')):
 if not p.is_file():continue
 rel=p.relative_to(O)
 if rel.parts[0]=='runtime' or 'repo' in rel.parts or 'cache' in rel.parts or p.name in ['SHA256SUMS','curated-manifest.json']:continue
 files.append({'path':str(rel),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
(O/'curated-manifest.json').write_text(json.dumps({'files':files,'scope':'only this checkpoint; original failed evidence, products, CI and parent source read-only','raw_sqlite_cache_and_binary':'local retained and Git excluded; identities/observations retained','compressed_logs':'lossless gzip raw logs; replay code only reads evidence','formal_runs_per_variant':1,'no_rerun':True},indent=2)+'\n')
files.append({'path':'curated-manifest.json','sha256':hashlib.sha256((O/'curated-manifest.json').read_bytes()).hexdigest()})
(O/'SHA256SUMS').write_text(''.join(f"{x['sha256']}  {x['path']}\n" for x in files))
print('packaged',len(files),'files',sum(x.get('bytes',0) for x in files),'bytes')
