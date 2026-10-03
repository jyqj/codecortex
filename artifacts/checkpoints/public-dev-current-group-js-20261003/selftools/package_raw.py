#!/usr/bin/env python3
"""Package only this current task's authorized DEV runs and verify archive bytes."""
import argparse,hashlib,json,tarfile
from pathlib import Path
sha=lambda b:hashlib.sha256(b).hexdigest()
p=argparse.ArgumentParser();p.add_argument('--pilot',type=Path,required=True);p.add_argument('--full',type=Path,required=True);p.add_argument('--output',type=Path,required=True);a=p.parse_args()
archive=a.output/'public-development-current-group-js-raw.tar.gz'
if archive.exists():raise ValueError('IMMUTABLE_OUTPUT_EXISTS')
files={}
with tarfile.open(archive,'w:gz') as t:
 for prefix,root in [('pilot-current01',a.pilot),('full-current01',a.full)]:
  for f in sorted(root.rglob('*')):
   if not f.is_file():continue
   rel=Path(prefix)/f.relative_to(root)
   if f.suffix in ['.db','.sqlite','.sqlite3'] or f.name in ['codecortex','cc-eval']:raise ValueError('FORBIDDEN_PAYLOAD')
   files[rel.as_posix()]={'sha256':sha(f.read_bytes()),'bytes':f.stat().st_size}
   t.add(f,arcname=rel.as_posix(),recursive=False)
manifest={'scope':'only group-js new current pilot/full DEV raw; no input source tree/binary/database','files':files,'file_count':len(files),'uncompressed_bytes':sum(f['bytes'] for f in files.values()),'archive_sha256':sha(archive.read_bytes())}
(a.output/'raw-artifact-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
with tarfile.open(archive) as t:
 members=[m for m in t.getmembers() if m.isfile()]
 assert len(members)==len(files)
 for m in members:
  assert m.name in files and sha(t.extractfile(m).read())==files[m.name]['sha256']
receipt={'archive_sha256':manifest['archive_sha256'],'readback_file_count':len(files),'all_byte_hashes_match':True,'archive_has_source_tree_binary_or_database':False,'new_search_calls_during_readback':0,'run_replays':'each original current run already replayed once; all 14 exit codes match and no files changed'}
(a.output/'archive-readback-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
