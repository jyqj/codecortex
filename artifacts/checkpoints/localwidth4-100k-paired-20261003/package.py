import pathlib,json,gzip,hashlib,shutil,subprocess,os
O=pathlib.Path(__file__).resolve().parent
exclude=lambda p:any(x in ('runtime','repo','cache','__pycache__') for x in p.relative_to(O).parts)
def admitted_files():
 for root,dirs,names in os.walk(O):
  dirs[:]=[d for d in dirs if d not in ('runtime','repo','cache','__pycache__')]
  for n in names:yield pathlib.Path(root)/n
for p in sorted(admitted_files()):
 if not p.is_file() or exclude(p) or p.suffix=='.gz':continue
 if p.suffix=='.jsonl' or p.stat().st_size>128*1024:
  with p.open('rb') as src,(pathlib.Path(str(p)+'.gz')).open('wb') as raw:
   with gzip.GzipFile(fileobj=raw,mode='wb',mtime=0,filename='') as dst:shutil.copyfileobj(src,dst)
  p.unlink()
files=[p for p in sorted(admitted_files()) if p.is_file() and not exclude(p) and p.name not in ('SHA256SUMS','curated-manifest.json')]
rows=[{'path':str(p.relative_to(O)),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in files]
manifest={'scope':'only this new task','files':rows,'file_count':len(rows),'total_bytes':sum(x['bytes'] for x in rows),'excluded':'owned rebuildable corpus/DB/cache/targets/binaries; no previous restricted localdiag/experiment2GB/42file copied','raw_sensitivity':'owned deterministic resource_N Rust inputs; loopback dummy bearer; no real-provider requests, heldout, credentials or private source','production_modified':False}
(O/'curated-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n');files.append(O/'curated-manifest.json')
(O/'SHA256SUMS').write_text(''.join(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+str(p.relative_to(O))+'\n' for p in files))
print(json.dumps({'file_count':len(files),'bytes':manifest['total_bytes']}))
