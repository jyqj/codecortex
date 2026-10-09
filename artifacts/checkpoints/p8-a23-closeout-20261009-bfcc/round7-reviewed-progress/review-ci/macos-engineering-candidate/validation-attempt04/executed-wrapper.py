import pathlib,subprocess,json,hashlib,datetime,os,sys,time,traceback
b=pathlib.Path.cwd()
w=b/'candidate-macos'
out=b/'review-ci/macos-engineering-candidate/validation-attempt04'
target=b/'targets/macos-engineering-1.95-sdk154-round7'
expected_head='b21cce4c8661589267ad5719f850accbec088d2f'
expected={'crates/cc-eval/tests/p8_scale.rs':'71831ab2b2ab0e7b34e9917f499bc33f7b66a74e0c156874b4154bb6b783d8dc','crates/cc-index/src/project_model/python_inventory.rs':'e3fd1a31e0ed7cb9c8fa90a7025508a957f0bc8288f7850fff5e400094a5203e','crates/cc-semantic/tests/p7_cache_allocation_independent_review.rs':'5d9d105a151dff3b82bb27125e5e4d276966611de0ef74634cb3a4a67396f495','crates/cc-eval/src/benchmark/p8_scale.rs':'18e6fbe38bc0484ff32050c749186671a7e4c3b15c0894b5fb205c9948fb7530','crates/cc-eval/tests/benchmark_adapters.rs':'a5ef80d761a89c83d97998fe02e68a185d6c803ed0999723d9b24d90d1f1289c'}
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(x): return hashlib.sha256(x).hexdigest()
def git(*args): return subprocess.check_output(['git',*args],cwd=w)
def identity():
 return {'head':git('rev-parse','HEAD').decode().strip(),'head_tree':git('rev-parse','HEAD^{tree}').decode().strip(),'branch':git('branch','--show-current').decode().strip(),'modified_paths':git('diff','HEAD','--name-only','-z').decode().strip('\0').split('\0'),'untracked_paths':[s for s in git('ls-files','--others','--exclude-standard','-z').decode().split('\0') if s],'status_porcelain':git('status','--porcelain').decode(),'diff_sha256':sha(git('diff','HEAD','--full-index','--binary')),'files':{p:{'bytes':(w/p).stat().st_size,'sha256':sha((w/p).read_bytes())} for p in expected}}
def validate(i):
 assert i['head']==expected_head,i
 assert set(i['modified_paths'])==set(expected),i
 assert i['untracked_paths']==[],i
 assert {p:v['sha256'] for p,v in i['files'].items()}==expected,i
first=identity();validate(first)
assert target.is_dir(),'expected same round7 target retained from attempt01'
assert not out.exists(),'refuse evidence overwrite'
review=b/'review-ci/macos-engineering-candidate/two-file-independent-source-review.json'
assert sha(review.read_bytes())=='08fafc692d13da6492eaabd536bc4ed6e77b9b8cb1c331accc34daa3546ea1e6'
out.mkdir(parents=True)
(out/'source.diff').write_bytes(git('diff','HEAD','--full-index','--binary'))
for source_path in expected:
 snapshot=out/'source-files'/source_path;snapshot.parent.mkdir(parents=True,exist_ok=True);snapshot.write_bytes((w/source_path).read_bytes())
def save(name,value): (out/name).write_text(json.dumps(value,ensure_ascii=False,indent=2)+'\n')
try:
 (out/'executed-wrapper.py').write_bytes(pathlib.Path(__file__).read_bytes())
except Exception as e:
 save('wrapper-source-copy-error.json',{'error':repr(e)})
