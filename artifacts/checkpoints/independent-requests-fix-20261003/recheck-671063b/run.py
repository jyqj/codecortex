"""Build each version in an absent, dedicated target; execute exact cargo artifact."""
import hashlib, json, os, pathlib, subprocess, time
ROOT = pathlib.Path(__file__).resolve().parent
VERSIONS = ('old', 'rejected', 'repaired')
base_env = dict(os.environ, CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup', CARGO_BUILD_JOBS='2')
base_env['PATH'] = '/workspace/.cargo/bin:' + base_env.get('PATH','')
receipt = {'rustc': subprocess.check_output(['rustc','--version'],env=base_env).decode().strip(), 'versions': {}}
jobs = {}
for label in VERSIONS:
    target = ROOT/'targets'/label
    assert not target.exists(), f'build identity requires absent target: {target}'
    manifest = ROOT/f'harness-{label}/Cargo.toml'
    env = dict(base_env, CARGO_TARGET_DIR=str(target))
    lock = ['cargo','generate-lockfile','--offline','--manifest-path',str(manifest)]
    with (ROOT/f'lock-{label}.log').open('w') as log:
        subprocess.run(lock,env=env,stdout=log,stderr=log,check=True)
    command = ['cargo','build','--offline','--locked','--message-format=json-render-diagnostics','--manifest-path',str(manifest)]
    out = (ROOT/f'build-{label}.jsonl').open('w')
    err = (ROOT/f'build-{label}.stderr').open('w')
    process = subprocess.Popen(command,env=env,stdout=out,stderr=err)
    jobs[label] = (process,out,err,env)
    receipt['versions'][label] = {'command':command,'target_dir':str(target),'target_absent_before_build':True,'source_sha':json.loads((ROOT/'source-manifest.json').read_text())[label]['sha']}
    print(f'started independent {label} build',flush=True)
while jobs:
    for label,(process,out,err,env) in list(jobs.items()):
        code = process.poll()
        if code is None: continue
        out.close();err.close()
        data = receipt['versions'][label]
        data['build_exit_code'] = code
        (ROOT/'build-execution-receipt.json').write_text(json.dumps(receipt,indent=2))
        assert code == 0, f'{label} build failed: {code}'
        artifacts = [json.loads(line) for line in (ROOT/f'build-{label}.jsonl').read_text().splitlines()]
        artifact = next(x for x in artifacts if x.get('reason')=='compiler-artifact' and x.get('executable') and x['target']['name']==f'independent-requests-recheck-{label}')
        binary = pathlib.Path(artifact['executable'])
        assert binary.resolve().is_relative_to(ROOT/'targets'/label)
        assert artifact['fresh'] is False
        data.update(executable=str(binary),artifact_fresh=artifact['fresh'],binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),harness_sha256=hashlib.sha256((ROOT/f'harness-{label}/src/main.rs').read_bytes()).hexdigest(),lock_sha256=hashlib.sha256((ROOT/f'harness-{label}/Cargo.lock').read_bytes()).hexdigest())
        runs = []
        for mode in ('controls','legal-identifiers'):
            args = [str(binary),label,str(ROOT/f'{label}-{mode}')]
            if mode == 'legal-identifiers': args += [mode]
            with (ROOT/f'run-{label}-{mode}.log').open('w') as log:
                result = subprocess.run(args,env=env,stdout=log,stderr=log)
            runs.append({'command':args,'exit_code':result.returncode})
            assert result.returncode == 0, f'{label}/{mode} failed'
        assert hashlib.sha256(binary.read_bytes()).hexdigest()==data['binary_sha256']
        data['runs'] = runs
        (ROOT/'build-execution-receipt.json').write_text(json.dumps(receipt,indent=2))
        del jobs[label]
        print(f'completed independent {label} build and both test modes',flush=True)
    if jobs: time.sleep(1)
