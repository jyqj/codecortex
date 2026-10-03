"""Archive only this run's public DEV evidence and prove byte-identical offline replay."""
import hashlib,json,subprocess,tarfile
from pathlib import Path
ROOT=Path('/workspace/codecortex')
BASE=ROOT/'artifacts/checkpoints/public-dev-current-group-pygo-20261003'
RUN=Path('/tmp/public-dev-current-group-pygo-20261003-full')
OUT=Path('/tmp/public-dev-current-group-pygo-20261003-readback')
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def inventory(root):return {p.relative_to(root).as_posix():{'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(root.rglob('*')) if p.is_file()}
files=inventory(RUN)
assert all(Path(n).suffix in ['.json','.jsonl','.md','.stdout','.stderr'] for n in files)
archive=BASE/'public-development-current-raw.tar.gz'
assert not archive.exists() and not OUT.exists()
with tarfile.open(archive,'w:gz') as t:
 for n in files:t.add(RUN/n,arcname=n,recursive=False)
OUT.mkdir()
with tarfile.open(archive,'r:gz') as t:
 for m in t.getmembers():
  assert m.isfile() and not Path(m.name).is_absolute() and '..' not in Path(m.name).parts
 t.extractall(OUT,filter='data')
assert inventory(OUT)==files
records=[]
plan=json.loads((BASE/'preregistration-receipt.json').read_bytes())
evaluator=json.loads(Path(plan['build_receipt_path']).read_bytes())['artifacts']['cc-eval']['copied_binary']
for r in json.loads((OUT/'commands.json').read_bytes()):
 cmd=[evaluator,'replay','--run',str(OUT/r['key'])]
 p=subprocess.run(cmd,capture_output=True)
 assert p.returncode==r['replay_exit_code']
 records.append({'suite':r['key'],'replay_exit_code':p.returncode,'stdout_sha256':hashlib.sha256(p.stdout).hexdigest(),'stderr_sha256':hashlib.sha256(p.stderr).hexdigest()})
assert inventory(OUT)==files
output=Path('/tmp/public-dev-current-group-pygo-20261003-analysis-readback')
p=subprocess.run(['python3',str(BASE/'selftools/analyze-current.py'),'--plan',str(BASE/'preregistration-receipt.json'),'--run',str(OUT),'--output',str(output)],capture_output=True)
assert p.returncode==0
for n in ['summary.json','case-means.jsonl','paired-projections.jsonl']:
 assert sha(output/n)==sha(BASE/'analysis'/n)
manifest={'schema_version':1,'scope':'new_current_group_pygo_public_dev_raw_only_no_full_source_binary_db','archive_sha256':sha(archive),'files':files}
(BASE/'raw-artifact-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
receipt={'schema_version':1,'scope':'new_current_group_pygo_offline_readback_only','archive_sha256':sha(archive),'raw_manifest_sha256':sha(BASE/'raw-artifact-manifest.json'),'retained_file_count':len(files),'retained_bytes':sum(v['bytes'] for v in files.values()),'all_files_byte_identical_after_extract_and_replay':True,'replay_records':records,'analysis_summary_byte_identical':True,'analysis_summary_sha256':sha(BASE/'analysis/summary.json'),'new_retrieval_calls':0,'source_binary_db_archived':False}
(BASE/'readback-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt))
