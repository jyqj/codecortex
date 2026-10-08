#!/usr/bin/env python3
# Run from repository root. Creates only this checkpoint's disposable harness/cache.
import pathlib,subprocess,tarfile,io,os
p=pathlib.Path(__file__).resolve().parent
h=p/'harness';h.mkdir(exist_ok=True)
raw=subprocess.check_output(['git','archive','5cce6eb3af90b79c786d30f349ca6a674dfcd4a1','Cargo.toml','Cargo.lock','crates'])
with tarfile.open(fileobj=io.BytesIO(raw)) as a:a.extractall(h,filter='data')
r=h/'crates/cc-server/src/semantic_runtime.rs'
r.write_text(r.read_text()+'\n#[cfg(all(test, feature = "semantic-http"))]\n#[path = "independent_review.rs"]\nmod independent_review;\n')
(h/'crates/cc-server/src/independent_review.rs').write_bytes((p/'independent_review.rs').read_bytes())
t=pathlib.Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
env=os.environ.copy();env.update(CARGO_HOME='/workspace/.cargo',RUSTC=str(t/'rustc'),RUSTDOC=str(t/'rustdoc'),CODECORTEX_SEMANTIC_CACHE_ROOT=str(p/'cache'))
with (p/'replay-run.log').open('w') as f:
 result=subprocess.run([str(t/'cargo'),'test','--manifest-path',str(h/'Cargo.toml'),'-p','cc-server','--features','semantic-http','--lib','independent_review_','--','--nocapture','--test-threads=1'],env=env,stdout=f,stderr=subprocess.STDOUT)
raise SystemExit(result.returncode)
