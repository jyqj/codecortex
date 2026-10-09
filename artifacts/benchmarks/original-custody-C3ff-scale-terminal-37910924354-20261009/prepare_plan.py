"""Prepare exact original terminal ZIP custody; no remote operation or workload."""
from pathlib import Path
import hashlib,json,stat
O=Path(__file__).resolve().parent
T=O.parent
PREFIX='artifacts/benchmarks/original-custody-C3ff-scale-terminal-37910924354-20261009'
PARENT='84edf9c08b4cc41384171d44e60a7676684260a2'
SOURCE='3ffcefc3b28ee1a4ed80caecebd7208a45c3e302'
CHUNK=2097152
sha=lambda b:hashlib.sha256(b).hexdigest()
oid=lambda b:hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()
canon=lambda x:(json.dumps(x,indent=2,sort_keys=True)+'\n').encode()
def write(p,data):
 p.parent.mkdir(parents=True,exist_ok=True)
 with p.open('xb') as f:f.write(data)
 assert p.read_bytes()==data

elements=[];records=[];proof=[];chunks={}
def add(path,source):
 data=source.read_bytes();elements.append(dict(path=PREFIX+'/'+path,source=str(source),mode='100644',bytes=len(data),sha256=sha(data),git_blob=oid(data)))
for identity in [11626430756,11626127365]:
 source=T/'original-zips'/f'github-actions-artifact-{identity}.zip';meta_path=T/f'artifact-{identity}-official-metadata.json';meta=json.loads(meta_path.read_text());assert meta['id']==identity and meta['workflow_run']['id']==37910924354 and meta['workflow_run']['head_sha']==SOURCE
 before=source.stat();assert stat.S_ISREG(before.st_mode) and not source.is_symlink();parts=[];offset=0;whole=hashlib.sha256()
 with source.open('rb') as f:
  while block:=f.read(CHUNK):
   h=sha(block);whole.update(block);part=dict(path=f'chunks/{h}.bin',offset=offset,bytes=len(block),sha256=h,git_blob=oid(block));parts.append(part)
   chunks.setdefault(h,dict(path=PREFIX+'/'+part['path'],source_zip=str(source),source_offset=offset,mode='100644',bytes=len(block),sha256=h,git_blob=part['git_blob']));offset+=len(block)
 assert offset==before.st_size==meta['size_in_bytes'] and 'sha256:'+whole.hexdigest()==meta['digest']
 reassembled=hashlib.sha256()
 for part in parts:
  with source.open('rb') as f:f.seek(part['offset']);block=f.read(part['bytes'])
  assert len(block)==part['bytes'] and sha(block)==part['sha256'] and oid(block)==part['git_blob'];reassembled.update(block)
 after=source.stat();signature=lambda s:[s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns];assert signature(before)==signature(after) and reassembled.hexdigest()==whole.hexdigest()
 manifest=dict(schema='codecortex-original-actions-zip-custody-v1',artifact_id=identity,artifact_name=meta['name'],run_id=37910924354,run_attempt=1,source_commit=SOURCE,actual_execution_checkout=SOURCE,original_zip_bytes=offset,original_zip_sha256=whole.hexdigest(),original_zip_filename=source.name,official_artifact_locator=meta['url'],official_created_at=meta['created_at'],official_expires_at=meta['expires_at'],chunk_bytes=CHUNK,chunks=parts,reconstruction='Concatenate exact original chunk bytes in ascending contiguous offset; no ZIP repacking',scope='original failed100k shard' if identity==11626430756 else 'original failed aggregate',accepted_measurement_samples=0,new_native_execution=False,new_task_completion=False,transport_scope='first original retrieval of these terminal artifact IDs by root; no native re-execution')
 p=O/f'manifests/{identity}.json';write(p,canon(manifest));add(f'manifests/{identity}.json',p);add(f'official-metadata/{identity}.json',meta_path)
 records.append(dict(artifact_id=identity,bytes=offset,sha256=whole.hexdigest(),chunks=len(parts),manifest=f'manifests/{identity}.json',manifest_sha256=sha(p.read_bytes()),accepted_measurement_samples=0))
 proof.append(dict(artifact_id=identity,source_zip=str(source),stat_before_after=signature(before),whole_sha256=whole.hexdigest(),contiguous_second_read_sha256=reassembled.hexdigest(),unchanged=True))
