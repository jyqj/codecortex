from pathlib import Path
import subprocess, json, hashlib, os, time, datetime, sys
root=Path.cwd(); out=root.parent/'native-public-dependency-fix-evidence'; label='control-preparation-v2'
baseline=json.loads((out/'before-domain-receipt.json').read_text()); expected={x['path']:dict(x) for x in baseline['entries']}
test='crates/cc-db/tests/p2b_resolution_store.rs'
expected[test].update(bytes=17946,sha256='39f49426530bd7367bb49030b1c8602ae5be908069ca3dfe4d974f8111a98ae9',git_blob='f009b732320705dbb221aa22a4f32d1aa9e2cfea')
expected['crates/cc-db/src/index_db_write_batch.rs'].update(bytes=57238,sha256='9d3300d999f58743cb42799c88e3328a101859ac879cc2161abbec17faa9b9b9',git_blob='34c1e5e5188910482fcf80863008620d30fa73b1')
expected['crates/cc-db/src/snapshot_write_txn.rs'].update(bytes=9204,sha256='5bf74ee89cd731afb8d42b6a3f951b9198e5b2de5172a8cfc94290a026134b3f',git_blob='159bec62dc5f8c5548aaf3fcffb608dcc8682c40')
extra=json.loads((out/'v2-complete-additional-inputs.json').read_text())
for e in extra['additional_compile_inputs']: expected[e['path']]=e
def snapshot(phase):
 rows=[]
 for name,e in sorted(expected.items()):
  p=root/name;data=p.read_bytes(); row={'path':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest(),'mode':'100755' if p.stat().st_mode & 0o111 else '100644'}
  assert row==e,(phase,name,row,e)
  rows.append(row)
 b=(json.dumps({'source_G':baseline['source'],'label':label,'phase':phase,'modified_paths':[test,'crates/cc-db/src/index_db_write_batch.rs','crates/cc-db/src/snapshot_write_txn.rs'],'rows':rows},indent=2,sort_keys=True)+'\n').encode(); p=out/(label+'-input-'+phase+'.json');p.write_bytes(b)
 return {'path':p.name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'entries':len(rows)}
before=snapshot('before')
env=os.environ.copy();env['PATH']='/Users/jin/.cargo/bin:'+env.get('PATH','');env['SDKROOT']='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk';env['CARGO_TARGET_DIR']=str(root.parent/'targets/public-dependency-fix-1.95-sdk154-round13')
commands=[('resolution-store-suite-v2',['test','--locked','-p','cc-db','--test','p2b_resolution_store'],7),('snapshot-binder-dependency-unit-v2',['test','--locked','-p','cc-db','--lib','index_db_snapshot_insert::'],14),('snapshot-leaf-owner-control-v2',['test','--locked','-p','cc-index','--test','snapshot_leaf_batching'],1),('workspace-format-v2',['fmt','--all','--','--check'],0),('cc-db-strict-clippy-v2',['clippy','--locked','-p','cc-db','--all-targets','--','-D','warnings'],0)]
import re
all_receipts=[]
for label,args,minimum_tests in commands:
 cmd=['/Users/jin/.cargo/bin/cargo','+1.95.0']+args
 before=snapshot('before'); start=time.monotonic()
 receipt={'schema':1,'label':label,'source_G':baseline['source'],'cwd':str(root),'command':cmd,'minimum_tests':minimum_tests,'before':before,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'ordinary engineering regression on exact3ff plus frozen three-file patch v2 and two restored original artifact compile inputs; cloned cached target; no cold/performance or whole-repository claim','environment':{k:env.get(k) for k in ['SDKROOT','CARGO_TARGET_DIR','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CC','CXX']}}
 sp=out/(label+'.stdout');ep=out/(label+'.stderr');rp=out/(label+'-receipt.json')
 with sp.open('wb') as so,ep.open('wb') as se:
  p=subprocess.Popen(cmd,cwd=str(root),env=env,stdout=so,stderr=se);receipt['pid']=p.pid;rp.write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({'started':label,'pid':p.pid,'command':cmd}),flush=True);code=p.wait()
 receipt.update(exit_code=code,elapsed_seconds=time.monotonic()-start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),after=snapshot('after'))
 for stream,path in [('stdout',sp),('stderr',ep)]:
  data=path.read_bytes();receipt[stream]={'path':path.name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()};print(label+' '+stream.upper()+'_TAIL\n'+data.decode(errors='replace')[-12000:],flush=True)
 text=sp.read_text(errors='replace');summaries=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed;',text);receipt['test_summaries']=[{'passed':int(a),'failed':int(b)} for a,b in summaries]
 receipt['nonzero_test_check_passed']=minimum_tests==0 or (sum(int(a) for a,b in summaries)>=minimum_tests and all(int(b)==0 for a,b in summaries))
 rp.write_text(json.dumps(receipt,indent=2)+'\n');all_receipts.append(receipt);print('RECEIPT '+json.dumps(receipt,sort_keys=True),flush=True)
 if code!=0 or not receipt['nonzero_test_check_passed']:
  (out/'controls-progress-v2.json').write_text(json.dumps(all_receipts,indent=2)+'\n');sys.exit(code if code!=0 else 1)
(out/'controls-progress-v2.json').write_text(json.dumps(all_receipts,indent=2)+'\n');print('ALL_CONTROLS_PASSED',len(all_receipts),flush=True)
