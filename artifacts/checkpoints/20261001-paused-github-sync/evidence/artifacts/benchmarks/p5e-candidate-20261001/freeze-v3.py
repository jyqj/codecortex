from pathlib import Path
import hashlib,json,subprocess,tarfile
R=Path.cwd();O=R/'artifacts/benchmarks/p5e-candidate-20261001/final-source-v3';O.mkdir(exist_ok=False)
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
old=json.load(open(R/'artifacts/benchmarks/p5d-20260930-resume/final-v3/source-manifest.json'))
paths={x['path'] for x in old['files']}
for folder in ['crates','scripts','docs','.github']:
 for p in (R/folder).rglob('*'):
  if p.is_file() and 'roadmap' not in p.parts and not any(n in p.parts for n in ['target','.codecortex','__pycache__','.git']) and p.suffix in ['.rs','.toml','.lock','.py','.sh','.md','.json','.jsonl','.sql','.yml','.yaml','.ts','.js','.txt','.html','.vue','.svelte','.go','.c','.cpp','.h']:paths.add(str(p.relative_to(R)))
files=[{'path':name,'bytes':(R/name).stat().st_size,'sha256':sha(R/name)} for name in sorted(paths) if (R/name).is_file()]
for row in files:assert not (R/row['path']).is_symlink()
digest=hashlib.sha256(json.dumps(files,sort_keys=True,separators=(',',':')).encode()).hexdigest()
with tarfile.open(O/'source.tar.gz','w:gz') as archive:
 for row in files:archive.add(R/row['path'],arcname=row['path'],recursive=False)
manifest={'schema_version':1,'status':'immutable_candidate_source_prepared_pending_profile_and_acceptance','head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'worktree_status':subprocess.check_output(['git','status','--short'],text=True),'source_digest_sha256':digest,'file_count':len(files),'total_bytes':sum(x['bytes'] for x in files),'archive_sha256':sha(O/'source.tar.gz'),'files':files,'scope':'production/config/tests/fixtures/internal docs/CI/scripts; mutable roadmap and artifact evaluation harness locked separately; uncommitted worktree is explicit'}
(O/'source-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:v for k,v in manifest.items() if k not in ['files','worktree_status']},indent=2))
