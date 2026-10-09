#!/usr/bin/env python3
import json,hashlib
from pathlib import Path
B=Path('/workspace/scratch/a217aaae3bde/checkpoint-round19-prep')
P=Path('/dev/shm/a217aaae3bde/platform-review/round20-main-delivery-actual-tree')
O=B/'benchmark-publication-binding'
def load(p):return json.loads(p.read_bytes())
def meta(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'git_blob_sha1':hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()}
trees=[P/(n+'.json') for n in ['root','artifacts','checkpoints','benchmarks']]
recursive=[P/(n+'-recursive.json') for n in ['originals','runtime-benchmarks','lifecycle-benchmarks']]
refpath=B/'main-delivery-ref-readback-raw.json';rawref=load(refpath);assert rawref['isError'] is False
ref=json.loads(rawref['structuredContent']['content'])
create=B/'main-delivery-create-branch-raw.json';before=B/'main-delivery-branch-before-raw.json'
paths=[P/'commit.json',*trees,*recursive,refpath,create,before,B/'main-delivery-branch-published.json',P/'independent-review.json']
pack={'schema':'copied-official-actual-main-delivery-captures-v1','commit':load(P/'commit.json'),'ref':ref,'trees':[load(p) for p in trees],'recursive_trees':[load(p) for p in recursive],'raw_create_branch_result':load(create),'raw_prior_ref_absence':load(before),'prior_ref_absence_capture':meta(before),'capture_sources':[meta(p) for p in paths],'derivation':'Exact captured official JSON objects parsed only; derived child tree entries remain explicitly derived from complete recursive official responses.'}
p=O/'actual-8aeb66-official-delivery-tree-pack.json'
with p.open('xb') as f:f.write((json.dumps(pack,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
print(json.dumps(meta(p)))
