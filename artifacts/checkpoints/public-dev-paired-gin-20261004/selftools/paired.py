#!/usr/bin/env python3
"""Fixed Gin public DEV pair. No implementation edits or result-dependent settings."""
import hashlib, importlib.util, json, os, shutil, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone

ROOT = Path('/workspace/codecortex')
OUT = ROOT/'artifacts/checkpoints/public-dev-paired-gin-20261004'
TMP = Path('/tmp/public-dev-paired-gin-20261004')
SHAS = {'baseline':'88f2cf099c8b81f3acef485fd5ac9b01c63ce790',
        'candidate':'37dd042eaa1209a86e0cafdcd92ae77e036e76f5'}
TOOL = Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
sha = lambda b: hashlib.sha256(b).hexdigest()
now = lambda: datetime.now(timezone.utc).isoformat()
def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, sort_keys=True)+'\n')
def git(*args):
    return subprocess.check_output(['git',*args],cwd=ROOT)
def inventory(path):
    return {p.relative_to(path).as_posix():sha(p.read_bytes()) for p in sorted(path.rglob('*')) if p.is_file()}
def environment(arm):
    env=os.environ.copy()
    env.update(RUSTUP_HOME='/workspace/.rustup', CARGO_HOME='/workspace/.cargo',
        RUSTC=str(TOOL/'rustc'), CARGO_TARGET_DIR=str(TMP/arm/'target'),
        CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0',
        CODECORTEX_BENCH_PROCESS_PROBE='0', TMPDIR=str(TMP/arm/'tmp'))
    env['PATH']=str(TOOL)+':'+env.get('PATH','')
    (TMP/arm/'tmp').mkdir(exist_ok=True)
    return env
def command(argv, cwd, env, prefix):
    prefix.parent.mkdir(parents=True,exist_ok=True)
    started=now()
    with prefix.with_suffix('.stdout').open('wb') as o, prefix.with_suffix('.stderr').open('wb') as e:
        p=subprocess.run(argv,cwd=cwd,env=env,stdout=o,stderr=e)
    receipt=dict(command=argv,cwd=str(cwd),exit_code=p.returncode,started_utc=started,finished_utc=now(),
        stdout_sha256=sha(prefix.with_suffix('.stdout').read_bytes()),stderr_sha256=sha(prefix.with_suffix('.stderr').read_bytes()))
    save(prefix.with_suffix('.receipt.json'),receipt)
    return receipt
