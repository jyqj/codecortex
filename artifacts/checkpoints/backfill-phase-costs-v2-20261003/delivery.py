"""Package existing diagnostic evidence only; no experiment execution."""
import pathlib,json,hashlib,os,shutil,subprocess
O=pathlib.Path(__file__).resolve().parent;R=O/'source';PREFIX=pathlib.Path('artifacts/checkpoints/backfill-phase-costs-v2-20261003');BASE='29b03a0fec6bfde92aac9c41dce996ebb2d08cc7';BRANCH='backfill-phase-costs-v2-20261003'
exclude={'SHA256SUMS','checksum-verification.log','delivery.index','delivery-receipt.json','remote-after.txt','remote-pr.json','push-delivery.log'}
files=[p for p in O.iterdir() if p.is_file() and '.sqlite3' not in p.name and p.name not in exclude and not p.name.startswith('delivery.index')]
for case in O.glob('*-n*'):
 if case.is_dir():files.extend(p for p in case.iterdir() if p.is_file())
files=sorted(set(files));manifest=''.join(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+str(p.relative_to(O))+'\n' for p in files);(O/'SHA256SUMS').write_text(manifest)
verify=subprocess.run(['sha256sum','-c','SHA256SUMS'],cwd=O,capture_output=True,text=True);assert verify.returncode==0,verify.stdout+verify.stderr;(O/'checksum-verification.log').write_text(verify.stdout)
files.extend([O/'SHA256SUMS',O/'checksum-verification.log']);stage=O/'staging';dst=stage/PREFIX;dst.mkdir(parents=True,exist_ok=True)
for p in files:
 t=dst/p.relative_to(O);t.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,t)
env={**os.environ,'GIT_INDEX_FILE':str(O/'delivery.index')}
def git(*args,input=None):return subprocess.check_output(['git','--git-dir='+str(R/'.git'),'--work-tree='+str(stage),*args],cwd=stage,env=env,input=input,text=True).strip()
git('read-tree',BASE);git('add','-f','--',str(PREFIX));tree=git('write-tree');changed=git('diff','--name-only',BASE,tree).splitlines();assert changed and all(p.startswith(str(PREFIX)+'/') for p in changed);assert len(changed)==len(files),(len(changed),len(files));assert not any('/repo/' in p or '/cache/' in p or '/source/' in p or '/target/' in p or '.sqlite3' in p for p in changed)
commit=git('commit-tree',tree,'-p',BASE,input='test: retain bounded fixed-v2 backfill phase cost diagnosis\n');git('update-ref','refs/heads/'+BRANCH,commit,'0'*40)
receipt={'repository':'jyqj/codecortex','branch':BRANCH,'base_sha':BASE,'commit_sha':commit,'tree_sha':tree,'changed_files':changed,'changed_file_count':len(changed),'outside_evidence_scope':False,'status':'committed_local','commit_scope_verified':True,'checksum_exit':verify.returncode,'push':None,'draft_pr':None,'remote_verified':False}
(O/'delivery-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({k:receipt[k] for k in ['commit_sha','tree_sha','changed_file_count','status']}))
