#!/usr/bin/env python3
"""Run available independent tools against fixed authored module fixtures only.

No package installation, downloads, indexed source execution, Cargo build scripts,
Python initializers or produced Rust binaries. Missing optional tools are not_run.
"""
import argparse
import hashlib
import importlib.machinery
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / 'crates/cc-eval/benchmarks/modules/p3c-fixtures.json'

def run(argv, cwd, env):
    process = subprocess.run([str(x) for x in argv], cwd=cwd, env=env, capture_output=True, text=True, timeout=90)
    return {'argv': [str(x) for x in argv], 'exit_code': process.returncode, 'stdout': process.stdout, 'stderr': process.stderr}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--toolchain', default='stable', choices=['stable','1.95.0'])
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    env = {k:v for k,v in os.environ.items() if not k.startswith(('RUSTFLAGS','RUSTDOCFLAGS','CARGO_','GO','PYTHON','CODECORTEX_'))}
    env['PATH'] = str(Path.home()/'.cargo/bin') + os.pathsep + env.get('PATH','')
    rustup = shutil.which('rustup', path=env['PATH'])
    resolved = run([rustup, 'which', '--toolchain', args.toolchain, 'rustc'],ROOT,env) if rustup else None
    rustc = resolved['stdout'].strip() if resolved and resolved['exit_code']==0 else None
    go = shutil.which('go', path=env['PATH'])
    versions = {'python': sys.version, 'rustc': run([rustc,'--version','--verbose'],ROOT,env) if rustc else None, 'go': run([go,'version'],ROOT,env) if go else None}
    data = json.loads(FIXTURES.read_text())
    rows=[]
    for case in data['cases']:
        row={'id':case['id'],'language':case['language'],'expected_files':case['expected_files']}
        with tempfile.TemporaryDirectory(prefix='cc-module-oracle-') as temp:
            root=Path(temp)
            for name,text in case['files'].items():
                relative=Path(name)
                if relative.is_absolute() or '..' in relative.parts or len(text)>1024*1024: raise ValueError('invalid fixture path/size')
                path=root/relative;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
            language=case['language']
            if language=='python':
                paths=[str(root/p) for p in case['python_roots']]
                spec=None
                # PathFinder with explicit paths does not import or execute parents.
                parts=case['python_absolute'].split('.')
                for i in range(len(parts)):
                    spec=importlib.machinery.PathFinder.find_spec('.'.join(parts[:i+1]),paths)
                    if spec is None:break
                    paths=list(spec.submodule_search_locations or [])
                actual=[str(Path(spec.origin).relative_to(root))] if spec and spec.origin else []
                row.update(actual_files=actual,status='passed' if actual==case['expected_files'] else 'failed',initializer_execution=False)
            elif language=='rust' and rustc:
                output=root/'out';output.mkdir()
                argv=[rustc,'--edition=2021','--crate-type=lib','--emit=metadata,dep-info','--out-dir',output,case['file']]
                for condition in case['rust_cfg']: argv+=['--cfg',condition]
                command=run(argv,root,env)
                depfiles=list(output.glob('*.d'))
                depinfo=depfiles[0].read_text() if depfiles else ''
                # The fixed fixture names contain no spaces, Make escapes or colons.
                actual=sorted(name for name in case['files'] if name.endswith('.rs') and (name+':') in depinfo)
                expected=sorted(case['rust_sources'])
                row.update(command=command,dep_info=depinfo,actual_sources=actual,expected_sources=expected,status='passed' if command['exit_code']==0 and actual==expected else 'failed')
            elif language=='go' and go:
                goenv=env.copy();goenv.update(GOPROXY='off',GOSUMDB='off',GOTOOLCHAIN='local',GOENV='off',CGO_ENABLED='0',GOCACHE=str(root/'cache'),GOPATH=str(root/'gopath'))
                goenv['GOWORK']=str(root/'go.work') if (root/'go.work').exists() else 'off'
                command=run([go,'list','-json',case['specifier']],root,goenv)
                actual=[]
                if command['exit_code']==0:
                    package=json.loads(command['stdout']);actual=sorted(str((Path(package['Dir'])/p).relative_to(root)) for p in package.get('GoFiles',[]))
                row.update(command=command,actual_files=actual,status='passed' if command['exit_code']==0 and actual==case['expected_files'] else 'failed')
            else:row.update(status='not_run',reason='compiler_not_available; no installation or download attempted')
        rows.append(row)
    result={'status':'failed' if any(r['status']=='failed' for r in rows) else 'passed_available_scope','fixture_sha256':hashlib.sha256(FIXTURES.read_bytes()).hexdigest(),'harness_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'toolchain':args.toolchain,'versions':versions,'cases':rows,'optional_typescript_oracle':{'status':'not_run','reason':'tsc not invoked; TypeScript semantics tested with authored gold only'},'limits':'Selected authored fixture membership only; no complete compiler, runtime, cross-platform, holdout or production certification'}
    (args.output/'result.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps({'status':result['status'],'cases':[(r['id'],r['status']) for r in rows]},indent=2))
    return int(result['status']=='failed')

if __name__=='__main__':raise SystemExit(main())