def build(arm):
    work=TMP/arm; env=environment(arm); dest=OUT/arm/'build'
    assert not dest.exists()
    assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=work).decode().strip()==SHAS[arm]
    assert not subprocess.check_output(['git','diff','HEAD','--','crates','Cargo.toml','Cargo.lock'],cwd=work)
    argv=[str(TOOL/'cargo'),'build','-p','cc-eval','--bin','cc-eval','-p','cc-server','--bin','codecortex','--locked','--message-format=json']
    r=command(argv,work,env,dest/'cargo-build')
    names=subprocess.check_output(['git','ls-files','crates','Cargo.toml','Cargo.lock'],cwd=work).decode().splitlines()
    r.update(source_sha=SHAS[arm],source_file_sha256={n:sha((work/n).read_bytes()) for n in names},
        rustc=subprocess.check_output([str(TOOL/'rustc'),'--version']).decode().strip(),
        cargo=subprocess.check_output([str(TOOL/'cargo'),'--version']).decode().strip(),
        build_environment={k:env[k] for k in ['CARGO_TARGET_DIR','CARGO_INCREMENTAL','CARGO_PROFILE_DEV_DEBUG','CARGO_HOME','RUSTUP_HOME']},artifacts={})
    if r['exit_code']==0:
        artifacts=[json.loads(l) for l in (dest/'cargo-build.stdout').read_bytes().splitlines() if l.startswith(b'{')]
        for name in ['cc-eval','codecortex']:
            selected=[a for a in artifacts if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')==name and a.get('executable')]
            assert len(selected)==1
            a=selected[0]; assert not {'semantic','eval-http'} & set(a['features'])
            binary=TMP/arm/'binaries'/name; binary.parent.mkdir(exist_ok=True)
            shutil.copy2(a['executable'],binary)
            r['artifacts'][name]=dict(path=str(binary),sha256=sha(binary.read_bytes()),compiler_artifact=a)
    save(dest/'build-receipt.json',r)
    print(json.dumps({'stage':'build','arm':arm,'exit_code':r['exit_code']}),flush=True)
    return r
def prepare(arm):
    build=json.loads((OUT/arm/'build/build-receipt.json').read_text()); assert build['exit_code']==0
    admission=ROOT/'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003'
    spec=importlib.util.spec_from_file_location('selector',admission/'selector.py'); loader=importlib.util.module_from_spec(spec); spec.loader.exec_module(loader)
    receipt=json.loads((admission/'admission-receipt.json').read_text())
    inputs=TMP/arm/'inputs'; assert not inputs.exists(); inputs.mkdir()
    actual={}
    # Materialize complete actual corpus required by the strict PYGO loader. Only Gin is executed.
    for entry,digest in receipt['source_bytes_sha256'].items():
        raw=git('show',entry); assert sha(raw)==digest
        rel=entry.split('/public-v19/',1)[1]; p=inputs/rel; p.parent.mkdir(parents=True,exist_ok=True); p.write_bytes(raw)
        actual[entry]=p.read_bytes()
    packages=loader.load_version(version=loader.VERSION,repositories=['requests','gin'],source_bytes=actual)
    package=packages['gin']; schedule=json.loads((OUT/'original-schedule.json').read_text())
    plan=dict(arm=arm,source_sha=SHAS[arm],version=loader.VERSION,input_lock=package['input_lock'],
        admission_file_sha256=inventory(admission),original_schedule_sha256=sha((OUT/'original-schedule.json').read_bytes()),
        actual_source_sha256={e:sha(v) for e,v in actual.items()},suites=[],created_utc=now())
    for old in schedule['suite_entries']:
        mode=old['profile']; original=json.loads(package['suites'][mode]); suite=original.copy()
        assert old['suite_entry']==package['input_lock'][mode+'_suite_entry']
        assert old['suite_sha256']==sha(package['suites'][mode])
        for key in ['repetitions','warmup','top_k','timeout_ms','seed']: assert suite[key]==old[key]
        assert suite['source']==old['source_lock']
        rows=[json.loads(l) for l in package[mode].splitlines()]
        assert len(rows)==old['query_count'] and all(q['split']=='dev' for q in rows)
        (inputs/'gin'/suite['queries']).write_bytes(package[mode])
        path=inputs/'gin'/('suite-'+mode+'-dev.json'); path.write_bytes(package['suites'][mode])
        if mode=='native':
            scratch=path.with_name('digest-scratch.json'); scratch.write_bytes(package['suites'][mode])
            r=command([build['artifacts']['cc-eval']['path'],'freeze','--suite',str(scratch)],TMP/arm,environment(arm),OUT/arm/'prepare/digest')
            assert r['exit_code']==0
            frozen=json.loads(scratch.read_bytes()); suite['queries_digest']=frozen['queries_digest']
            assert frozen==suite # Only BLAKE3 queries_digest changes; source/config stay exact.
            scratch.unlink(); save(path,suite)
        validate=command([build['artifacts']['cc-eval']['path'],'validate','--suite',str(path)],TMP/arm,environment(arm),OUT/arm/'prepare'/('validate-'+mode))
        assert validate['exit_code']==0
        plan['suites'].append(dict(mode=mode,path=str(path),query_count=len(rows),scheduled_rows=len(rows)*suite['repetitions'],
            suite=suite,suite_sha256=sha(path.read_bytes()),queries_sha256=sha(package[mode]),original_suite_sha256=sha(package['suites'][mode])))
    plan['input_inventory']=inventory(inputs)
    save(OUT/arm/'plan.json',plan)
    print(json.dumps({'stage':'prepare','arm':arm,'scheduled':366}),flush=True)
def execute(arm):
    work=TMP/arm; env=environment(arm)
    plan=json.loads((OUT/arm/'plan.json').read_text()); build=json.loads((OUT/arm/'build/build-receipt.json').read_text())
    assert inventory(work/'inputs')==plan['input_inventory']
    for a in build['artifacts'].values(): assert sha(Path(a['path']).read_bytes())==a['sha256']
    for suite in plan['suites']:
        mode=suite['mode']; dest=OUT/arm/'runs'/mode; assert not dest.exists()
        r=command([build['artifacts']['cc-eval']['path'],'run','--backend','mcp-stdio','--binary',build['artifacts']['codecortex']['path'],
            '--suite',suite['path'],'--output',str(dest),'--profile','quality'],work,env,OUT/arm/'logs'/(mode+'-run'))
        before=inventory(dest)
        replay=command([build['artifacts']['cc-eval']['path'],'replay','--run',str(dest)],work,env,OUT/arm/'logs'/(mode+'-replay'))
        after=inventory(dest)
        save(OUT/arm/'logs'/(mode+'-integrity.json'),dict(run=r,replay=replay,before_replay=before,after_replay=after,byte_identical=before==after))
        print(json.dumps({'stage':'run','arm':arm,'mode':mode,'exit_code':r['exit_code'],'replay_exit_code':replay['exit_code'],'byte_identical':before==after}),flush=True)
    assert inventory(work/'inputs')==plan['input_inventory']
    for a in build['artifacts'].values(): assert sha(Path(a['path']).read_bytes())==a['sha256']
if __name__=='__main__':
    action,arm=sys.argv[1:]
    {'build':build,'prepare':prepare,'execute':execute}[action](arm)
