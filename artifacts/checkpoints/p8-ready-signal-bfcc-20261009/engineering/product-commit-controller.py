import pathlib,subprocess,json,hashlib,datetime,os
base=pathlib.Path.cwd();src=base/'candidate-ready-signal';root=base/'review-next-candidate/ready-signal-round11';out=root/'product-commit';out.mkdir(exist_ok=False)
head='9f7b16f0758eb79f306cf44605b84550f02de441';path='crates/cc-semantic/tests/crash_independent_review.rs'
wanted='9a81c1b79fcb71bc71f95e67a86c3ebd070b70797e9a06fe7d9df93f72238545'
def git(*a):return subprocess.check_output(['git','-C',str(src),*a])
def write(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
def fp(p):b=p.read_bytes();return {'path':str(p.relative_to(base)),'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest()}
final=json.loads((root/'attempt01/final.json').read_text())
assert final['all_five_completed'] is True and final['all_original_commands_succeeded'] is True
names=['semantic-fault-matrix','fmt','clippy','workspace-tests','final-fixture']
assert [x['name'] for x in final['results']]==names
frozen=json.loads((root/'attempt01/frozen-inputs.json').read_text());assert len(frozen)==1235
expected_digest=hashlib.sha256(json.dumps(frozen,sort_keys=True,separators=(',',':')).encode()).hexdigest()
for name in names:
 r=json.loads((root/'attempt01'/str(name+'.json')).read_text())
 assert r['exit_code']==0 and r['source_unchanged'] is True
 assert r['source_before_digest']==expected_digest==r['source_after_digest']
 assert json.loads((root/'attempt01'/str(name+'.native-exit.json')).read_text())['exit_code']==0
 for stream in ['stdout','stderr']:
  assert fp(root/'attempt01'/str(name+'.'+stream))['sha256']==r[stream]['sha256']
for x in frozen:
 f=src/x['path'];b=f.read_bytes()
 assert len(b)==x['bytes'] and hashlib.sha256(b).hexdigest()==x['sha256']
 assert ('100755' if f.stat().st_mode&0o111 else '100644')==x['mode']
assert git('rev-parse','HEAD').decode().strip()==head
assert not git('diff','--cached','--name-only').strip()
assert git('diff','--name-only').decode().splitlines()==[path]
assert hashlib.sha256((src/path).read_bytes()).hexdigest()==wanted
assert git('diff','--full-index','--',path)==(root/'source.diff').read_bytes()
write(out/'precommit-verification.json',{'verified_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'parent':head,'actual_execution_identity':'G9f plus single uncommitted fixture; all five original commands completed before this commit','all_five_child_exit0':True,'input_count':1235,'input_digest':expected_digest,'product_count':1090,'validation_count':139,'trust_root_paths':3,'additional_ledger_and_review_paths':3,'allowed_staged_paths':[path],'source_sha256':wanted})
r=subprocess.run(['git','-C',str(src),'add','--',path],capture_output=True,text=True);write(out/'git-add.json',{'argv':r.args,'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr});assert r.returncode==0
assert git('diff','--cached','--name-only').decode().splitlines()==[path]
r=subprocess.run(['git','-C',str(src),'commit','-m','fix(semantic): publish complete crash-test ready signals atomically'],capture_output=True,text=True)
write(out/'git-commit.json',{'argv':r.args,'exit_code':r.returncode,'stdout':r.stdout,'stderr':r.stderr});assert r.returncode==0
p=git('rev-parse','HEAD').decode().strip();tree=git('rev-parse','HEAD^{tree}').decode().strip()
assert git('rev-list','--parents','-n','1',p).decode().split()==[p,head]
assert git('diff-tree','--no-commit-id','--name-status','-r',p).decode().strip()=='M\t'+path
assert not git('status','--porcelain').strip()
(out/'commit.raw').write_bytes(git('cat-file','commit',p))
alltree={}
for row in git('ls-tree','-r','-z',p).split(b'\0'):
 if row:
  fields,n=row.split(b'\t',1);mode,kind,oid=fields.decode().split();alltree[n.decode()]=(mode,kind,oid)
for x in frozen:
 assert alltree[x['path']]==(x['mode'],'blob',x['git_blob']),x['path']
oids=[x['git_blob'] for x in frozen]
raw=subprocess.check_output(['git','-C',str(src),'cat-file','--batch'],input=('\n'.join(oids)+'\n').encode())
offset=0
for x in frozen:
 end=raw.index(b'\n',offset);h=raw[offset:end].decode().split();n=int(h[2]);body=raw[end+1:end+1+n];offset=end+2+n
 assert h[0]==x['git_blob'] and h[1]=='blob' and n==x['bytes'] and hashlib.sha256(body).hexdigest()==x['sha256']
assert offset==len(raw)
write(out/'complete-product-validation-inputs.json',{'source':p,'tree':tree,'parent':head,'product_count':1090,'validation_count':139,'base_delta_count':47,'files':frozen,'basis':'All1235 actual execution byte/mode/blob inputs exactly equal committed P'})
bridge={'schema_version':1,'product_commit':p,'product_tree':tree,'parent':head,'status_porcelain':'','changed_status':{'M':[path]},'actual_execution_identity':'G9f plus one uncommitted fixture file, then immutable P bridge; commands were not rerun under P checkout','commands':names,'all_five_exit0':True,'source_input_count':1235,'input_digest':expected_digest,'product_count':1090,'validation_count':139,'binding_count':3,'additional_ledger_review_count':3,'base':'7354db236c9d9850a75f31672697ae9eab44565e','historical_delta_count':47,'commit_raw':fp(out/'commit.raw'),'complete_map':fp(out/'complete-product-validation-inputs.json'),'mode_blob_sha256_all_match_actual_execution_inputs':True,'new_todo_credit':0,'remaining_todo':29,'no_R_G_push_or_newstudy':True}
write(out/'source-bridge.json',bridge)
print(json.dumps({'P':p,'tree':tree,'parent':head,'source_bridge':fp(out/'source-bridge.json'),'rawcommit':fp(out/'commit.raw'),'complete_map':fp(out/'complete-product-validation-inputs.json')}),flush=True)
