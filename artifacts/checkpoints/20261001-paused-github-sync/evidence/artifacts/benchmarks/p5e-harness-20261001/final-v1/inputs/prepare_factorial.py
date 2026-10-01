#!/usr/bin/env python3
"""Build isolated path/exact/facet-reservation cells AFTER owner accepts a freeze.
Never touches product source. All builds sequential, full raw quality gates retained.
"""
import argparse, hashlib, itertools, json, os, shutil, subprocess, time
from pathlib import Path

def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def inventory(root): return {p.relative_to(root).as_posix():sha(p) for p in sorted(root.rglob('*')) if p.is_file()}
def write(path, value): path.write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--accepted-source', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--sdkroot', type=Path, required=True)
    p.add_argument('--runner', type=Path, help='accepted cc-eval binary; re-lock cloned current-source suite only')
    p.add_argument('--build', action='store_true', help='explicit costly sequential 8 release builds')
    a=p.parse_args(); source=a.accepted_source.resolve(); out=a.output.resolve()
    out.mkdir(parents=True,exist_ok=False)
    ref=out/'reference';ref.mkdir()
    # Minimal complete Cargo source closure; no workspace target or private config.
    for name in ['Cargo.toml','Cargo.lock','crates']:
        src=source/name
        if src.is_symlink() or (src.is_dir() and any(x.is_symlink() for x in src.rglob('*'))):
            raise ValueError('symlink in source closure: '+str(src))
        if src.is_dir(): shutil.copytree(src,ref/name)
        else: shutil.copy2(src,ref/name)
    if (source/'.cargo').exists():
        config=source/'.cargo'
        if config.is_symlink() or any(x.is_symlink() for x in config.rglob('*')): raise ValueError('symlink in Cargo config closure')
        shutil.copytree(config,ref/'.cargo')
    for x in ref.rglob('*'):
        if x.is_symlink(): raise ValueError('source symlink')
    controls=[
        {'id':'path','path':'crates/cc-search/src/lanes/path.rs','on_text':'context.config.path_weight > 0.0','off_text':'false /* P5E isolated path-lane-off */'},
        {'id':'exact','path':'crates/cc-search/src/lanes/exact_symbol.rs','on_text':'context.config.exact_symbol_weight > 0.0','off_text':'false /* P5E isolated exact-lane-off */'},
        {'id':'facet_selector','path':'crates/cc-server/src/engine.rs','on_text':'cc_search::selection::coverage::select(&hits, detected_intent, top_k)?','off_text':'cc_search::selection::coverage::select(&hits, cc_model::Intent::Locate, top_k)?'},
    ]
    for c in controls:
        if (ref/c['path']).read_text().count(c['on_text']) != 1: raise ValueError('control not unique: '+c['id'])
    if a.runner:
        suite=ref/'crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json'
        q=ref/'crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl'; old_gold=sha(q)
        old_suite=json.loads(suite.read_text())
        subprocess.run([str(a.runner.resolve()),'freeze','--suite',str(suite)],check=True)
        new_suite=json.loads(suite.read_text())
        assert sha(q)==old_gold and new_suite['queries_digest']==old_suite['queries_digest'], 'gold must stay unchanged'
        write(out/'source-suite-relock.json',{'before':old_suite,'after':new_suite,'gold_sha256':old_gold,'scope':'cloned current source manifest only; no gold/scoring change'})
    original=inventory(ref); write(out/'reference-source.json',original)
    env=os.environ.copy(); env['SDKROOT']=str(a.sdkroot.resolve());env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']=str(out/'target')
    env['RUSTFLAGS']='-C link-arg=-isysroot -C link-arg='+env['SDKROOT'];env['RUSTDOCFLAGS']=env['RUSTFLAGS']
    options={'command':['cargo','build','--locked','--release','-p','cc-server','--bin','codecortex'],'SDKROOT':env['SDKROOT'],'RUSTFLAGS':env['RUSTFLAGS'],'profile':'release','features':'default','jobs':2}
    variants=[]
    for bits in itertools.product([False,True], repeat=3):
        enabled=[c['id'] for c,on in zip(controls,bits) if on];vid='cell_'+''.join('1' if x else '0' for x in bits)
        root=out/'sources'/vid;shutil.copytree(ref,root)
        for c,on in zip(controls,bits):
            if not on:
                f=root/c['path'];f.write_text(f.read_text().replace(c['on_text'],c['off_text'],1))
        v={'id':vid,'enabled':enabled,'source_root':str(root.relative_to(out)), 'binary':f'binaries/{vid}', 'build_receipt':f'receipts/{vid}.json'};variants.append(v)
        if a.build:
            log=out/f'{vid}-build.log';started=time.time()
            with log.open('w') as stream: r=subprocess.run(options['command'],cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT)
            receipt={'source_files':inventory(root),'binary_sha256':'unavailable','build_options':options,'exit_code':r.returncode}
            if r.returncode==0:
                binary=out/v['binary'];binary.parent.mkdir(exist_ok=True);shutil.copy2(out/'target/release/codecortex',binary);receipt['binary_sha256']=sha(binary)
            (out/'receipts').mkdir(exist_ok=True);write(out/v['build_receipt'],receipt)
            write(out/f'{vid}-timing.json',{'started_epoch':started,'elapsed_s':time.time()-started,'exit_code':r.returncode,'log_sha256':sha(log)})
            if r.returncode: raise RuntimeError('build failure, raw retained')
    datasets=[{'id':id,'suite':str((ref/'crates/cc-eval/benchmarks/manifests'/name).relative_to(out)), 'hints':{}} for id,name in [('smoke','p0-smoke.json'),('exact','p1b-exact.json'),('intents','p1c-intents.json'),('source','p0-codecortex-subset.json')]]
    write(out/'plan.json',{'schema_version':1,'reference_source':'reference','controls':controls,'variants':variants,'datasets':datasets,'seed':1905})
    write(out/'preparation.json',{'status':'built_not_measured' if a.build else 'prepared_not_built','reference_files':len(original),'source_root':str(source),'limits':['facet_selector disables facet reservation only; overlap/provenance validation retained','existing dev questions are not holdout','counterfactual no-lane inputs remain identical; missing facets not excused','existing Partial/S11 gates retained; no release claim']})
if __name__=='__main__': main()
