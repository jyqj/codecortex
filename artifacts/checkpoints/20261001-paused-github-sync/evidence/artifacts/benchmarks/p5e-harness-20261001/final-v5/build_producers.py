#!/usr/bin/env python3
import hashlib,json,os,pathlib,shutil,subprocess,time
root=pathlib.Path.cwd();candidate=root/'artifacts/benchmarks/p5e-candidate-release-20261001-v2';harness=root/'artifacts/benchmarks/p5e-harness-20261001/final-v5';manifest=json.load(open(candidate/'source-manifest.json'))
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def verify():
 for f in manifest['files']:
  p=candidate/'source'/f['path'];assert p.is_file() and not p.is_symlink() and p.stat().st_size==f['bytes'] and sha(p)==f['sha256'],f['path']
 return {'files':len(manifest['files']),'canonical_source_sha256':manifest['source_digest_sha256'],'all_manifest_files_unchanged':True}
verify();baseline=json.load(open(root/'artifacts/benchmarks/p5e-baseline-release-20261001/BUILD-RECEIPT.json'))
env=os.environ.copy();env.update(SDKROOT=baseline['options']['SDKROOT'],RUSTFLAGS=baseline['options']['RUSTFLAGS'],CARGO_BUILD_JOBS='2',CARGO_TARGET_DIR='/tmp/p5e-candidate-release-target-20261001')
rustc=subprocess.check_output(['rustc','+stable','-vV'],text=True);cargo=subprocess.check_output(['cargo','+stable','-V'],text=True);assert rustc==baseline['rustc'] and cargo==baseline['cargo']
for name,folder,argv,binary in [('candidate',candidate,['-p','cc-server','--bin','codecortex','--manifest-path',str(candidate/'source/Cargo.toml')],'codecortex'),('harness',harness,['--manifest-path',str(harness/'driver/Cargo.toml')],'p5e-evidence-driver')]:
 receipt=folder/'BUILD-RECEIPT.json';assert not receipt.exists();log=folder/'build.log';assert not log.exists();cmd=['cargo','+stable','build','--offline','--locked','--release']+argv
 started=time.time();print('START',name,cmd,flush=True)
 with log.open('w') as stream:rc=subprocess.call(cmd,env=env,stdout=stream,stderr=subprocess.STDOUT)
 closure=verify();output=folder/'binaries'/binary
 if rc==0:
  output.parent.mkdir(exist_ok=True);assert not output.exists();shutil.copy2(pathlib.Path(env['CARGO_TARGET_DIR'])/'release'/binary,output);output.chmod(0o555)
 payload={'status':'immutable_release_build_passed_not_formal_runs' if rc==0 else 'build_failed','exit_code':rc,'source_sha256':manifest['source_digest_sha256'],'source_postcheck':closure,'build_argv':cmd,'options':{k:env[k] for k in ['SDKROOT','RUSTFLAGS','CARGO_BUILD_JOBS','CARGO_TARGET_DIR']},'profile':'release','features':'default','rustc':rustc,'cargo':cargo,'elapsed_seconds':time.time()-started,'log_sha256':sha(log),'binary':str(output.relative_to(root)) if rc==0 else None,'binary_sha256':sha(output) if rc==0 else None,'limits':['incremental reuse target,not cold build certification','immutable old binaries unchanged','no quality or performance run','harness driver closure separately final manifest locked']}
 receipt.write_text(json.dumps(payload,indent=2)+'\n');print('TERMINAL',name,rc,payload['binary_sha256'],flush=True)
 if rc:raise SystemExit(rc)
