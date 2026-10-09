import pathlib,subprocess,json,hashlib,datetime,os,platform
base=pathlib.Path.cwd();src=base/'candidate-fts-integration';out=base/'review-next-candidate/fts-integration-round9/attempt01';out.mkdir(parents=True,exist_ok=False)
target=base/'targets/fts-integration-1.95-sdk154-round9';assert not target.exists()
expected_head='ae77486dab0141173fcb2c4c99dd85d34e888157'
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=src,text=True).strip()==expected_head
tracked=subprocess.check_output(['git','ls-files','-z','--','crates','scripts','.github','Cargo.toml','Cargo.lock','rust-toolchain.toml','rustfmt.toml','clippy.toml','.cargo'],cwd=src).decode().split('\0')
paths=sorted(set(p for p in tracked if p)|{'crates/cc-db/src/snapshot_fts_tests.rs'})
def inventory():
 values=[]
 for rel in paths:
  f=src/rel
  if f.is_symlink():data=os.readlink(f).encode();mode='120000'
  elif f.is_file():data=f.read_bytes();mode='100755' if f.stat().st_mode&0o111 else '100644'
  else:values.append({'path':rel,'missing':True});continue
  values.append({'path':rel,'mode':mode,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()})
 return values
def digest(inv):return hashlib.sha256(json.dumps(inv,sort_keys=True,separators=(',',':')).encode()).hexdigest()
frozen=inventory();(out/'frozen-product-validation-inputs.json').write_text(json.dumps(frozen,indent=2)+'\n');expected_digest=digest(frozen)
env=os.environ.copy()
for k in ['RUST_TEST_THREADS','RUST_TEST_NOCAPTURE']:env.pop(k,None)
env.update({'SDKROOT':'/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk','CARGO_TARGET_DIR':str(target),'CARGO_BUILD_JOBS':'4','PATH':'/Users/jin/.cargo/bin:'+env.get('PATH','')})
platform_info={'platform':platform.platform(),'SDKROOT':env['SDKROOT'],'CARGO_TARGET_DIR':str(target),'CARGO_BUILD_JOBS':'4','test_threads':'default; RUST_TEST_THREADS unset; no harness --test-threads','parent':expected_head,'source_input_count':len(frozen),'source_input_digest':expected_digest,'rustc_verbose':subprocess.check_output(['/Users/jin/.cargo/bin/rustup','run','1.95.0','rustc','-Vv'],env=env,text=True)}
(out/'platform.json').write_text(json.dumps(platform_info,indent=2)+'\n')
commands=[
 ('fmt',['/Users/jin/.cargo/bin/cargo','+1.95.0','fmt','--all','--','--check']),
 ('clippy',['/Users/jin/.cargo/bin/cargo','+1.95.0','clippy','--workspace','--all-targets','--','-D','warnings']),
 ('workspace-tests',['/Users/jin/.cargo/bin/cargo','+1.95.0','test','--workspace']),
 ('final-fixture',['/Users/jin/.cargo/bin/cargo','+1.95.0','test','-p','cc-eval','--','integration_fixtures_and_corpus'])
]
results=[]
for name,cmd in commands:
 before=inventory();assert before==frozen
 r={'name':name,'command':cmd,'cwd':str(src),'status':'running','started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'source_before_digest':digest(before),'source_input_count':len(before),'environment':{k:env[k] for k in ['SDKROOT','CARGO_TARGET_DIR','CARGO_BUILD_JOBS']},'test_threads':'default','skip_ignore_filter_override':False}
 (out/(name+'.json')).write_text(json.dumps(r,indent=2)+'\n')
 with (out/(name+'.stdout')).open('wb') as so,(out/(name+'.stderr')).open('wb') as se:
  child=subprocess.Popen(cmd,cwd=src,env=env,stdout=so,stderr=se);r['pid']=child.pid;(out/(name+'.json')).write_text(json.dumps(r,indent=2)+'\n');code=child.wait()
 r.update({'exit_code':code,'status':'completed','finished_at':datetime.datetime.now(datetime.timezone.utc).isoformat()})
 after=inventory();r['source_after_digest']=digest(after);r['source_unchanged']=before==after
 for ext in ['stdout','stderr']:
  data=(out/(name+'.'+ext)).read_bytes();r[ext]={'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()}
 if after!=before:(out/(name+'.source-after-drift.json')).write_text(json.dumps(after,indent=2)+'\n')
 (out/(name+'.json')).write_text(json.dumps(r,indent=2)+'\n');results.append(r)
 print(json.dumps({'name':name,'exit_code':code,'source_unchanged':r['source_unchanged']}),flush=True)
 if code!=0 or not r['source_unchanged']:break
(out/'final.json').write_text(json.dumps({'results':results,'all_four_completed':len(results)==4,'all_original_commands_succeeded':len(results)==4 and all(x['exit_code']==0 and x['source_unchanged'] for x in results),'wrapper_scope':'Records child integer status. Wrapper 0 is never by itself validation approval.'},indent=2)+'\n')
