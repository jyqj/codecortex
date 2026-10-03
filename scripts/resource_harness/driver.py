"""Reusable PR126 driver. Help/inspect/import never build or start a product."""
import argparse
import json
from pathlib import Path
import sys
import time

from .identity import (SOURCES, compiler_scan, digest, driver_identity, precheck,
                       validate_receipt)
from .protocol import write


def load_bundle(path):
    bundle = json.loads(path.read_text())
    if bundle['sources'] != SOURCES or bundle['driver_files'] != driver_identity():
        raise RuntimeError('build bundle source/driver identity mismatch; prepare with this exact driver')
    receipts = {}
    for variant in SOURCES:
        record = bundle['receipts'][variant]
        receipt_path = Path(record['path'])
        if digest(receipt_path) != record['sha256']:
            raise RuntimeError(f'{variant}: receipt hash mismatch')
        receipts[variant] = json.loads(receipt_path.read_text())
        validate_receipt(receipts[variant], variant)
    return bundle, receipts


def run_single(bundle_path, variant, output, group, scope, *, measure_fn=None):
    # No build fallback exists in this path. Both receipts are mandatory.
    bundle, receipts = load_bundle(bundle_path)
    scan = compiler_scan()
    initial = precheck(group, output.parent)
    output.mkdir()
    state = dict(variant=variant, formal_runs=0, status='preflight',
                 builds_ready_sha256=digest(bundle_path), driver_files=driver_identity(),
                 precheck=initial, compiler_scan=scan, started_ns=time.monotonic_ns(),
                 group_scope=scope, group_independence='unknown',
                 strict_paired_causal_speedup=False)
    write(output / 'session.json', state)
    if measure_fn is None:
        from .measurement import measure
        measure_fn = measure
    try:
        # Baseline consumer must also complete the candidate's OWN cache smoke.
        for name in dict.fromkeys((variant, 'candidate')):
            summary = measure_fn(Path(receipts[name]['source_root']), output / ('preflight-' + name),
                        SOURCES[name], receipts[name], bundle['receipts'][name]['sha256'],
                        group, scope, preflight=True)
            if (summary['status'] != 'passed_preflight_32' or summary['files'] != 32
                    or summary['failures'] or summary['not_run']):
                raise RuntimeError(f'{name} controlled hold/cache precondition failed; no 100k run')
        state['candidate_own_cache_smoke_complete'] = True
        state['preflight_complete_ns'] = time.monotonic_ns()
        # Recheck after the smokes; consumers cannot build within measurement.
        load_bundle(bundle_path)
        state['compiler_scan_before_measurement'] = compiler_scan()
        precheck(group, output)
        state['formal_runs'] = 1
        state['status'] = 'measurement'
        write(output / 'session.json', state)
        summary = measure_fn(Path(receipts[variant]['source_root']), output / 'measurement',
                    SOURCES[variant], receipts[variant], bundle['receipts'][variant]['sha256'],
                    group, scope)
        state['status'] = summary['status']
        return 0 if summary['status'] == 'passed_declared_100k_local_scope' else 1
    except BaseException as exc:
        state['status'] = 'failed'
        state['error'] = repr(exc)
        raise
    finally:
        state['finished_ns'] = time.monotonic_ns()
        write(output / 'session.json', state)
        # Hash ONLY this new session's files. Never package historical artifacts.
        write(output / 'logs-sha256.json', {str(p.relative_to(output)): digest(p)
              for p in sorted(output.rglob('*')) if p.is_file() and p.name != 'logs-sha256.json'})


def main(argv=None):
    if not __debug__:
        raise RuntimeError('assertions are protocol gates; Python -O is forbidden')
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    commands.add_parser('manifest', help='print fixed protocol; no product/build')
    prepare_parser = commands.add_parser('prepare', help='explicitly build BOTH versions; no product')
    prepare_parser.add_argument('--baseline-root', required=True, type=Path)
    prepare_parser.add_argument('--candidate-root', required=True, type=Path)
    prepare_parser.add_argument('--toolchain-bin', required=True, type=Path)
    prepare_parser.add_argument('--output', required=True, type=Path)
    run_parser = commands.add_parser('run-single', help='explicit product preflights plus ONE formal run')
    run_parser.add_argument('--builds-ready', required=True, type=Path)
    run_parser.add_argument('--variant', required=True, choices=SOURCES)
    run_parser.add_argument('--output', required=True, type=Path)
    for command in (prepare_parser, run_parser):
        command.add_argument('--group', required=True, type=Path,
                             help='existing readable actual measured cgroup path; no creation/migration')
        command.add_argument('--scope', required=True, choices=('shared_build_cache_runner', 'unknown'))
    args = parser.parse_args(argv)
    if args.command == 'manifest':
        print((Path(__file__).parent / 'protocol-manifest.json').read_text())
        return 0
    if args.command == 'prepare':
        from .build import prepare
        prepare(dict(baseline=args.baseline_root.resolve(), candidate=args.candidate_root.resolve()),
                args.output.resolve(), args.toolchain_bin.resolve(), args.group.resolve(), args.scope)
        return 0
    return run_single(args.builds_ready.resolve(), args.variant, args.output.resolve(),
                      args.group.resolve(), args.scope)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f'{type(exc).__name__}: {exc}', file=sys.stderr)
        raise SystemExit(2)
