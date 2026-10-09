#!/usr/bin/env python3
"""Offline original-evidence review. Never run codecortex, build, index or workload."""
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import traceback

ROOT = Path(__file__).resolve().parent
REPO = Path('/workspace/scratch/50c364fd60b1/codecortex')
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
KEYS = ['c1', 'c4', 'c8', 'c16', 'soak', 'backfill']

def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

def encoded(v):
    return (json.dumps(v, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()

def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as f:
        f.write(encoded(value))

def require(condition, message):
    if not condition:
        raise AssertionError(message)

def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=REPO, input=data)

def source_inventory():
    entries = {}
    for row in git('ls-tree', '-r', '-z', SOURCE, '--', 'crates', 'Cargo.toml', 'Cargo.lock', 'scripts').split(b'\0'):
        if row:
            metadata, p = row.split(b'\t', 1)
            mode, kind, oid = metadata.decode().split()
            require(kind == 'blob' and mode in ('100644', '100755'), 'nonregular source')
            entries[p.decode()] = (mode, oid)
    unique = list(dict.fromkeys(oid for mode, oid in entries.values()))
    raw = git('cat-file', '--batch', data=('\n'.join(unique) + '\n').encode())
    offset, bodies = 0, {}
    for oid in unique:
        end = raw.index(b'\n', offset)
        got, kind, length = raw[offset:end].decode().split()
        require(got == oid and kind == 'blob', 'wrong Git batch object')
        offset = end + 1
        bodies[oid] = raw[offset:offset + int(length)]
        offset += int(length)
        require(raw[offset:offset + 1] == b'\n', 'truncated Git object')
        offset += 1
    require(offset == len(raw), 'trailing Git bytes')
    build = {p: hashlib.sha256(bodies[oid]).hexdigest() for p, (mode, oid) in sorted(entries.items())
             if p in ('Cargo.toml', 'Cargo.lock') or p.startswith('crates/')}
    snapshot = dict(source_commit=SOURCE, source_tree=git('rev-parse', SOURCE + '^{tree}').decode().strip(),
                    input_count=len(build), inputs=build, manifest_sha256=hashlib.sha256(encoded(build)).hexdigest())
    return entries, bodies, snapshot

ENTRIES, BODIES, EXPECTED_SOURCE = source_inventory()

def verify_observers(receipt, directory):
    before = receipt['observer_before']
    require(before == receipt['observer_after'], 'observer before/after mismatch')
    require(before['source_commit'] == SOURCE, 'wrong observer source')
    require(before['manifest_sha256'] == hashlib.sha256(encoded(before['files'])).hexdigest(), 'observer manifest digest')
    for relative, row in before['files'].items():
        mode, oid = ENTRIES[relative]
        body = BODIES[oid]
        require(row == dict(bytes=len(body), git_blob=oid, git_mode=mode, sha256=hashlib.sha256(body).hexdigest()),
                'observer row differs from fixed Git: ' + relative)
        require((directory / 'observer-source' / relative).read_bytes() == body, 'retained observer bytes differ')
    return len(before['files'])

def verify_source(receipt):
    for field in ['source_before', 'source_after']:
        require(receipt[field] == EXPECTED_SOURCE, field + ' differs from all fixed Git blobs')
    if 'source_final' in receipt:
        require(receipt['source_final'] == EXPECTED_SOURCE, 'final source differs')
    require(receipt['toolchain_before'] == receipt['toolchain_after'], 'toolchain before/after mismatch')
    if 'toolchain_final' in receipt:
        require(receipt['toolchain_before'] == receipt['toolchain_final'], 'toolchain final mismatch')
    require('release: 1.95.0' in receipt['toolchain_before']['rustc']['version'], 'wrong rustc release')
    require(receipt['build_exit_code'] == 0, 'original build failed')

def command(review, name, args):
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    with (review / (name + '.stdout')).open('xb') as out, (review / (name + '.stderr')).open('xb') as err:
        result = subprocess.run(args, stdout=out, stderr=err, timeout=60, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'})
    row = dict(argv=args, started_at=started, finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
               exit_code=result.returncode, stdout_sha256=sha(review / (name + '.stdout')), stderr_sha256=sha(review / (name + '.stderr')))
    write(review / (name + '.receipt.json'), row)
    require(result.returncode == 0, name + ' failed; original command output retained')
    return row

def verify_cargo(receipt, base, backfill=False):
    raw = base / ('build.jsonl' if backfill else 'product-build.jsonl')
    messages = [json.loads(line) for line in raw.read_text().splitlines() if line.strip()]
    require(sha(raw) == receipt['cargo_log_sha256'], 'Cargo raw digest')
    require(sha(base / ('build.stderr' if backfill else 'product-build.stderr')) == receipt['stderr_sha256'], 'Cargo stderr digest')
    finish = [m for m in messages if m.get('reason') == 'build-finished']
    require(len(finish) == 1 and finish[0].get('success') is True, 'Cargo actual build-finished')
    artifacts = {'p7_worker_contention': {'cargo_artifact': receipt['cargo_artifact'],
                    'binary_sha256': receipt['executable_sha256'], 'copy_source': receipt['copy_source']}} if backfill else receipt['artifacts']
    answer = {}
    for name, row in artifacts.items():
        selected = [m for m in messages if m.get('reason') == 'compiler-artifact' and m.get('target', {}).get('name') == name and m.get('executable')]
        require(len(selected) == 1 and selected[0] == row['cargo_artifact'], 'ambiguous or changed Cargo target: ' + name)
        m = selected[0]
        require(m['fresh'] is False, 'fresh borrowed artifact')
        require(m['profile']['opt_level'] == '3' and m['profile']['debug_assertions'] is False, 'not actual optimized release')
        require(sha(base / name) == row['binary_sha256'] == row['copy_source']['sha256'], 'retained artifact/copy bytes differ')
        require((base / name).stat().st_size == row['copy_source']['bytes'], 'retained executable size')
        answer[name] = dict(sha256=sha(base / name), bytes=(base / name).stat().st_size,
                           original_cargo_path=m['executable'], original_cargo_profile=m['profile'], fresh=m['fresh'])
    return answer

def run_one(key):
    base = ROOT / key / 'extracted'
    review = ROOT / key / 'offline-review'
    review.mkdir(exist_ok=False)
    backfill = key == 'backfill'
    build = base if backfill else base / 'p8-build'
    receipt = json.loads((build / ('receipt.json' if backfill else 'build-receipt.json')).read_text())
    verify_source(receipt)
    observer_count = verify_observers(receipt, build)
    binaries = verify_cargo(receipt, build, backfill)
    observers = build / 'observer-source/scripts'
    result = dict(key=key, source_commit=SOURCE, source_inputs=len(EXPECTED_SOURCE['inputs']), observer_inputs=observer_count,
                  source_and_observer_blobs_match=True, binaries=binaries, original_toolchain_records_equal=True,
                  current_machine_is_not_original_compiler_receipt=True, commands=[])
    if backfill:
        require(receipt['source_final'] == receipt['source_before'] and receipt['observer_final'] == receipt['observer_before'], 'backfill final identity')
        require(receipt['status'] == 'passed_observation' and receipt['execution_exit_code'] == receipt['exit_code'] == 0, 'backfill original result')
        require('1 passed; 0 failed; 0 ignored' in (base / 'execution.stdout').read_text(), 'selected backfill test missing')
        result['commands'].append(command(review, 'original-seal-verifier', [sys.executable, '-B', str(observers / 'p8_backfill.py'), 'verify', '--output', str(base)]))
    else:
        runtime = base / 'p8-runtime'
        plan = json.loads((runtime / 'plan.json').read_text())
        report = json.loads((runtime / 'report.json').read_text())
        require(plan['source'] == EXPECTED_SOURCE, 'plan source')
        require(report['final_build_verification'] == plan['build_identity'], 'final build identity')
        for name, field in [('codecortex', 'product_sha256'), ('p8-oracle', 'oracle_sha256'), ('p8-runtime-statistics', 'statistics_sha256')]:
            require(plan[field] == binaries[name]['sha256'], 'plan executable hash')
        for relative, row in plan['retained_build_evidence'].items():
            copy = runtime / 'build-evidence' / relative
            require(copy.read_bytes() == (build / relative).read_bytes(), 'build evidence copy ' + relative)
            require(row == {'bytes': copy.stat().st_size, 'sha256': sha(copy)}, 'build copy identity')
        require(report['status'] == 'passed_observation' and report['exit_code'] == 0 and report['failures'] == [], 'original runtime result')
        require(report['task_complete'] is False and report['release_approval'] is False, 'invalid whole task/release claim')
        result['commands'].append(command(review, 'original-seal-verifier', [sys.executable, '-B', str(observers / 'p8_runtime.py'), 'verify', '--output', str(runtime), '--build-output', str(build)]))
        # Executable COPY retains the original binary bytes; originals remain untouched.
        for name in ['p8-runtime-statistics', 'p8-oracle']:
            target = review / name
            shutil.copyfile(build / name, target)
            target.chmod(0o700)
            require(sha(target) == binaries[name]['sha256'], 'offline verifier copy changed')
        result['commands'].append(command(review, 'original-statistics-replay', [str(review / 'p8-runtime-statistics'), '--plan', str(runtime / 'plan.json'), '--raw', str(runtime / 'raw.jsonl'), '--output', str(review / 'statistics.json')]))
        require((runtime / 'statistics.json').read_bytes() == (runtime / 'statistics-replay.json').read_bytes() == (review / 'statistics.json').read_bytes(), 'offline statistics not byte-identical')
        # Copy only the two original persisted databases for SQLite scratch/sidecars.
        for side in ['project', 'fresh-full']:
            directory = review / 'oracle-input' / side / '.codecortex'
            directory.mkdir(parents=True)
            source = runtime / side / '.codecortex/index.sqlite3'
            shutil.copyfile(source, directory / 'index.sqlite3')
            require(sha(source) == sha(directory / 'index.sqlite3'), 'offline oracle database copy differs')
        result['commands'].append(command(review, 'original-oracle-replay', [str(review / 'p8-oracle'), '--left', str(review / 'oracle-input/project'), '--right', str(review / 'oracle-input/fresh-full'), '--output', str(review / 'parity.json')]))
        original = json.loads((runtime / 'parity.json').read_text())
        replay = json.loads((review / 'parity.json').read_text())
        ignored = {'elapsed_ns', 'left', 'right'}
        require({k:v for k,v in original.items() if k not in ignored} == {k:v for k,v in replay.items() if k not in ignored}, 'offline oracle differs beyond absolute paths/time')
        require(original['exit_code'] == 0 and original['comparison']['equal'] is True and len(original['comparison']['tables']) == 15, 'original complete fifteen-table parity')
        for side in ['project', 'fresh-full']:
            require(sha(runtime / side / '.codecortex/index.sqlite3') == sha(review / 'oracle-input' / side / '.codecortex/index.sqlite3'), 'oracle changed database bytes')
        result.update(statistics_byte_identical=True, complete_fifteen_table_oracle_replayed=True,
                      oracle_replay_ignored_fields=sorted(ignored), oracle_has_no_product_build_or_repair=True,
                      raw_sha256=sha(runtime / 'raw.jsonl'), plan_sha256=sha(runtime / 'plan.json'),
                      oracle_source_sha256=hashlib.sha256(BODIES[ENTRIES['crates/cc-eval/src/bin/p8-oracle.rs'][1]]).hexdigest())
    result['integrity_and_replay_status'] = 'passed'
    write(review / 'integrity-and-replay.json', result)
    print(json.dumps({k:result[k] for k in ['key','source_inputs','observer_inputs','integrity_and_replay_status']}, ensure_ascii=False), flush=True)
    return result

if __name__ == '__main__':
    output = []
    try:
        for key in KEYS:
            output.append(run_one(key))
        write(ROOT / 'integrity-and-replays.json', dict(source=EXPECTED_SOURCE, results=output,
              scope='read-only retained evidence and verifier replay; no codecortex/index/build/workload execution; no performance sample',
              remaining_original_todos=29))
    except Exception:
        write(ROOT / 'integrity-and-replays-failure.json', dict(completed=output, traceback=traceback.format_exc()))
        raise
