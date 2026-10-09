#!/usr/bin/env python3
"""Run the unchanged M6 runtime workloads in a private local checkout.

One fresh native release build is shared by the five runtime observations.
Backfill retains its own fresh target and original fake-provider worker plan.
This wrapper records execution; it does not change any product validator.
"""
import concurrent.futures
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import threading
import time

WORK = Path('/tmp/codecortex-p8-m6-28fe-20261009')
SOURCE = WORK / 'source'
HEAD = 'e95fd750d2f5052d0308c970cd5032c48b765cde'
TREE = '7fa242edeb826c574d57a43b89f140d03496a9f8'
TOOLCHAIN = Path('/workspace/scratch/2eaa00d0f93a/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
CACHE = Path('/workspace/scratch/28fef0db5e01/runtime-review/m5-local-soak/cargo-home')
INPUTS = ('scripts/p8_runtime.py', 'scripts/p8_runtime_build.py',
          'scripts/p8_backfill.py', 'scripts/p7_build_identity.py',
          'scripts/p8_cold_build.py', 'scripts/p8_rollback.py',
          'scripts/p7_stdio_build_receipt.py',
          'scripts/resource_harness/__init__.py',
          'scripts/resource_harness/runtime.py',
          'docs/roadmap/code-index-v2/tasks.json')
LOCK = threading.Lock()
CHILDREN = {}


def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def digest(path):
    h = hashlib.sha256()
    with Path(path).open('rb') as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()


def identity():
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=SOURCE).decode().strip()
    tree = subprocess.check_output(['git', 'rev-parse', 'HEAD^{tree}'], cwd=SOURCE).decode().strip()
    if (commit, tree) != (HEAD, TREE):
        raise RuntimeError('Private source identity changed')
    return dict(commit=commit, tree=tree, files={p: digest(SOURCE / p) for p in INPUTS})


def save():
    tmp = WORK / 'execution-receipt.json.tmp'
    tmp.write_text(json.dumps(RECEIPT, indent=2) + '\n')
    tmp.replace(WORK / 'execution-receipt.json')


def execute(label, argv, environment):
    before = identity()
    if before != RECEIPT['source_before']:
        raise RuntimeError('Private source bytes changed before ' + label)
    phase = dict(label=label, argv=argv, cwd=str(SOURCE), started_at_utc=utc(),
                 source_before=before, status='running')
    log_path = WORK / (label + '.log')
    started = time.monotonic()
    with LOCK:
        RECEIPT['phases'].append(phase)
        save()
    print(json.dumps(dict(event='phase_started', label=label, at_utc=utc())), flush=True)
    with log_path.open('xb') as log:
        child = subprocess.Popen(argv, cwd=SOURCE, env=environment, stdout=log, stderr=subprocess.STDOUT)
        with LOCK:
            CHILDREN[label] = child
        while True:
            try:
                code = child.wait(timeout=20)
                break
            except subprocess.TimeoutExpired:
                print(json.dumps(dict(event='heartbeat', label=label, elapsed_s=round(time.monotonic()-started, 3), at_utc=utc())), flush=True)
    after = identity()
    phase.update(status='completed', exit_code=code, finished_at_utc=utc(),
                 elapsed_seconds=time.monotonic()-started, source_after=after,
                 source_unchanged=after == before, log=str(log_path),
                 log_bytes=log_path.stat().st_size, log_sha256=digest(log_path))
    with LOCK:
        CHILDREN.pop(label, None)
        save()
    print(json.dumps(dict(event='phase_finished', label=label, exit_code=code, elapsed_s=phase['elapsed_seconds'])), flush=True)
    if after != before:
        raise RuntimeError('Private source bytes changed during ' + label)
    return code


def runtime_cell(name, arguments):
    build = WORK / 'build'
    args = ['--binary', str(build / 'codecortex'), '--oracle', str(build / 'p8-oracle'),
            '--statistics', str(build / 'p8-runtime-statistics'),
            '--build-receipt', str(build / 'build-receipt.json')]
    output = WORK / name / 'runtime'
    run_code = execute(name + '-run', ['python3', '-B', 'scripts/p8_runtime.py', *args,
                                      *arguments, '--output', str(output)], ENV)
    verify_code = execute(name + '-verify', ['python3', '-B', 'scripts/p8_runtime.py',
                                           'verify', '--output', str(output),
                                           '--build-output', str(build)], ENV)
    return run_code == 0 and verify_code == 0


