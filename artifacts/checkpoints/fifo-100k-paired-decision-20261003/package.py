#!/usr/bin/env python3
import gzip,hashlib,json,pathlib,shutil,subprocess
O=pathlib.Path(__file__).resolve().parent
def read(p):return json.loads(p.read_text())
def eligible(p):
 rel=p.relative_to(O).parts
 return p.is_file() and rel[0]!='runtime' and not any(x in rel for x in ['repo','cache']) and p.name not in ['SHA256SUMS','curated-manifest.json']
# Store original large observations compressed; do not delete corpus/cache/db/binaries.
for label in ['baseline','candidate']:
 out=O/label
 for p in [out/'cargo-build.jsonl',out/'build-resources.jsonl',out/'summary.json',out/'live/n100000/cold-result.json']:
  if p.exists():
   with p.open('rb') as src,gzip.open(str(p)+'.gz','wb') as dst:shutil.copyfileobj(src,dst)
   p.unlink()
 paths=[out/'live/n100000/repo',out/'live/n100000/cache',O/'runtime'/label]
 retained=[]
 for root in paths:
  counts=bytes_=allocated=0;errors=[]
  if root.exists():
   for p in root.rglob('*'):
    try:
     if p.is_file():
      s=p.stat();counts+=1;bytes_+=s.st_size;allocated+=s.st_blocks*512
    except OSError as e:errors.append(str(e))
  retained.append({'path':str(root),'regular_files_observed':counts,'logical_bytes_observed':bytes_,'allocated_bytes_observed':allocated,'errors':errors,'retained_local':True,'excluded_from_git':True})
 (out/'retained-local-data.json').write_text(json.dumps(retained,indent=2)+'\n')
# Curated checksums only; no runtime, corpus, cache or database is uploaded.
files=sorted(p for p in O.rglob('*') if eligible(p));entries=[{'path':str(p.relative_to(O)),'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in files]
(O/'curated-manifest.json').write_text(json.dumps({'files':entries,'file_count':len(entries),'logical_bytes':sum(x['bytes'] for x in entries),'excluded':['runtime builds/registry/binaries','fresh source corpus/db/cache'],'product_outcome_not_modified':True},indent=2)+'\n')
(O/'SHA256SUMS').write_text(''.join(x['sha256']+'  '+x['path']+'\n' for x in entries))
print(json.dumps({'files':len(entries),'bytes':sum(x['bytes'] for x in entries)}))
