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
    p.add_argument('--runner', type=Path, help='accepted cc-eval binary for validate only; NEVER freeze/rewrite gold')
    p.add_argument('--workspace-root', type=Path, required=True, help='original workspace holding immutable original-51 suite closures')
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
        {'id':'intent_aware_facet_reservation','path':'crates/cc-server/src/engine.rs','on_text':'cc_search::selection::coverage::select_with_query(\n            &hits,\n            detected_intent,\n            top_k,\n            query,\n        )?','off_text':'cc_search::selection::coverage::select_with_query(\n            &hits,\n            cc_model::Intent::Locate,\n            top_k,\n            query,\n        )?'},
    ]
    for c in controls:
        if (ref/c['path']).read_text().count(c['on_text']) != 1: raise ValueError('control not unique: '+c['id'])
    workspace=a.workspace_root.resolve()
    suites=[('smoke',workspace/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-smoke/suite.json'),
            ('exact',workspace/'crates/cc-eval/benchmarks/manifests/p1b-exact.json'),
            ('intents',workspace/'crates/cc-eval/benchmarks/manifests/p1c-intents.json'),
            ('source',workspace/'artifacts/benchmarks/p0-g0-20260927/frozen-inputs/p0-codecortex-subset/suite.json')]
    if a.runner:
        subprocess.run([str(a.runner.resolve()),'validate',*[x for _,suite in suites for x in ('--suite',str(suite))]],check=True)
    input_locks=[]
    for ident,suite in suites:
        metadata=json.loads(suite.read_text());query=(suite.parent/metadata['queries']).resolve();corpus=(suite.parent/metadata['source']['root']).resolve()
        original_rows=[json.loads(line) for line in query.read_text().splitlines() if line.strip()]
        input_locks.append({'id':ident,'suite':str(suite),'suite_sha256':sha(suite),'query_sha256':sha(query),'source_files_sha256':{path:sha(corpus/path) for path in metadata['source']['files']},'queries':len(original_rows),'families':sorted(set(row['query_family'] for row in original_rows)),'repetitions':metadata['repetitions']})
    assert sum(row['queries'] for row in input_locks)==51
    write(out/'original-51-input-lock.json',{'scope':'exact immutable original P0 smoke/source + locked exact/intents;NO current-source relock','datasets':input_locks,'queries':51,'requests_per_cell':153,'total_matrix_requests':1224})
    original=inventory(ref); write(out/'reference-source.json',original)
    env=os.environ.copy(); env['SDKROOT']=str(a.sdkroot.resolve());env['CARGO_BUILD_JOBS']='2';env['CARGO_TARGET_DIR']='not_used_until_fresh_per_celltarget'
    env['RUSTFLAGS']='-C link-arg=-isysroot -C link-arg='+env['SDKROOT'];env['RUSTDOCFLAGS']=env['RUSTFLAGS']
    options={'command':['cargo','+stable','build','--offline','--locked','--release','-p','cc-server','--bin','codecortex'],'rustc':subprocess.check_output(['rustc','+stable','-Vv'],text=True),'cargo':subprocess.check_output(['cargo','+stable','-V'],text=True),'SDKROOT':env['SDKROOT'],'RUSTFLAGS':env['RUSTFLAGS'],'profile':'release','features':'default','jobs':2}
    variants=[]
    for bits in itertools.product([False,True], repeat=3):
        enabled=[c['id'] for c,on in zip(controls,bits) if on];vid='cell_'+''.join('1' if x else '0' for x in bits)
        root=out/'sources'/vid;shutil.copytree(ref,root)
        for c,on in zip(controls,bits):
            if not on:
                f=root/c['path'];f.write_text(f.read_text().replace(c['on_text'],c['off_text'],1))
        v={'id':vid,'enabled':enabled,'source_root':str(root.relative_to(out)), 'binary':f'binaries/{vid}', 'build_receipt':f'receipts/{vid}.json'};variants.append(v)
        if a.build:
            cell_target=out/'targets'/vid
            if cell_target.exists(): raise ValueError('per-celltarget must befresh: '+str(cell_target))
            env['CARGO_TARGET_DIR']=str(cell_target)
            cell_options={**options,'CARGO_TARGET_DIR':str(cell_target),'binding':'fresh unique per-celltarget;runtimefactorwitness requiredbeforemeasurement'}
            log=out/f'{vid}-build.log';started=time.time()
            with log.open('w') as stream: r=subprocess.run(options['command'],cwd=root,env=env,stdout=stream,stderr=subprocess.STDOUT)
            receipt={'source_files':inventory(root),'binary_sha256':'unavailable','build_options':cell_options,'exit_code':r.returncode}
            if r.returncode==0:
                binary=out/v['binary'];binary.parent.mkdir(exist_ok=True);shutil.copy2(cell_target/'release/codecortex',binary);receipt['binary_sha256']=sha(binary)
            (out/'receipts').mkdir(exist_ok=True);write(out/v['build_receipt'],receipt)
            write(out/f'{vid}-timing.json',{'started_epoch':started,'elapsed_s':time.time()-started,'exit_code':r.returncode,'log_sha256':sha(log)})
            if r.returncode: raise RuntimeError('build failure, raw retained')
    datasets=[{'id':ident,'suite':str(suite),'hints':{}} for ident,suite in suites]
    write(out/'plan.json',{'schema_version':1,'reference_source':'reference','controls':controls,'variants':variants,'datasets':datasets,'seed':1905})
    write(out/'preparation.json',{'status':'built_not_measured' if a.build else 'prepared_not_built','reference_files':len(original),'source_root':str(source),'limits':['intent-aware facet reservation vs Locate-policy control;Locate default implementation anchor/query-aware source support/overlap/provenance retained','existing dev questions are not holdout','counterfactual no-lane inputs remain identical; missing facets not excused','existing Partial/S11 gates retained; no release claim']})
if __name__=='__main__': main()
