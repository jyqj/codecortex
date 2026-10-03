"""Validate committed query/source locks using a real clean pinned Git checkout."""
import pathlib,json,hashlib,tempfile,subprocess,sys
base=pathlib.Path(__file__).resolve().parents[1]; upstream=pathlib.Path(sys.argv[1]).resolve(); evaluator=pathlib.Path(sys.argv[2]).resolve(); evidence=[]
for profile in ['native','compat']:
 suite=base/f'suite-{profile}-dev.json'; s=json.loads(suite.read_text()); s['source']['root']=str(upstream); s['source']['commit']='43fe48e8a0f44af783116cdb010725e6bb50255f'; s['queries']=str((base/s['queries']).resolve())
 with tempfile.TemporaryDirectory(prefix='gin-eval-git-lock-') as tmp:
  p=pathlib.Path(tmp)/'suite.json'; p.write_text(json.dumps(s,indent=2)+'\n'); proc=subprocess.run([str(evaluator),'validate','--suite',str(p)],capture_output=True)
  evidence.append(dict(profile=profile,committed_suite_sha256=hashlib.sha256(suite.read_bytes()).hexdigest(),ephemeral_git_suite_sha256=hashlib.sha256(p.read_bytes()).hexdigest(),exit_code=proc.returncode,diagnostics_sha256=hashlib.sha256(proc.stdout+proc.stderr).hexdigest(),source_commit=s['source']['commit'],source_blake3=s['source']['digest'],queries_blake3=s['queries_digest']))
  assert proc.returncode==0
print(json.dumps(dict(status='actual_evaluator_clean_git_and_source_byte_locks_passed',checks=evidence,retrieval_run=False),indent=2))
