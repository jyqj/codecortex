"""Safely archive new Express original outputs and check every member byte."""
import hashlib,json,tarfile
from pathlib import Path
D=Path(__file__).resolve().parent.parent
sha=lambda b:hashlib.sha256(b).hexdigest()
archive=D/'paired-express-original-output.tar.gz';assert not archive.exists()
files={}
roots=[('baseline/full',Path('/workspace/express-runtime/baseline/full')),('candidate/full',Path('/workspace/express-runtime/candidate/full')),
 ('execution',D/'execution'),('analysis',D/'analysis'),('diagnostics/initial',D/'analysis-initial-diagnostic'),('diagnostics/intermediate',D/'analysis-intermediate-diagnostic')]
with tarfile.open(archive,'w:gz') as t:
 for prefix,root in roots:
  for f in sorted(root.rglob('*')):
   if not f.is_file():continue
   assert not f.is_symlink()
   assert f.suffix not in ['.db','.sqlite','.sqlite3'] and f.name not in ['codecortex','cc-eval']
   rel=Path(prefix)/f.relative_to(root)
   assert 'source' not in rel.parts and '.codecortex' not in rel.parts and 'work' not in rel.parts
   b=f.read_bytes();files[rel.as_posix()]={'sha256':sha(b),'bytes':len(b)};t.add(f,arcname=rel.as_posix(),recursive=False)
 lic=D/'retained-licenses/express/LICENSE';rel='retained-licenses/express/LICENSE'
 files[rel]={'sha256':sha(lic.read_bytes()),'bytes':lic.stat().st_size};t.add(lic,arcname=rel,recursive=False)
with tarfile.open(archive) as t:
 members=t.getmembers();assert len(members)==len(files) and all(m.isfile() for m in members)
 for m in members:assert m.name in files and sha(t.extractfile(m).read())==files[m.name]['sha256']
manifest={'scope':'complete newly executed Express raw/original outputs, derived analysis and preserved initial/intermediate analysis diagnostics; no complete corpus source tree/binary/database',
 'archive_sha256':sha(archive.read_bytes()),'files':files,'file_count':len(files),'uncompressed_bytes':sum(v['bytes'] for v in files.values())}
(D/'raw-artifact-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
receipt={'archive_sha256':manifest['archive_sha256'],'readback_file_count':len(files),'all_byte_hashes_match':True,'retrieval_calls_during_packaging':0,
 'run_replay':{c['arm']+'/'+c['profile']:{'run_exit':c['run_exit_code'],'replay_exit':c['replay_exit_code'],'changed':c['changed_on_replay']} for c in json.loads((D/'commands.json').read_bytes())}}
(D/'archive-readback-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt))
