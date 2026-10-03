import pathlib,json,hashlib
O=pathlib.Path(__file__).resolve().parent
files=[]
for p in sorted(O.rglob('*')):
 if not p.is_file():continue
 rel=p.relative_to(O)
 if rel.parts[0]=='runtime' or 'repo' in rel.parts or 'cache' in rel.parts or p.name in ['SHA256SUMS','curated-manifest.json']:continue
 files.append({'path':str(rel),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()})
(O/'curated-manifest.json').write_text(json.dumps({'files':files,'scope':'only this checkpoint; original failed evidence, products, CI and parent source read-only','raw_sqlite_cache_and_binary':'local retained and Git excluded; identities/observations retained','compressed_logs':'lossless gzip raw logs; replay code only reads evidence','formal_runs_per_variant':1,'no_rerun':True},indent=2)+'\n')
files.append({'path':'curated-manifest.json','sha256':hashlib.sha256((O/'curated-manifest.json').read_bytes()).hexdigest()})
(O/'SHA256SUMS').write_text(''.join(f"{x['sha256']}  {x['path']}\n" for x in files))
for x in files:assert hashlib.sha256((O/x['path']).read_bytes()).hexdigest()==x['sha256']
print('sealed and verified',len(files),'evidence files')
