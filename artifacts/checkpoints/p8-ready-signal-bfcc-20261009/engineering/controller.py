import pathlib,subprocess,json,hashlib,datetime,os,platform
base=pathlib.Path.cwd();src=base/'candidate-ready-signal';out=base/'review-next-candidate/ready-signal-round11/attempt01'
out.mkdir(parents=True,exist_ok=False)
target=base/'targets/ready-signal-1.95-sdk154-round11';assert target.is_dir()
head='9f7b16f0758eb79f306cf44605b84550f02de441'
review_path='artifacts/checkpoints/p8-combined-engineering-bfcc-20261009/independent-source-review.json'
expected=json.loads((out.parent/'frozen-inputs.json').read_text())
paths=sorted(set(x['path'] for x in expected['files'])|{'docs/roadmap/code-index-v2/tasks.json','docs/roadmap/code-index-v2/05-TODO.md',review_path})
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def write(p,d):p.write_text(json.dumps(d,indent=2)+'\n')
def inventory():
 rows=[]
 for n in paths:
  p=src/n;assert p.is_file() and not p.is_symlink(),n
  b=p.read_bytes();rows.append({'path':n,'mode':'100755' if p.stat().st_mode&0o111 else '100644','bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(b)).encode()+b'\0'+b).hexdigest()})
 return rows
def digest(rows):return hashlib.sha256(json.dumps(rows,sort_keys=True,separators=(',',':')).encode()).hexdigest()
frozen=inventory();by_path={x['path']:x for x in frozen}
assert all(by_path[x['path']]==x for x in expected['files'])
assert len(frozen)==1235
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()==head
assert subprocess.check_output(['git','diff','--name-only'],cwd=src,text=True).splitlines()==['crates/cc-semantic/tests/crash_independent_review.rs']
write(out/'frozen-inputs.json',frozen)
env=os.environ.copy()
for k in ['RUST_TEST_THREADS','RUST_TEST_NOCAPTURE','RUSTFLAGS','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_PROFILE_TEST_DEBUG']:
 env.pop(k,None)
env.update({'SDKROOT':'/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk','CARGO_TARGET_DIR':str(target),'CARGO_BUILD_JOBS':'4','PATH':'/Users/jin/.cargo/bin:'+env.get('PATH','')})
cargo='/Users/jin/.cargo/bin/cargo'
ci_env=env.copy();ci_env.update({'CARGO_INCREMENTAL':'0','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','CARGO_BUILD_JOBS':'2','RUSTFLAGS':'-D warnings'})
ci_env['CRASH_INDEPENDENT_EVIDENCE_DIR']=str(out/'fault-crash-evidence')
write(out/'platform.json',{'platform':platform.platform(),'rustc_verbose':subprocess.check_output(['/Users/jin/.cargo/bin/rustup','run','1.95.0','rustc','-Vv'],env=env,text=True),'SDKROOT':env['SDKROOT'],'source_parent':head,'source_scope':'G9f plus one uncommitted fixture-only change; P null','input_count':len(frozen),'input_digest':digest(frozen),'cache_provenance':'../target-clone.json','original_failure_platform':'Ubuntu GitHub Actions; current reproduction/repair validation is native Mac, not Linux CI','test_threads':'default','scope':'Original semantic all-targets fault command then the four original engineering commands. No benchmark, scale, retry, skip or changed budgets.'})
commands=[
 ('semantic-fault-matrix',[cargo,'+1.95.0','test','-p','cc-semantic','--locked','--all-targets','--','--nocapture'],ci_env),
 ('fmt',[cargo,'+1.95.0','fmt','--all','--','--check'],env),
 ('clippy',[cargo,'+1.95.0','clippy','--workspace','--all-targets','--','-D','warnings'],env),
 ('workspace-tests',[cargo,'+1.95.0','test','--workspace'],env),
 ('final-fixture',[cargo,'+1.95.0','test','-p','cc-eval','--','integration_fixtures_and_corpus'],env)]
results=[]
for name,cmd,e in commands:
 before=inventory();assert before==frozen
 assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()==head
 receipt={'name':name,'command':cmd,'cwd':str(src),'status':'running','started_at':now(),'source_before_digest':digest(before),'source_input_count':len(before),'environment':{k:e.get(k) for k in ['SDKROOT','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_PROFILE_TEST_DEBUG','RUSTFLAGS','RUST_TEST_THREADS','CRASH_INDEPENDENT_EVIDENCE_DIR']},'test_threads':'default','skip_ignore_filter_override':False}
 write(out/(name+'.json'),receipt)
 with (out/(name+'.stdout')).open('wb') as so,(out/(name+'.stderr')).open('wb') as se:
  child=subprocess.Popen(cmd,cwd=src,env=e,stdout=so,stderr=se);receipt['pid']=child.pid;write(out/(name+'.json'),receipt);code=child.wait()
 write(out/(name+'.native-exit.json'),{'exit_code':code,'finished_at':now()})
 receipt.update(exit_code=code,status='completed',finished_at=now())
 after=inventory();receipt['source_after_digest']=digest(after);receipt['source_unchanged']=before==after
 for stream in ['stdout','stderr']:
  data=(out/(name+'.'+stream)).read_bytes();receipt[stream]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
 write(out/(name+'.json'),receipt);results.append(receipt)
 print(json.dumps({'name':name,'exit_code':code,'source_unchanged':receipt['source_unchanged']}),flush=True)
 if code!=0 or not receipt['source_unchanged']:break
write(out/'final.json',{'results':results,'all_five_completed':len(results)==5,'all_original_commands_succeeded':len(results)==5 and all(x['exit_code']==0 and x['source_unchanged'] for x in results),'original_G9f_failure_retained':True,'wrapper_scope':'Wrapper0 records observations, never equals child success or TODO approval.'})
