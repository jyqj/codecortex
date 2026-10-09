from pathlib import Path
import json,subprocess,hashlib,datetime,re
base=Path.cwd();src=base/'candidate-fts-integration';out=base/'review-next-candidate/fts-integration-round9';p=out/'product-commit';p.mkdir(exist_ok=False)
final=json.loads((out/'attempt01/final.json').read_text());assert final['all_four_completed'] and final['all_original_commands_succeeded']
frozen=json.loads((out/'frozen-five-path-source.json').read_text());paths=[x['path'] for x in frozen['paths']];parent='ae77486dab0141173fcb2c4c99dd85d34e888157'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()==parent
for x in frozen['paths']:assert hashlib.sha256((src/x['path']).read_bytes()).hexdigest()==x['sha256']
mapping=json.loads((out/'execution-full-product-validation-inputs.json').read_text())
for rel,x in mapping.items():
 f=src/rel;data=f.read_bytes();mode='100755' if f.stat().st_mode&0o111 else '100644';assert mode==x['mode'] and hashlib.sha256(data).hexdigest()==x['sha256']
def command(name,args):
 r={'argv':args,'cwd':str(src),'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat()}
 with (p/(name+'.stdout')).open('wb') as so,(p/(name+'.stderr')).open('wb') as se:r['exit_code']=subprocess.call(args,cwd=src,stdout=so,stderr=se)
 r['finished_at']=datetime.datetime.now(datetime.timezone.utc).isoformat()
 for ext in ['stdout','stderr']:
  b=(p/(name+'.'+ext)).read_bytes();r[ext]={'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
 (p/(name+'.json')).write_text(json.dumps(r,indent=2)+'\n');assert r['exit_code']==0,(name,r['exit_code']);return r
command('git-add',['git','add','--']+paths)
staged=subprocess.check_output(['git','diff','--cached','--name-status','HEAD'],cwd=src,text=True);(p/'staged-name-status.txt').write_text(staged)
expected={rel:('A' if rel.endswith('/snapshot_fts_tests.rs') else 'M') for rel in paths}
assert {line.split('\t',1)[1]:line.split('\t',1)[0] for line in staged.splitlines()}==expected
command('staged-diff-check',['git','diff','--cached','--check'])
command('git-commit',['git','commit','-m','perf(db): batch rebuild FTS mirrors with guarded rowid windows'])
commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip();tree=subprocess.check_output(['git','rev-parse','HEAD^{tree}'],cwd=src,text=True).strip();parents=subprocess.check_output(['git','show','-s','--format=%P','HEAD'],cwd=src,text=True).strip().split();assert parents==[parent]
assert not subprocess.check_output(['git','status','--porcelain=v1'],cwd=src)
diff=subprocess.check_output(['git','diff','--name-status',parent,commit],cwd=src,text=True);assert diff==staged
raw=subprocess.check_output(['git','ls-tree','-rz',commit],cwd=src).split(b'\0');actual={}
for entry in raw:
 if not entry:continue
 h,r=entry.split(b'\t',1);mode,kind,blob=h.decode().split();actual[r.decode()]={'mode':mode,'type':kind,'blob':blob}
for rel,x in mapping.items():assert actual[rel]['mode']==x['mode'] and actual[rel]['blob']==x['blob'],rel
all_parent=subprocess.check_output(['git','ls-tree','-rz',parent],cwd=src).split(b'\0');old={}
for entry in all_parent:
 if not entry:continue
 h,r=entry.split(b'\t',1);mode,kind,blob=h.decode().split();old[r.decode()]={'mode':mode,'type':kind,'blob':blob}
changed=sorted(k for k in set(old)|set(actual) if old.get(k)!=actual.get(k));assert changed==sorted(paths)
bridge={'schema':'fts-round9-product-commit-source-bridge-v1','commit':commit,'tree':tree,'parents':parents,'branch':'task/p8-fts-integration-bfcc-20261009','changed_paths':expected,'working_tree_clean':True,'only_five_product_test_paths_committed':True,'all_other_Git_paths_modes_and_blobs_identical_to_parent':True,'complete_product_inputs_verified':sum(x['domain']=='product' for x in mapping.values()),'ordinary_validation_inputs_verified':sum(x['domain']=='validation' for x in mapping.values()),'full_input_map_sha256':hashlib.sha256((out/'execution-full-product-validation-inputs.json').read_bytes()).hexdigest(),'all_committed_modes_blobs_equal_execution_snapshot':True,'actual_test_identity':'ae77486 parent checkout plus these five uncommitted source files, then exact identity bridge; not re-labelled as direct P checkout execution','per_command_inventory':'1214 captured inputs; all1090 product inputs included.17 ordinary source-integrity validation files use unchanged parent+pre-execution exact-scope proof and post-execution full mode/blob verification, not a claimed per-command capture.','raw_validation_receipts':'../attempt01','remote_push':False,'R_G_bindings_created':False,'new_original_todos_completed':0}
b=p/'source-bridge.json';b.write_text(json.dumps(bridge,indent=2)+'\n');print(json.dumps(bridge,indent=2));print('bridge_meta',b.stat().st_size,hashlib.sha256(b.read_bytes()).hexdigest())
