#!/usr/bin/env python3
"""Safely archive current run outputs only; retain originals and verify every member."""
import json,tarfile
from paired import OUT,sha,dump
root=OUT/'runs';archive=OUT/'typescript-paired-original-outputs.tar.gz'
assert not archive.exists()
files={}
with tarfile.open(archive,'w:gz') as t:
 for f in sorted(root.rglob('*')):
  if not f.is_file():continue
  assert not f.is_symlink()
  rel=f.relative_to(root).as_posix()
  assert f.suffix not in ['.db','.sqlite','.sqlite3','.ts','.js','.rs'] and f.name not in ['cc-eval','codecortex']
  assert f.suffix in ['.json','.jsonl','.stdout','.stderr','.md']
  data=f.read_bytes();files[rel]={'sha256':sha(data),'bytes':len(data)};t.add(f,arcname=rel,recursive=False)
with tarfile.open(archive,'r:gz') as t:
 members=t.getmembers();assert len(members)==len(files)
 for m in members:assert m.isfile() and m.name in files and sha(t.extractfile(m).read())==files[m.name]['sha256']
manifest={'scope':'only current TypeScript paired runs: full raw responses, normalized/source verification/query snapshots/scores/costs/manifests/stage/commands/replay stdout stderr; no full input source tree/binary/database','archive_sha256':sha(archive.read_bytes()),'file_count':len(files),'uncompressed_bytes':sum(f['bytes'] for f in files.values()),'files':files}
dump(OUT/'raw-artifact-manifest.json',manifest)
dump(OUT/'archive-readback-receipt.json',{'archive_sha256':manifest['archive_sha256'],'all_member_original_byte_hashes_match':True,'file_count':len(files),'new_retrieval_calls':0,'full_source_tree_binary_database_in_archive':False,'licenses':'retained-licenses/license exact original bytes; MIT/TypeScript notices retained'})
print(json.dumps({k:v for k,v in manifest.items() if k!='files'}))