def backfill_cell():
    env = dict(ENV, CARGO_TARGET_DIR=str(WORK / 'backfill-target'))
    out = str(WORK / 'backfill')
    run_code = execute('backfill-run', ['python3', '-B', 'scripts/p8_backfill.py', '--output', out], env)
    verify_code = execute('backfill-verify', ['python3', '-B', 'scripts/p8_backfill.py', 'verify', '--output', out], env)
    return run_code == 0 and verify_code == 0


if any((WORK / p).exists() for p in ('execution-receipt.json', 'runtime-target', 'backfill-target', 'build', 'backfill', 'soak', 'c1', 'c4', 'c8', 'c16')):
    raise RuntimeError('The original execution directories must not be reused')
ENV = dict(os.environ, PATH=str(TOOLCHAIN) + ':' + os.environ['PATH'],
           CARGO_HOME=str(CACHE), CARGO_TARGET_DIR=str(WORK / 'runtime-target'),
           CARGO_BUILD_JOBS='2', CARGO_INCREMENTAL='0', CARGO_TERM_COLOR='never',
           PYTHONDONTWRITEBYTECODE='1')
RECEIPT = dict(schema_version=1, kind='M6_original_six_local_runtime_observations',
               status='running', started_at_utc=utc(), source_before=identity(), phases=[],
               wrapper_sha256=digest(__file__), disk_free_before=shutil.disk_usage(WORK).free,
               runtime_plan=dict(concurrencies=[1,4,8,16], operations=900, files=1000, interval_ms=500),
               soak_plan=dict(concurrency=4, operations=3601, files=1000, interval_ms=1000),
               backfill_plan='unchanged p8_backfill.py: three fixed seeds, two phases, C1/4/8/16,32 work units',
               build_scope='One new private locked offline release target shared by five runtime observations; backfill has its own new target. No old product build is reused.',
               host_scope='Concurrent cells share this actual local host; not independent GitHub runners or a controlled speed comparison.',
               prior_attempts='M5 failed/incomplete originals retained unchanged; queued M6 GitHub Actions is a separate execution.',
               acceptance_scope='Actual fixed-source runtime evidence only; original scale and hard task dependencies remain separate.',
               original_tasks_completed=0, remaining_original_tasks=29)
save()
try:
    build_code = execute('build', ['python3', '-B', 'scripts/p8_runtime_build.py', '--output', str(WORK / 'build')], ENV)
    if build_code != 0:
        RECEIPT.update(status='failed_build', runtime_status='not_run', finished_at_utc=utc())
        save()
        raise SystemExit(1)
    plans = [('c' + str(c), ['--concurrency', str(c), '--operations', '900',
                            '--files', '1000', '--interval-ms', '500']) for c in (1,4,8,16)]
    plans.append(('soak', ['--profile', 'soak', '--concurrency', '4', '--operations', '3601',
                           '--files', '1000', '--interval-ms', '1000']))
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        futures = {pool.submit(runtime_cell, name, args): name for name, args in plans}
        futures[pool.submit(backfill_cell)] = 'backfill'
        outcomes = {}
        for future in concurrent.futures.as_completed(futures):
            outcomes[futures[future]] = future.result()
            with LOCK:
                RECEIPT['cell_outcomes'] = dict(outcomes)
                save()
    RECEIPT.update(status='passed' if all(outcomes.values()) else 'failed',
                   finished_at_utc=utc(), source_after=identity(),
                   disk_free_after=shutil.disk_usage(WORK).free)
    save()
    print(json.dumps(dict(event='complete', status=RECEIPT['status'], outcomes=outcomes)), flush=True)
    raise SystemExit(0 if RECEIPT['status'] == 'passed' else 1)
except SystemExit:
    raise
except BaseException as error:
    with LOCK:
        running = list(CHILDREN.items())
    for label, child in running:
        if child.poll() is None:
            child.send_signal(signal.SIGINT)
    RECEIPT.update(status='execution_infrastructure_failed', error=type(error).__name__ + ': ' + str(error),
                   finished_at_utc=utc(), signalled_owned_children=[label for label, _ in running])
    save()
    raise
