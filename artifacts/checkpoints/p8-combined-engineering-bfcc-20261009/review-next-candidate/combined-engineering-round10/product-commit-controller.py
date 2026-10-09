from pathlib import Path
import json,subprocess,hashlib,datetime,re
base=Path.cwd();src=base/'candidate-combined';out=base/'review-next-candidate/combined-engineering-round10';p=out/'product-commit';p.mkdir(exist_ok=False)
final=json.loads((out/'attempt01/final.json').read_text());assert final['all_four_completed'] and final['all_original_commands_succeeded']
frozen={'paths':json.loads((out/'frozen-nine-path-source.json').read_text())};paths=[x['path'] for x in frozen['paths']];parent='275e8799d4947d297329073eaa3ca675d3fd0777'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()==parent
for x in frozen['paths']:assert hashlib.sha256((src/x['path']).read_bytes()).hexdigest()==x['sha256']
full_maps=json.loads((out/'complete-product-validation-inputs.json').read_text())
mapping={x['path']:{**x,'domain':domain,'blob':x['git_blob']} for domain in ['product','validation'] for x in full_maps[domain]}
assert len(full_maps['product'])==1090 and len(full_maps['validation'])==139
command_inventory=json.loads((out/'attempt01/frozen-product-validation-inputs.json').read_text())
assert len(command_inventory)==1232
expected_digest=hashlib.sha256(json.dumps(command_inventory,sort_keys=True,separators=(',',':')).encode()).hexdigest()
assert len(final['results'])==4 and all(r['exit_code']==0 and r['source_unchanged'] and r['source_before_digest']==expected_digest and r['source_after_digest']==expected_digest for r in final['results'])
for x in command_inventory:
 f=src/x['path'];data=f.read_bytes();mode='100755' if f.stat().st_mode&0o111 else '100644';assert mode==x['mode'] and hashlib.sha256(data).hexdigest()==x['sha256']
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
command('git-commit',['git','commit','-m','perf(db): integrate guarded rebuild FTS with macOS checks'])
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
bridge={'schema':'combined-round10-product-commit-source-bridge-v1','commit':commit,'tree':tree,'parents':parents,'branch':'task/p8-combined-engineering-bfcc-20261009','changed_paths':expected,'working_tree_clean':True,'only_nine_product_test_paths_committed':True,'all_other_Git_paths_modes_and_blobs_identical_to_parent':True,'complete_product_inputs_verified':sum(x['domain']=='product' for x in mapping.values()),'ordinary_validation_inputs_verified':sum(x['domain']=='validation' for x in mapping.values()),'full_input_map_sha256':hashlib.sha256((out/'complete-product-validation-inputs.json').read_bytes()).hexdigest(),'all_committed_modes_blobs_equal_execution_snapshot':True,'actual_test_identity':'G173 275e8799 parent checkout plus these nine uncommitted source files, then exact identity bridge; not re-labelled as direct P checkout execution','per_command_inventory':'1232 captured inputs: all 1090 product, all 139 ordinary validation, and 3 live binding inputs before and after each of the original four commands.','raw_validation_receipts':'../attempt01','remote_push':False,'R_G_bindings_created':False,'new_original_todos_completed':0}
b=p/'source-bridge.json';b.write_text(json.dumps(bridge,indent=2)+'\n');print(json.dumps(bridge,indent=2));print('bridge_meta',b.stat().st_size,hashlib.sha256(b.read_bytes()).hexdigest())

raw_commit=subprocess.check_output(['git','cat-file','commit',commit],cwd=src);(p/'commit.raw').write_bytes(raw_commit);assert hashlib.sha1(b'commit '+str(len(raw_commit)).encode()+b'\0'+raw_commit).hexdigest()==commit
print('raw_commit',len(raw_commit),hashlib.sha256(raw_commit).hexdigest())
