#!/usr/bin/env python3
"""Preserve the five pre-start errors, create their missing parent directories,
and execute the same original M6 workload arguments in separate attempt logs.
The successful original build and the ongoing backfill remain untouched.
"""
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import threading
import time

BASE = Path('/tmp/codecortex-p8-m6-28fe-20261009')
SOURCE = BASE / 'source'
OUT = BASE / 'runtime-parent-recovery'
BUILD = BASE / 'build'
HEAD = 'e95fd750d2f5052d0308c970cd5032c48b765cde'
TOOLCHAIN = '/workspace/scratch/2eaa00d0f93a/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin'
LOCK = threading.Lock()


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity():
    head = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=SOURCE).decode().strip()
    if head != HEAD:
        raise RuntimeError('Private source HEAD changed')
    files = ('scripts/p8_runtime.py', 'scripts/p8_runtime_build.py',
             'scripts/p8_backfill.py', 'scripts/p7_build_identity.py',
             'scripts/p8_cold_build.py', 'scripts/p8_rollback.py',
             'scripts/p7_stdio_build_receipt.py', 'scripts/resource_harness/__init__.py',
             'scripts/resource_harness/runtime.py')
    build_files = ('build-receipt.json', 'seal.json', 'codecortex', 'p8-oracle', 'p8-runtime-statistics')
    return dict(head=head, observers={p:digest(SOURCE/p) for p in files},
                build={p:digest(BUILD/p) for p in build_files})


def save():
    tmp = OUT / 'receipt.json.tmp'
    tmp.write_text(json.dumps(RECEIPT, indent=2) + '\n')
    tmp.replace(OUT / 'receipt.json')


def phase(name, argv):
    record = dict(name=name, argv=argv, cwd=str(SOURCE), status='running', started_at_utc=utc())
    with LOCK:
        RECEIPT['phases'].append(record)
        save()
    start = time.monotonic()
    print(json.dumps(dict(event='phase_started', name=name, at_utc=utc())), flush=True)
    path = OUT / (name + '.log')
    with path.open('xb') as log:
        child = subprocess.Popen(argv, cwd=SOURCE, env=ENV, stdout=log, stderr=subprocess.STDOUT)
        while True:
            try:
                code = child.wait(timeout=20)
                break
            except subprocess.TimeoutExpired:
                print(json.dumps(dict(event='heartbeat', name=name, elapsed_s=round(time.monotonic()-start, 3), at_utc=utc())), flush=True)
    with LOCK:
        record.update(status='completed', exit_code=code, finished_at_utc=utc(),
                      elapsed_seconds=time.monotonic()-start, log_bytes=path.stat().st_size,
                      log_sha256=digest(path))
        save()
    print(json.dumps(dict(event='phase_finished', name=name, exit_code=code)), flush=True)
    return code


def cell(name, arguments):
    binary = ['--binary', str(BUILD/'codecortex'), '--oracle', str(BUILD/'p8-oracle'),
              '--statistics', str(BUILD/'p8-runtime-statistics'),
              '--build-receipt', str(BUILD/'build-receipt.json')]
    output = str(BASE/name/'runtime')
    code = phase(name+'-run', ['python3', '-B', 'scripts/p8_runtime.py', *binary, *arguments, '--output', output])
    verified = phase(name+'-verify', ['python3', '-B', 'scripts/p8_runtime.py', 'verify',
                                    '--output', output, '--build-output', str(BUILD)])
    return code == 0 and verified == 0


OUT.mkdir(exist_ok=False)
original_raw = (BASE/'execution-receipt.json').read_bytes()
original = json.loads(original_raw)
assert original['source_before']['commit'] == HEAD
assert any(p['label']=='build' and p['exit_code']==0 for p in original['phases'])
names = ['c1', 'c4', 'c8', 'c16', 'soak']
prior = []
for name in names:
    for suffix in ['run', 'verify']:
        label = name+'-'+suffix
        r = next(p for p in original['phases'] if p['label']==label)
        raw = (BASE/(label+'.log')).read_bytes()
        assert r['status']=='completed' and r['exit_code']==2
        assert raw.decode() == "FileNotFoundError: [Errno 2] No such file or directory: '"+str(BASE/name)+"'\n"
        assert not (BASE/name).exists()
        prior.append(dict(label=label, exit_code=2, bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest()))
(OUT/'original-execution-observation.json').write_bytes(original_raw)
for name in names:
    (BASE/name).mkdir(exist_ok=False)
ENV = dict(os.environ, PATH=TOOLCHAIN+':'+os.environ['PATH'],
           CARGO_HOME='/workspace/scratch/28fef0db5e01/runtime-review/m5-local-soak/cargo-home',
           CARGO_TARGET_DIR=str(BASE/'runtime-target'), CARGO_BUILD_JOBS='2',
           CARGO_INCREMENTAL='0', CARGO_TERM_COLOR='never', PYTHONDONTWRITEBYTECODE='1')
RECEIPT = dict(schema_version=1, status='running', started_at_utc=utc(),
               kind='M6_same_argv_after_output_parent_preparation', source_and_build_before=identity(),
               original_observation_sha256=digest(OUT/'original-execution-observation.json'),
               original_failed_pre_start_phases=prior, created_parent_directories=[str(BASE/n) for n in names],
               original_wrapper_sha256=original['wrapper_sha256'], wrapper_sha256=digest(__file__), phases=[],
               scope='Five workloads with unchanged argv/source/build after parent-directory preparation. Original errors stay failed. Backfill remains its separate original attempt. Same local host, no isolated-host performance claim.',
               original_tasks_completed=0, remaining_original_tasks=29)
save()
plans = [('c'+str(c), ['--concurrency',str(c),'--operations','900','--files','1000','--interval-ms','500']) for c in (1,4,8,16)]
plans.append(('soak',['--profile','soak','--concurrency','4','--operations','3601','--files','1000','--interval-ms','1000']))
with concurrent.futures.ThreadPoolExecutor(max_workers=5) as pool:
    futures = {pool.submit(cell, n, a):n for n,a in plans}
    outcomes = {}
    for future in concurrent.futures.as_completed(futures):
        outcomes[futures[future]] = future.result()
        with LOCK:
            RECEIPT['cell_outcomes'] = dict(outcomes)
            save()
after = identity()
RECEIPT.update(status='passed' if all(outcomes.values()) and after==RECEIPT['source_and_build_before'] else 'failed',
               finished_at_utc=utc(), source_and_build_after=after,
               source_and_build_unchanged=after==RECEIPT['source_and_build_before'])
save()
print(json.dumps(dict(event='complete', status=RECEIPT['status'], outcomes=outcomes)), flush=True)
raise SystemExit(0 if RECEIPT['status']=='passed' else 1)
