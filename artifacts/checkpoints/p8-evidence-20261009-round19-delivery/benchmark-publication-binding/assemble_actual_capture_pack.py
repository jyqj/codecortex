#!/usr/bin/env python3
"""Copy exact captured official objects; no network or Git repository writes."""
import hashlib,json
from pathlib import Path
B=Path('/workspace/scratch/a217aaae3bde/checkpoint-round19-prep')
P=Path('/dev/shm/a217aaae3bde/platform-review/round19-full41-actual-tree')
O=B/'benchmark-publication-binding'
def load(p):return json.loads(p.read_bytes())
def meta(p):
 b=p.read_bytes();return {'path':str(p),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
paths=[P/'commit.json',P/'root.json',P/'artifacts.json',P/'checkpoints.json',P/'originals-recursive.json',B/'E-41-originals-ref-readback-raw.json',B/'E-41-originals-published.json',P/'independent-review.json']
refraw=load(paths[5]);assert refraw['isError'] is False
ref=json.loads(refraw['structuredContent']['content'])
pack={'schema':'copied-official-full41-tree-captures-v1','commit':load(paths[0]),'ref':ref,'trees':[load(p) for p in paths[1:4]],'recursive_trees':[load(paths[4])],'capture_sources':[meta(p) for p in paths],'derivation':'Only parsed exact saved official JSON objects. Recursive child tree views are reconstructed by the binder; no separate child GET is claimed.'}
p=O/'actual-780296-official-tree-pack.json'
with p.open('xb') as f:f.write((json.dumps(pack,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode())
print(json.dumps(meta(p)))
