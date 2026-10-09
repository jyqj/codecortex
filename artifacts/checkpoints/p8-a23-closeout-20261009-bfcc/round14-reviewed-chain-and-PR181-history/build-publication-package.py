from pathlib import Path,PurePosixPath
import json,hashlib,zipfile,stat,re,datetime
BASE=Path('/workspace/scratch/bfccb8494ba0');OUT=Path('/dev/shm/pr-triage-round14-chain-publication')
PREFIX='artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/round14-reviewed-chain-and-PR181-history'
LIMIT=8*1024*1024

def identity(b):return {'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()}
def meta(p):return {'local_path':str(p),**identity(p.read_bytes())}
def write_json(p,v):
 with p.open('x') as f:json.dump(v,f,ensure_ascii=False,indent=2);f.write('\n')
def safe(n):
 p=PurePosixPath(n);assert n and '\0' not in n and '\\' not in n and not p.is_absolute() and '..' not in p.parts and str(p)==n,n
inputs=[];members={}
def add(p,name,expected=None,original_repository_path=None):
 safe(name);assert name not in members
 b=p.read_bytes();m=meta(p)
 if expected:
  assert m['bytes']==expected['bytes'] and m['sha256']==expected['sha256']
  if 'git_blob' in expected:assert m['git_blob']==expected['git_blob']
 assert not re.search(rb'https://[^\s"<>]+[?&](?:sig|X-Amz-Signature|X-Goog-Signature)=',b),str(p)
 inputs.append(m);members[name]={'data':b,'input':m,'original_repository_path':original_repository_path}
selection1=BASE/'scale-intake-primary/round14-R-thin-review/archive-selection.json';s1=json.loads(selection1.read_text());assert s1['file_count']==5 and s1['total_bytes']==1865762
for e in s1['files']:
 add(Path(s1['base'])/e['path'],'scale-intake-primary/'+e['path'],e)
add(selection1,'scale-intake-primary/round14-R-thin-review/archive-selection.json')
selection2=Path('/dev/shm/pr-triage-round14-pr181-retirement/archive-selection.json');s2=json.loads(selection2.read_text());assert s2['file_count']==6 and s2['total_bytes']==198055
for e in s2['files']:add(Path(e['local_path']),'pr181-retirement/'+Path(e['local_path']).name,e,e['repository_path'])
add(selection2,'pr181-retirement/archive-selection.json')
for name in ['actual-P-R-G-H-and-initial-admission-preparation.json','PR181-external-closure-observation.json']:
 add(BASE/'root-round14'/name,'root-round14/'+name)
assert len(members)==15
closure=json.loads(members['root-round14/PR181-external-closure-observation.json']['data']);official=closure['official_metadata']
assert official['merged'] is True and official['head']['sha']=='d7426175ae21a9619cbcb393e41ec535695972d1' and official['merge_commit_sha']=='b9412406e11422d7cf914458a8bfbbd58cf94eaa' and official['merged_at']=='2026-10-09T11:00:55Z'
prep=json.loads(members['root-round14/actual-P-R-G-H-and-initial-admission-preparation.json']['data']);assert prep['publication_candidate']['structuredContent']['sha']=='399a71a4c11e7041ce9946fba4d3f72200c47155' and prep['original_tasks_remaining']==29
assert 'original_v15_not_started' in prep['native_H_initial_preparation']['structuredContent']['output']['stdout_tail']
zip_path=OUT/'selected-chain-and-PR181-history.zip'
with zipfile.ZipFile(zip_path,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=9) as z:
 for name,r in sorted(members.items()):
  zi=zipfile.ZipInfo(name,date_time=(2026,10,9,11,2,40));zi.create_system=3;zi.external_attr=(stat.S_IFREG|0o644)<<16;zi.compress_type=zipfile.ZIP_DEFLATED;z.writestr(zi,r['data'])
def inspect_zip(path,expect=None):
 rows=[];seen=set()
 with zipfile.ZipFile(path) as z:
  for i in z.infolist():
   safe(i.orig_filename);assert i.orig_filename==i.filename and i.filename not in seen and not stat.S_ISLNK(i.external_attr>>16)
   seen.add(i.filename);b=z.read(i)
   if expect is not None:assert b==expect[i.filename]['data']
   rows.append({'member':i.filename,**identity(b),'crc32':f'{i.CRC:08x}','original_repository_path':expect[i.filename]['original_repository_path'] if expect is not None else None})
 if expect is not None:assert seen==set(expect)
 return {'container':meta(path),'member_count':len(rows),'member_bytes':sum(x['bytes'] for x in rows),'every_member_crc_read':True,'safe_unique_paths':True,'members':rows}
archives=[inspect_zip(zip_path,members)]
existing=[(Path('/dev/shm/todo-audit-pr180-v15-and-forward-preparation.zip'),460708,'6c253524cf58eac36005dad45c14b8521f2066336835d190a79ce62a1fb2d0d2'),(Path('/dev/shm/todo-audit-pr180-actual-G-and-forward-readback.zip'),364170,'2237c3ef5d03208e2841ab70ca357da9fa2a8d383156092b79704ce7e2c23c92')]
for path,size,digest in existing:
 m=meta(path);assert m['bytes']==size and m['sha256']==digest;inputs.append(m);archives.append(inspect_zip(path))
for m in inputs:assert meta(Path(m['local_path']))==m
readme='''# Reviewed chain and PR181 history — round 14

This append-only publication preserves the reviewed P/R/G/H preparation and two different PR181 observations at their actual times. Packaging only reads bytes, checks SHA/Git blob identities and reads every ZIP member for CRC; it does not rerun source validators, Cargo, CI or research.

## Time and identity boundaries

- The `pr181-retirement/` report and unposted comment draft concern head `27e60348f01d16298dd4d28f48dd1e2d47ee687c`, still open/draft in the original 2026-10-09 10:59:46 UTC recheck. They are historical planning records, not an executed closure and not a review of the later head.
- The subsequent official observation records PR181 externally **merged at 11:00:55 UTC**, head `d7426175ae21a9619cbcb393e41ec535695972d1`, merge commit/main `b9412406e11422d7cf914458a8bfbbd58cf94eaa`, base `55aa2bcf355441585bcf980e1d6f4fab8eebe59d`. Root did not post the old draft, close PR181 or perform this merge. Its source/history consequences are reviewed separately; this package does not reinterpret the old retirement recommendation as an approval of the new head.
- P is `c92eb5ac7ece70d1f62271d7e2dacac513c285b5`; R is `f89d5feec91ae611d52614b8fe217bcd7fd5cf64`; G is `b2bbd5817ce00367cea52696b098c6eafe4aa0a6`; H is `399a71a4c11e7041ce9946fba4d3f72200c47155`. The frozen root preparation snapshot records H's successful initial native preparation with 1,246 selected entries. **The original H v15 had not started and was not accepted at that snapshot.** Static G/H and forward-readback reviews do not supply its execution credit.
- The independent source audit and P's six-command native evidence (29 tests) already reside under R/staging H at `artifacts/checkpoints/p8-public-api-merge-bfcc-20261009/`; their large original containers are not copied again here. Their actual P/cached-target scope is unchanged.
- Ongoing new-main/H2 work and subsequent actual execution are outside this frozen package and will be appended separately. No artifact, source, selection or prior failed/pending result is overwritten.

## Containers

| Published container | Contents |
|---|---|
| `todo-audit-pr180-v15-and-forward-preparation.zip` | Existing 460,708-byte audit package, byte-for-byte reused. |
| `todo-audit-pr180-actual-G-and-forward-readback.zip` | Existing 364,170-byte audit package, byte-for-byte reused. |
| `selected-chain-and-PR181-history.zip` | Five R thin-review originals plus their original selection; six historical PR181 review files plus their original selection; both frozen root observations. Exactly 15 members. |

The newly packed ZIP lays out `scale-intake-primary/round14-R-thin-review/`, `pr181-retirement/` and `root-round14/`. Original selection bytes remain intact; `manifest.json` records both actual container members and earlier suggested repository paths where present. The reused ZIPs remain independent containers and were not unpacked into the new ZIP. Every upload item is at most 8 MiB; no split or reassembly is needed. `upload-index.json` gives exact local sources, repository destinations, sizes, SHA-256 and Git blob IDs.

## Original task accounting

**163 done /29 remaining; newly completed original TODOs: 0.** PR history, source review and packaging do not close tasks. Existing scoped component passes remain valid at their original identities. Complete registered same-source scale/statistics, original hard dependencies and the original P8-007 actual mixed-path DB lock acquisition wait observation remain outstanding where previously recorded. Availability probes are not relabeled as that missing observation. No additional telemetry architecture or threshold is introduced.
'''
with (OUT/'README.md').open('x') as f:f.write(readme)
manifest={'schema_version':1,'created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'prefix':PREFIX,'scope':'Byte-preserving packaging only; two prior ZIPs reused; original selections and time-qualified observations frozen.','inputs_before_after_identical':True,'original_inputs':inputs,'archives':archives,'new_container_original_file_count':15,'original_todos_remaining':29,'root_PR181_close_comment_or_merge_executed':False,'H_original_v15_execution_at_frozen_snapshot':'not_started_not_accepted','new_main_H2_scope':'excluded_pending_separate_work'}
write_json(OUT/'manifest.json',manifest)
paths=[existing[0][0],existing[1][0],zip_path,OUT/'README.md',OUT/'manifest.json',Path(__file__)]
uploads=[]
for p in paths:
 m=meta(p);assert m['bytes']<=LIMIT;uploads.append({**m,'repository_path':PREFIX+'/'+p.name,'mode':'100644'})
index={'schema_version':1,'prefix':PREFIX,'file_count':len(uploads),'total_bytes':sum(x['bytes'] for x in uploads),'files':uploads,'maximum_item_bytes':LIMIT,'all_items_within_limit':True,'split_parts':False,'manifest_sha256':meta(OUT/'manifest.json')['sha256']}
write_json(OUT/'upload-index.json',index)
for m in inputs:assert meta(Path(m['local_path']))==m
print(json.dumps({'upload_index':meta(OUT/'upload-index.json'),'file_count':len(uploads),'total_bytes':index['total_bytes'],'new_zip':meta(zip_path),'archives_member_counts':[x['member_count'] for x in archives]},ensure_ascii=False))