env=os.environ.copy()
env['PATH']='/Users/jin/.cargo/bin:'+env.get('PATH','')
env['RUSTUP_TOOLCHAIN']='1.95.0'
env['SDKROOT']='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk'
env['CARGO_TARGET_DIR']=str(target)
env['CARGO_BUILD_JOBS']='4'
removed={k:env.pop(k) for k in ['RUST_TEST_THREADS'] if k in env}
overrides={k:env[k] for k in ['PATH','RUSTUP_TOOLCHAIN','SDKROOT','CARGO_TARGET_DIR','CARGO_BUILD_JOBS']}
commands=[('01-fmt',['cargo','fmt','--all','--','--check']),('02-clippy',['cargo','clippy','--workspace','--all-targets','--','-D','warnings']),('03-workspace-test',['cargo','test','--workspace']),('04-integration-fixtures',['cargo','test','-p','cc-eval','--','integration_fixtures_and_corpus'])]
plan={'started_at':now(),'worktree':str(w),'source':first,'commands':[{'name':n,'command':c} for n,c in commands],'environment_overrides':overrides,'removed_environment_for_default_test_threads':removed,'other_relevant_inherited_environment':{k:env[k] for k in ['RUSTFLAGS','RUSTDOCFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_PROFILE_DEV_DEBUG','CARGO_INCREMENTAL'] if k in env},'target_existed_before':True,'target_reuse_authorization':'Parent explicitly authorized reuse of only this round7 target after attempt01 failure; a23 and oracle targets untouched','source_review_scope':'Existing independent two-file review remains exact; newly authorized import-only third path, equivalent fourth predicate and seven private-test Linux cfg fifth-path attributes; full-scope independent addendum pending','default_test_threads':True,'independent_review':{'path':str(review.relative_to(b)),'sha256':sha(review.read_bytes())},'contributing':{'path':'CONTRIBUTING.md','sha256':sha((w/'CONTRIBUTING.md').read_bytes())},'stop_on_first_failure':True,'todo_credit':0,'remaining_original_todos':29}
save('plan.json',plan)
save('progress.json',{'status':'preflight','at':now()})
pre=[]
for name,argv in [('rustc',['/Users/jin/.cargo/bin/rustc','-Vv']),('cargo',['/Users/jin/.cargo/bin/cargo','-Vv']),('sdk',['/usr/bin/xcrun','--sdk','macosx15.4','--show-sdk-path'])]:
 rr=subprocess.run(argv,cwd=w,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
 (out/('preflight-'+name+'.stdout')).write_bytes(rr.stdout);(out/('preflight-'+name+'.stderr')).write_bytes(rr.stderr)
 rec={'name':name,'command':argv,'exit_code':rr.returncode,'stdout_sha256':sha(rr.stdout),'stderr_sha256':sha(rr.stderr)};pre.append(rec)
 if rr.returncode!=0:
  save('preflight.json',pre);save('final.json',{'status':'preflight_failed','at':now(),'preflight':pre,'validation_commands_started':0});sys.exit(rr.returncode)
assert b'rustc 1.95.0 ' in (out/'preflight-rustc.stdout').read_bytes()
assert (out/'preflight-sdk.stdout').read_text().strip()==env['SDKROOT']
save('preflight.json',pre)
receipts=[];final_exit=0
for name,command in commands:
 before=identity();validate(before)
 assert before==first,'source drift before command'
 actual=['/Users/jin/.cargo/bin/cargo']+command[1:]
 started=now()
 with (out/(name+'.stdout')).open('wb') as stdout,(out/(name+'.stderr')).open('wb') as stderr:
  proc=subprocess.Popen(actual,cwd=w,env=env,stdout=stdout,stderr=stderr)
  save('progress.json',{'status':'running','command_name':name,'command':command,'pid':proc.pid,'started_at':started,'source_before':before})
  print(json.dumps({'event':'started','name':name,'pid':proc.pid,'at':started}),flush=True)
  rc=proc.wait()
 after=identity()
 receipt={'name':name,'command':command,'actual_argv':actual,'cwd':str(w),'environment_overrides':overrides,'default_test_threads':True,'pid':proc.pid,'started_at':started,'finished_at':now(),'exit_code':int(rc),'source_before':before,'source_after':after,'source_unchanged':before==after==first,'stdout':{'path':name+'.stdout','bytes':(out/(name+'.stdout')).stat().st_size,'sha256':sha((out/(name+'.stdout')).read_bytes())},'stderr':{'path':name+'.stderr','bytes':(out/(name+'.stderr')).stat().st_size,'sha256':sha((out/(name+'.stderr')).read_bytes())}}
 save(name+'.json',receipt);receipts.append(receipt)
 print(json.dumps({'event':'finished','name':name,'exit_code':rc,'source_unchanged':receipt['source_unchanged'],'at':receipt['finished_at']}),flush=True)
 if rc!=0 or not receipt['source_unchanged']:
  final_exit=rc if rc!=0 else 125
  break
final={'status':'passed' if len(receipts)==4 and all(x['exit_code']==0 and x['source_unchanged'] for x in receipts) else 'not_passed','finished_at':now(),'command_receipts':[{'name':r['name'],'exit_code':r['exit_code'],'source_unchanged':r['source_unchanged']} for r in receipts],'not_run':[n for n,c in commands if n not in {r['name'] for r in receipts}],'source_final':identity(),'wrapper_exit':final_exit,'todo_credit':0,'remaining_original_todos':29}
save('final.json',final);save('progress.json',{'status':'completed','at':now(),'final':final})
print(json.dumps(final),flush=True)
sys.exit(final_exit)
