#!/usr/bin/env python3
"""Create an evidence-only commit on pinned PR110 without changing the checkout."""
import json,os,pathlib,subprocess
O=pathlib.Path(__file__).resolve().parent;ROOT=O.parents[2]
p=json.loads((O/'remote-plan.json').read_text());manifest=json.loads((O/'curated-manifest.json').read_text());paths=[str((O/x['path']).relative_to(ROOT)) for x in manifest['files']]+[str((O/'curated-manifest.json').relative_to(ROOT)),str((O/'SHA256SUMS').relative_to(ROOT))]
assert all(x.startswith('artifacts/checkpoints/fifo-100k-paired-decision-20261003/') for x in paths)
index=O/'runtime/delivery-index';assert not index.exists();env={**os.environ,'GIT_INDEX_FILE':str(index)}
def git(*args,**kw):return subprocess.check_output(['git',*args],cwd=ROOT,env=env,**kw)
git('read-tree',p['base_sha']);(O/'runtime/commit-paths.txt').write_bytes(b'\0'.join(x.encode() for x in paths)+b'\0');git('add','--force','--pathspec-from-file='+str(O/'runtime/commit-paths.txt'),'--pathspec-file-nul')
tree=git('write-tree',text=True).strip();commit=git('commit-tree',tree,'-p',p['base_sha'],input=b'test: retain pre-registered paired 100k FIFO performance decision\n').decode().strip();changed=git('diff','--name-only',p['base_sha'],commit,text=True).splitlines();assert sorted(changed)==sorted(paths)
assert git('diff',p['base_sha'],commit,'--','crates','scripts','Cargo.toml','Cargo.lock',text=True)==''
git('update-ref','refs/heads/'+p['head_branch'],commit,'0'*40)
(O/'local-commit-receipt.json').write_text(json.dumps({'commit':commit,'tree':tree,'parent':p['base_sha'],'branch':p['head_branch'],'changed_files':changed,'production_delta_from_base':'','checkout_preserved':True},indent=2)+'\n')
print(json.dumps({'commit':commit,'files':len(changed),'branch':p['head_branch']}))
