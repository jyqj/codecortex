#!/usr/bin/env python3
"""Prepare only fixed source and original gate evidence in one new owned directory."""
from pathlib import Path
import base64
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

OWNED = Path('/Users/jin/Desktop/codecortex-rust/artifacts/checkpoints/p8-round19-evidence-intake-20261009/gates-lifecycle275e-author')
PARENT = Path('/Users/jin/Desktop/codecortex-rust')
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
TREE = '5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46'
MANIFEST = '593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83'
RUN, JOB, ARTIFACT = 37890756917, 113690916001, 11598517678
ZIP_BYTES = 33005224
ZIP_SHA = 'f2fddad2d95b53731862b87a80fbeb6dacca5da46ceb3e00f0ecebe04d1d173b'
CHECKER_BLOB = 'e44318b2054d1ef97a20ba4621b8aa765fc9c143'
CHECKER_SHA = '28e6a88b58e55c168277750410ddd7f7f1ff540a14a8303189e410eb82d556fd'
HELPER_SHA = '5003305105464936edf8f444fc0b93cd14a5d5f074dc650f16de2259e86f1cc5'
COMMANDS = []

def need(ok, message):
    if not ok:
        raise ValueError(message)

def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(block)
    return h.hexdigest()

def exclusive_json(path, value):
    with Path(path).open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write('\n')

def command(argv, cwd=None, data=None, timeout=180):
    started = time.monotonic()
    result = subprocess.run(argv, cwd=cwd, input=data, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, timeout=timeout)
    COMMANDS.append(dict(argv=argv, cwd=str(cwd) if cwd else None,
                         exit_code=result.returncode, elapsed_seconds=time.monotonic()-started,
                         stdout_bytes=len(result.stdout), stdout_sha256=hashlib.sha256(result.stdout).hexdigest(),
                         stderr=result.stderr.decode('utf-8', errors='replace')))
    need(result.returncode == 0, 'command failed: ' + argv[0])
    return result.stdout

def git(root, *args, data=None):
    return command(['git', '-c', 'core.hooksPath=/dev/null', '-c', 'core.fsmonitor=false',
                    '-C', str(root), *args], data=data)

def gh(path):
    return command(['/opt/homebrew/bin/gh', 'api', 'repos/jyqj/codecortex/' + path])

def existing_parent_state():
    git_dir = Path(git(PARENT, 'rev-parse', '--absolute-git-dir').decode().strip())
    return {str(p): digest(p) if p.is_file() else None for p in (git_dir/'HEAD', git_dir/'index')}

