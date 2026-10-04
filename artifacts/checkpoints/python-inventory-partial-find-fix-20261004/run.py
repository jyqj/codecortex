#!/usr/bin/env python3
"""Generate only an independent test target; copy the subject byte-for-byte.
No product source edits. Run only review_ tests, with original locked deps.
"""
import pathlib, os, subprocess, hashlib, json, shutil
root=pathlib.Path(__file__).resolve().parents[3]
out=pathlib.Path(__file__).resolve().parent
src=root/'crates/cc-index/src/project_model'
target=root/'crates/cc-index/tests/independent_inventory_review.rs'
subject=root/'crates/cc-index/tests/independent_inventory_subject'
subject.mkdir(exist_ok=True)
for name in ['native.rs','tests.rs']:
    shutil.copyfile(src/'python_inventory'/name,subject/name)
original=(src/'python_inventory.rs').read_bytes()
# Rust module directory is tests/independent_inventory_subject because of this filename.
(subject.parent/'independent_inventory_subject.rs').write_bytes(original+b'\n#[cfg(test)] mod independent { include!("'+str(out/'review_tests.rs').encode()+b'"); }\n')
wrapper='#![allow(dead_code)]\nuse cc_index::project_model::potential_config_path;\n'
for name in ['python','config_cache','jsonc','package','go']:
    wrapper+=f'#[path = "{src/name}.rs"] mod {name};\n'
wrapper+='mod independent_inventory_subject;\n'
target.write_text(wrapper)
env=dict(os.environ,RUSTUP_HOME='/workspace/.rustup',CARGO_HOME='/workspace/.cargo',CARGO_TARGET_DIR='/workspace/inventory-review-target')
env['PATH']='/workspace/.cargo/bin:'+env['PATH']
cmd=['cargo','+1.95.0','test','--locked','-p','cc-index','--test','independent_inventory_review','review_','--','--nocapture']
try:
    p=subprocess.run(cmd,cwd=root,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
    (out/'tests.log').write_text(p.stdout.rstrip('\n')+'\n')
    hashes={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [src/'python_inventory.rs',src/'python_inventory/native.rs',src/'python.rs',src/'config_cache.rs',root/'Cargo.lock',out/'review_tests.rs']}
    (out/'validation.json').write_text(json.dumps({'subject_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'subject_status':subprocess.check_output(['git','status','--short'],cwd=root,text=True),'argv':cmd,'exit':p.returncode,'sha256':hashes},indent=2)+'\n')
    print(p.stdout[-14000:]);raise SystemExit(p.returncode)
finally:
    target.unlink(missing_ok=True);(subject.parent/'independent_inventory_subject.rs').unlink(missing_ok=True);shutil.rmtree(subject)
