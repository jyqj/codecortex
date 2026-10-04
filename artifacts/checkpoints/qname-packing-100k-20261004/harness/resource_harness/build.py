"""Explicit preparation only. No product process or measurement imports."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

from .identity import checkout_identity, digest, driver_identity, precheck, SOURCES
from .protocol import write
from .runtime import Journal, Observer


def build_one(root, out, variant, toolchain, group, scope):
    identity = checkout_identity(root, variant)
    out.mkdir()
    for name in ('tmp', 'target', 'cargo'):
        (out / name).mkdir()
    env = dict(os.environ, CARGO_HOME=str(out / 'cargo'), RUSTC=str(toolchain / 'rustc'),
               TMPDIR=str(out / 'tmp'), CARGO_TARGET_DIR=str(out / 'target'), CARGO_BUILD_JOBS='4')
    env['PATH'] = str(toolchain) + ':' + env['PATH']
    command = [str(toolchain / 'cargo'), 'build', '--release', '-p', 'cc-server',
               '--bin', 'codecortex', '--no-default-features', '--features',
               'semantic-http', '--locked', '--message-format=json-render-diagnostics']
    receipt = identity | dict(command=command, profile='release', target_dir=str(out / 'target'),
        environment={k: env[k] for k in ('CARGO_HOME', 'TMPDIR', 'CARGO_TARGET_DIR', 'CARGO_BUILD_JOBS')},
        timeout_seconds=900, guard_memory_bytes=12*1024**3, guard_disk_reserve_bytes=4*1024**3,
        sample_interval_seconds=.5, guard_stop=None, driver_files=driver_identity())
    for tool in ('rustc', 'cargo'):
        receipt[tool] = subprocess.check_output([str(toolchain / tool), '-Vv'], env=env, text=True)
    if not receipt['rustc'].startswith('rustc 1.95.0 (59807616e 2026-04-14)'):
        raise RuntimeError('fixed rustc 1.95.0 identity required')
    precheck(group, out)
    start = time.monotonic()
    with (out / 'composition.jsonl').open('w') as composition, \
         (out / 'cargo-build.jsonl').open('w') as stdout, \
         (out / 'build-stderr.log').open('w') as stderr:
        observer = Observer(Journal(composition), group, scope=scope,
                            processes={'harness_root': os.getpid()})
        observer.begin_phase('build-before')
        proc = subprocess.Popen(command, cwd=root, env=env, stdout=stdout,
                                stderr=stderr, start_new_session=True)
        observer.processes['build_root'] = proc.pid
        try:
            observer.begin_phase('build')
            while proc.poll() is None:
                observer.sample()
                current = int((group / 'memory.current').read_text())
                reason = ('timeout_900s' if time.monotonic()-start>900 else
                          'memory_12GiB' if current>12*1024**3 else
                          'disk_below_4GiB' if shutil.disk_usage(out).free<4*1024**3 else None)
                if reason:
                    receipt['guard_stop'] = reason
                    write(out / 'build-receipt.json', receipt)
                    os.killpg(proc.pid, signal.SIGTERM)
                    proc.wait(timeout=15)
                    break
                time.sleep(.5)
            receipt['build_exit_code'] = proc.wait()
        except BaseException as exc:
            receipt['guard_stop'] = receipt['guard_stop'] or 'build_observation_error'
            receipt['error'] = repr(exc)
            write(out / 'build-receipt.json', receipt)
            if proc.poll() is None:
                os.killpg(proc.pid, signal.SIGTERM)
                proc.wait(timeout=15)
            raise
        finally:
            observer.begin_phase('build-complete')
            receipt['wall_seconds'] = time.monotonic()-start
            write(out / 'build-receipt.json', receipt)
    if receipt['build_exit_code'] != 0 or receipt['guard_stop']:
        raise RuntimeError(f'{variant} build failed; retained receipt')
    artifacts = [m for line in (out / 'cargo-build.jsonl').read_text().splitlines()
                 if (m := json.loads(line)).get('reason') == 'compiler-artifact'
                 and m.get('target', {}).get('name') == 'codecortex' and m.get('executable')]
    assert len(artifacts) == 1
    artifact = artifacts[0]
    assert sorted(artifact['features']) == ['semantic', 'semantic-http']
    assert artifact['profile']['opt_level'] == '3' and not artifact['profile']['debug_assertions']
    receipt['compiler_artifact'] = artifact
    receipt['binary_sha256'] = digest(artifact['executable'])
    assert checkout_identity(root, variant) == identity
    receipt['log_sha256'] = {name: digest(out / name) for name in
                            ('cargo-build.jsonl', 'build-stderr.log', 'composition.jsonl')}
    write(out / 'build-receipt.json', receipt)
    return receipt


def prepare(roots, output, toolchain, group, scope):
    precheck(group, output.parent)
    # Validate both roots and toolchain before starting any build.
    for variant in SOURCES:
        checkout_identity(roots[variant], variant)
    for name in ('cargo', 'rustc'):
        if not os.access(toolchain / name, os.X_OK):
            raise PermissionError(f'not executable: {toolchain / name}')
    output.mkdir()
    receipts = {variant: build_one(roots[variant], output / variant, variant,
                                  toolchain, group, scope) for variant in SOURCES}
    bundle = dict(driver_files=driver_identity(), sources=SOURCES,
                  receipts={v: dict(path=str(output / v / 'build-receipt.json'),
                                    sha256=digest(output / v / 'build-receipt.json')) for v in receipts},
                  builds_complete_ns=time.monotonic_ns(), group_scope=scope,
                  group_independence='unknown')
    write(output / 'builds-ready.json', bundle)
    return bundle
