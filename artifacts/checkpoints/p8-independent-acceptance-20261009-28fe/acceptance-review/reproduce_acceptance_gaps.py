#!/usr/bin/env python3
"""Synthetic contract-only negative controls; never product acceptance evidence."""
import contextlib, copy, io, json, pathlib, sys
repo=pathlib.Path('/workspace/scratch/28fef0db5e01/codecortex')
sys.path[:0]=[str(repo/'scripts'),str(repo/'scripts/tests')]
import p8_cold_build as cold
import p8_rollback as rollback
from test_p8_platform import PlatformControls

results={'source_commit':'5610335b31f0f866cfa17712c8a55b9b22dac4cf', 'scope':'synthetic isolated contract negative controls; no product performance or platform evidence', 'cases':[]}
case=PlatformControls()
case.setUp()
try:
    bundles=cold.new_directory(case.base/'review-bundles')
    destinations=[]
    for os_name in cold.PLATFORMS:
        for tc in cold.TOOLCHAINS:
            for package in cold.PACKAGES:
                destinations.append(case.bundle(bundles,dict(platform=os_name,toolchain=tc,package=package)))
    commit=case.git('rev-parse','HEAD')
    baseline=cold.collect_cells(bundles,case.root,commit)
    results['baseline_counts']=baseline['counts']
    destination=destinations[0]
    rp=destination/'receipt.json'
    original=rp.read_bytes()
    original_record=json.loads(original)
    original_source_receipt=pathlib.Path(original_record['cargo_artifact']['executable']).parents[3]/'receipt.json'
    def reset_bundle():
        bundle=json.loads((destination/'bundle.json').read_text())
        bundle['files']={p.relative_to(destination).as_posix():cold.digest(p) for p in destination.rglob('*') if p.is_file() and p.name!='bundle.json'}
        bundle['receipt_sha256']=cold.digest(rp)
        cold.write_json(destination/'bundle.json',bundle)
    def check(name, mutate):
        record=json.loads(original)
        mutate(record)
        cold.write_json(rp,record)
        cold.write_json(original_source_receipt,record)
        reset_bundle()
        row={'id':name}
        try:cold.validate_cell(original_source_receipt);row['local_validator']='accepted'
        except Exception as e:row['local_validator']=type(e).__name__+': '+str(e)
        try:
            result=cold.collect_cells(bundles,case.root,commit)
            row['exported_collector']={'status':result['status'],'counts':result['counts']}
        except Exception as e:row['exported_collector']=type(e).__name__+': '+str(e)
        results['cases'].append(row)
        rp.write_bytes(original);original_source_receipt.write_bytes(original);reset_bundle()
    check('cold_profile_lie',lambda r:r.update(profile='release'))
    check('cold_compiler_invocation_unbound',lambda r:r['command'].__setitem__(0,'not-the-recorded-cargo'))
    check('cold_compiler_environment_unbound',lambda r:r.update(compiler_environment={'RUSTC':'wrong-rustc'}))
    check('cold_missing_log_bindings',lambda r:r.update(logs_sha256={}))
    empty=cold.new_directory(case.base/'empty-bundles')
    failure_out=case.base/'failed-collection'
    stderr=io.StringIO()
    with contextlib.redirect_stderr(stderr):
        code=cold.main(['--source-root',str(case.root),'--collect-cells',str(empty),'--expected-commit',commit,'--output-dir',str(failure_out)])
    results['cases'].append({'id':'cold_failed_collection_does_not_retain_receipt','exit_code':code,'output_files':[str(p.relative_to(failure_out)) for p in failure_out.rglob('*')],'stderr':stderr.getvalue()})
finally:
    case.doCleanups()
case=PlatformControls()
case.setUp()
try:
    source=case.root/'crates/cc-db/src/index_migrate.rs';source.parent.mkdir(parents=True)
    source.write_text('pub const CURRENT_SCHEMA_VERSION: u32 = 25;\n')
    case.git('add','.');case.git('commit','-qm','add genuine fixture schema declaration')
    receipt,record=case.build_receipt()
    identity=rollback.binary_identity(record['cargo_artifact']['executable'],receipt,'default')
    row={'id':'rollback_cold_receipt_manifest_compatibility','binary_identity':'accepted','manifest_type':type(json.loads(pathlib.Path(identity['source_manifest_path']).read_text())).__name__}
    try:row['product_schema_version']=rollback.product_schema_version(identity)
    except Exception as e:row['product_schema_version']=type(e).__name__+': '+str(e)
    results['cases'].append(row)
finally:
    case.doCleanups()
print(json.dumps(results,ensure_ascii=False,indent=2))