def main():
    need(not sys.flags.optimize, 'optimized Python is forbidden')
    need(OWNED.is_dir() and not OWNED.is_symlink(), 'explicit existing owned root')
    for ancestor in OWNED.parents:
        need(not ancestor.is_symlink(), 'aliased owned root ancestor')
    need(shutil.disk_usage(OWNED).free >= ZIP_BYTES + 64*1024**2 + 512*1024**2, 'preserved free space')
    source, out = OWNED/'source', OWNED/'gates'
    need(not source.exists() and not out.exists(), 'new source and gate directories required')
    before = existing_parent_state()
    result = dict(status='preparation_failed_or_partial', scope='fixed original evidence preparation only; receiver not executed',
                  source=SOURCE, tree=TREE, run=RUN, job=JOB, artifact=ARTIFACT,
                  active_parent_before=before, product_executed=False, cargo_executed=False)
    try:
        source.mkdir()
        command(['git', '-c', 'core.hooksPath=/dev/null', 'init', '-q', str(source)])
        common = Path(git(PARENT, 'rev-parse', '--path-format=absolute', '--git-common-dir').decode().strip())
        need((common/'objects').is_dir(), 'existing read-only object store')
        (source/'.git/objects/info/alternates').write_text(str((common/'objects').resolve())+'\n')
        git(source, 'config', 'core.autocrlf', 'false')
        git(source, 'config', 'core.filemode', 'true')
        git(source, 'sparse-checkout', 'init', '--no-cone')
        git(source, 'sparse-checkout', 'set', '--no-cone', '--stdin',
            data=b'/Cargo.toml\n/Cargo.lock\n/crates/\n/scripts/\n')
        git(source, 'checkout', '--detach', SOURCE)
        need(git(source, 'rev-parse', 'HEAD').decode().strip() == SOURCE, 'source commit')
        need(git(source, 'rev-parse', 'HEAD^{tree}').decode().strip() == TREE, 'source tree')
        need(git(source, 'status', '--porcelain').strip() == b'', 'new source checkout clean')
        helper = source/'scripts/p7_build_identity.py'
        need(digest(helper) == HELPER_SHA, 'exact source snapshot helper')
        sys.dont_write_bytecode = True
        sys.path.insert(0, str(source/'scripts'))
        from p7_build_identity import source_snapshot
        snapshot = source_snapshot(source)
        need(snapshot['source_commit'] == SOURCE and snapshot['source_tree'] == TREE
             and snapshot['input_count'] == len(snapshot['inputs']) == 1089
             and snapshot['manifest_sha256'] == MANIFEST, 'complete exact source manifest')
        out.mkdir()
        exclusive_json(out/'source-before.json', snapshot)
        paths = {'run.json':f'actions/runs/{RUN}',
                 'jobs.json':f'actions/runs/{RUN}/jobs?per_page=100&filter=all',
                 'artifacts.json':f'actions/runs/{RUN}/artifacts?per_page=100',
                 f'job-{JOB}.log':f'actions/jobs/{JOB}/logs'}
        for name, path in paths.items():
            with (out/name).open('xb') as stream:
                stream.write(gh(path))
        run = json.loads((out/'run.json').read_bytes())
        jobs = json.loads((out/'jobs.json').read_bytes())
        arts = json.loads((out/'artifacts.json').read_bytes())
        need(run['id'] == RUN and run['head_sha'] == SOURCE and run['run_attempt'] == 1
             and run['status'] == 'completed' and run['conclusion'] == 'success', 'run identity')
        need(jobs['total_count'] == len(jobs['jobs']) == 1 and jobs['jobs'][0]['id'] == JOB
             and jobs['jobs'][0]['conclusion'] == 'success', 'complete original job')
        need(arts['total_count'] == len(arts['artifacts']) == 1, 'complete original artifact population')
        a = arts['artifacts'][0]
        need(a['id'] == ARTIFACT and a['size_in_bytes'] == ZIP_BYTES and a['digest'] == 'sha256:'+ZIP_SHA
             and a['workflow_run']['id'] == RUN and a['workflow_run']['head_sha'] == SOURCE
             and not a['expired'], 'fixed original artifact')
        zip_path = out/f'artifact-{ARTIFACT}.zip'
        with zip_path.open('xb') as stream:
            result_download = subprocess.run(['/opt/homebrew/bin/gh', 'api',
                  f'repos/jyqj/codecortex/actions/artifacts/{ARTIFACT}/zip'],
                  stdout=stream, stderr=subprocess.PIPE, timeout=180)
        result['download_exit_code'] = result_download.returncode
        need(result_download.returncode == 0, 'original download failed; preserve partial bytes')
        need(zip_path.stat().st_size == ZIP_BYTES and digest(zip_path) == ZIP_SHA, 'original whole ZIP')
        j = json.loads(gh('git/blobs/'+CHECKER_BLOB))
        need(j['sha'] == CHECKER_BLOB and j['encoding'] == 'base64', 'checker Git object')
        body = base64.b64decode(j['content'])
        need(len(body) == j['size'] and hashlib.sha256(body).hexdigest() == CHECKER_SHA
             and hashlib.sha1(('blob '+str(len(body))+'\0').encode()+body).hexdigest() == CHECKER_BLOB,
             'complete fixed checker bytes')
        with (out/'inspect_original.py').open('xb') as stream:
            stream.write(body)
        after = source_snapshot(source)
        need(after == snapshot, 'source changed during preparation')
        exclusive_json(out/'source-after.json', after)
        result.update(status='prepared_for_independent_receiver_execution',
                      source_manifest_sha256=MANIFEST, source_inputs=1089,
                      source_root=str(source), checker_blob=CHECKER_BLOB,
                      checker_sha256=CHECKER_SHA, original_zip_sha256=ZIP_SHA,
                      original_zip_bytes=ZIP_BYTES)
    finally:
        result['active_parent_after'] = existing_parent_state()
        result['active_parent_unchanged'] = result['active_parent_after'] == before
        if not result['active_parent_unchanged']:
            result['status'] = 'preparation_failed_or_partial'
        result['commands'] = COMMANDS
        exclusive_json(OWNED/'gates-preparation-receipt.json', result)
    need(result['active_parent_unchanged'], 'active parent metadata changed during read-only preparation')
    print(json.dumps({k:result[k] for k in ('status','source','run','job','artifact','active_parent_unchanged')}))

if __name__ == '__main__':
    main()