existing=dict(commit=PARENT,build_and_four_accepted_zip_prefix='artifacts/benchmarks/original-custody-C3ff-scale-37910924354-20261009',older_26_zip_prefix='artifacts/benchmarks/original-custody-C3ff-20261009',all_existing_leaves='unchanged, including prior timestamped pending-scope catalogs')
catalog=dict(schema='codecortex-original-actions-custody-catalog-v1',source_commit=SOURCE,source_tree='0e655b4258afd9ca040dd7c1ace868bf1c322a30',run_id=37910924354,run_attempt=1,artifact_count=2,artifacts=records,original_zip_bytes=sum(x['bytes'] for x in records),unique_chunk_count=len(chunks),study_status='closed_failed',accepted_shards=4,accepted_samples=41,required_shards=150,required_samples=1500,new_accepted_measurement_credit=0,registered_repetitions=30,existing_custody=existing,publication_state='planned only; no remote objects/ref claim',native_execution=False,task_statuses_changed=False)
write(O/'catalog-planned.json',canon(catalog));write(O/'source-byte-preservation-proof.json',canon(dict(schema='terminal-custody-plan-byte-proof-v1',artifacts=proof,zip_members_extracted=False,native_or_helper_replay=False)))
write(O/'README.md',b'''# Original C3ff terminal scale evidence

This sibling collection preserves the original failed 100k repetition-0 ZIP and
original failed aggregate ZIP for run 37910924354 at fixed C3ff. The native
supervisor reported deadline_exceeded/exit3; the aggregate rejected the missing
or failed shard. Remaining145 measurements were skipped. The four previously
accepted shards and41 samples retain their original scope; no partial100k cold
prefix is admitted as a sample. No threshold, N30, source or plan changed.

Both originals use contiguous2MiB chunks with whole SHA256 and per-chunk
SHA256/Git blob identities. No ZIP bytes are repacked or modified. Restore with
the unchanged restore_original_zip.py and a manifests/<id>.json into an absent
output path. Existing31 original ZIPs, their catalogs and historical scope
statements remain unchanged at parent84edf9c; this terminal collection supplies
the later failure status. It does not grant any TODO completion or new native
measurement credit.
''')
assert oid((O/'restore_original_zip.py').read_bytes())=='b4972957fa531e3ad64fe131a075700f8e3e0ff2'
for name in ['catalog-planned.json','source-byte-preservation-proof.json','README.md','restore_original_zip.py','prepare_plan.py']:add(name,O/name)
elements.extend(sorted(chunks.values(),key=lambda x:x['path']));assert len(elements)==13 and len({x['path'] for x in elements})==13
plan=dict(schema='C-terminal-two-original-ZIP-custody-append-plan-v1',expected_branch='evidence/p8-originals-c3ff-a217-20261009',expected_parent=PARENT,expected_parent_tree='bffebe291a9e16d5abd5f537e9dcec68a14cfb33',new_prefix=PREFIX,only_new_prefix_additions=True,elements=elements,artifact_count=2,original_zip_bytes=catalog['original_zip_bytes'],chunk_count=len(chunks),file_count=len(elements),existing_custody=existing,accepted_shards=4,accepted_samples=41,formal_completion=False,remaining_todos=29,remote_objects_created=False,refs_changed=False,native_or_helper_replay=False)
write(O/'append-plan.json',canon(plan));p=O/'append-plan.json';print(str(p),len(p.read_bytes()),sha(p.read_bytes()),len(elements),len(chunks),catalog['original_zip_bytes'])
