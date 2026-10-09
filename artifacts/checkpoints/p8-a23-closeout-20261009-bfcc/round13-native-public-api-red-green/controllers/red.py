from pathlib import Path
import subprocess, json, hashlib, os, time, datetime, sys
root=Path.cwd(); out=root.parent/'native-public-dependency-fix-evidence'; label='red-public-catch-commit'
baseline=json.loads((out/'before-domain-receipt.json').read_text()); expected={x['path']:dict(x) for x in baseline['entries']}
test='crates/cc-db/tests/p2b_resolution_store.rs'
expected[test].update(bytes=17920,sha256='248c42c5e0a2179c86def636cc64d8cc9307c9a344f3897ed5833d82eac70256',git_blob='6659ddbd2d3e35b677935077e09cb003dca786d6')
def snapshot(phase):
 rows=[]
 for name,e in sorted(expected.items()):
  p=root/name;data=p.read_bytes(); row={'path':name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest(),'mode':'100755' if p.stat().st_mode & 0o111 else '100644'}
  assert row==e,(phase,name,row,e)
  rows.append(row)
 b=(json.dumps({'source_G':baseline['source'],'label':label,'phase':phase,'modified_paths':[test],'rows':rows},indent=2,sort_keys=True)+'\n').encode(); p=out/(label+'-input-'+phase+'.json');p.write_bytes(b)
 return {'path':p.name,'bytes':len(b),'sha256':hashlib.sha256(b).hexdigest(),'entries':len(rows)}
before=snapshot('before')
env=os.environ.copy();env['PATH']='/Users/jin/.cargo/bin:'+env.get('PATH','');env['SDKROOT']='/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk';env['CARGO_TARGET_DIR']=str(root.parent/'targets/public-dependency-fix-1.95-sdk154-round13')
cmd=['/Users/jin/.cargo/bin/cargo','+1.95.0','test','--locked','-p','cc-db','--test','p2b_resolution_store','public_snapshot_dependency_failure_preserves_original_catch_and_commit_prefix','--','--exact','--nocapture']
assert Path(env['SDKROOT']).is_dir() and Path(env['CARGO_TARGET_DIR']).is_dir()
receipt={'schema':1,'label':label,'cwd':str(root),'source_G':baseline['source'],'command':cmd,'expected_outcome':'actual regression assertion: public snapshot64 versus original per-row70, not a build/preparation error','scope':'ordinary engineering regression, cached target cloned with cp -cR; no cold or performance claim','environment':{k:env.get(k) for k in ['SDKROOT','CARGO_TARGET_DIR','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS','CC','CXX']},'before':before,'started_at':datetime.datetime.now(datetime.timezone.utc).isoformat()}
start=time.monotonic();sp=out/(label+'.stdout');ep=out/(label+'.stderr');rp=out/(label+'-receipt.json')
with sp.open('wb') as so,ep.open('wb') as se:
 p=subprocess.Popen(cmd,cwd=str(root),env=env,stdout=so,stderr=se);receipt['pid']=p.pid;rp.write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps({'started':label,'pid':p.pid,'command':cmd}),flush=True);code=p.wait()
receipt.update(exit_code=code,elapsed_seconds=time.monotonic()-start,ended_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),after=snapshot('after'))
for stream,path in [('stdout',sp),('stderr',ep)]:
 data=path.read_bytes();receipt[stream]={'path':path.name,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()};print(stream.upper()+'_TAIL\n'+data.decode(errors='replace')[-14000:])
rp.write_text(json.dumps(receipt,indent=2)+'\n');print('RECEIPT '+json.dumps(receipt,sort_keys=True),flush=True)
sys.exit(code)
