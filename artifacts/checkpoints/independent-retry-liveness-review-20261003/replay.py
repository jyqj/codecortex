#!/usr/bin/env python3
"""Only owned copies/targets, explicit roots, direct compiler, same acceptance oracle."""
import pathlib,subprocess,tarfile,io,os,json,hashlib,sys
p=pathlib.Path(__file__).resolve().parent
repo=p.parents[2]
tool=pathlib.Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
sources={'old':'8c7c764ac17c5725c08ab2c22a4c2aea87ca35a2','fixed':'cec4ad8ef47c6c3fe86cae6a35e357935b94a0ec'}
results=[]
for label in sys.argv[1:] or ['old','fixed']:
 sha=sources[label];h=p/'source'/label;h.mkdir(parents=True,exist_ok=True)
 raw=subprocess.check_output(['git','-C',str(repo),'archive',sha,'Cargo.toml','Cargo.lock','crates'])
 with tarfile.open(fileobj=io.BytesIO(raw)) as a:a.extractall(h,filter='data')
 r=h/'crates/cc-server/src/semantic_runtime.rs'; original=r.read_bytes();r.write_bytes(original+b'\n#[cfg(all(test, feature="semantic-http"))]\n#[path="fresh_retry_oracle.rs"]\nmod fresh_retry_oracle;\n')
 (r.parent/'fresh_retry_oracle.rs').write_bytes((p/'oracle.rs').read_bytes())
 env=os.environ.copy();env.update(CARGO_HOME='/workspace/.cargo',RUSTC=str(tool/'rustc'),RUSTDOC=str(tool/'rustdoc'),CARGO_TARGET_DIR=str(p/'target'/label),CODECORTEX_SEMANTIC_CACHE_ROOT=str(p/'cache'/label),INDEPENDENT_FIXTURE_ROOT=str(pathlib.Path('/tmp/independent-retry-liveness-review-20261003')/label/'fixtures'))
 cmd=[str(tool/'cargo'),'test','--locked','--offline','--manifest-path',str(h/'Cargo.toml'),'-p','cc-server','--features','semantic-http','--lib','fresh_retry_oracle::','--','--nocapture','--test-threads=1']
 with (p/f'{label}-run.log').open('w') as f: result=subprocess.run(cmd,env=env,stdout=f,stderr=subprocess.STDOUT)
 binaries=[x for x in (p/'target'/label/'debug/deps').glob('cc_server-*') if x.is_file() and os.access(x,os.X_OK) and x.suffix!='.d']
 identity={'source':sha,'label':label,'command':cmd,'exit_code':result.returncode,'features':['semantic-http','semantic'],'compiler':subprocess.check_output([str(tool/'rustc'),'-Vv'],text=True),'cargo':subprocess.check_output([str(tool/'cargo'),'-V'],text=True),'oracle_sha256':hashlib.sha256((r.parent/'fresh_retry_oracle.rs').read_bytes()).hexdigest(),'runtime_original_sha256':hashlib.sha256(original).hexdigest(),'runtime_copy_sha256':hashlib.sha256(r.read_bytes()).hexdigest(),'binaries':{str(x.relative_to(p)):hashlib.sha256(x.read_bytes()).hexdigest() for x in binaries},'source_hashes':{str(x.relative_to(h)):hashlib.sha256(x.read_bytes()).hexdigest() for x in h.rglob('*') if x.is_file()},'environment':{k:env[k] for k in ['CARGO_HOME','RUSTC','RUSTDOC','CARGO_TARGET_DIR','CODECORTEX_SEMANTIC_CACHE_ROOT','INDEPENDENT_FIXTURE_ROOT']}}
 (p/f'{label}-identity.json').write_text(json.dumps(identity,indent=2)+'\n');print(label,result.returncode,flush=True)

 results.append(result.returncode)
raise SystemExit(next((code for code in results if code),0))
