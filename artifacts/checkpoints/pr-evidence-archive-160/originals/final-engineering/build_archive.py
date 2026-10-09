#!/usr/bin/env python3
"""Copy and verify immutable engineering evidence; never modify source evidence."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tarfile
import tempfile
from datetime import datetime, timezone

WORKSPACE = Path('/workspace/scratch/2eaa00d0f93a')
OUTPUT = WORKSPACE / 'p8-final-engineering-archive'
REPO = WORKSPACE / 'codecortex'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
HEADS = {
    'refs/heads/task/p8-final-candidate-20261009': '570b2afb33d19cb56e110fa0130b529541327fdd',
    'refs/heads/task/p8-scale-20261009': '958c7d58e8f4486a0dd3917395a8883a58c1aaff',
    'refs/heads/task/p8-platform-review-20261009': '88c580cdd216e4347d0a625ade92a865ddecf6a3',
}
ROOTS = [
    'p8-unified-validation-0832118b',
    'p8-unified-validation-sql-ordinal',
    'p8-unified-validation-formatted',
    'p8-unified-rust-controls',
    'p8-unified-clippy-6956',
    'p8-unified-clippy-be9c74ff',
    'p8-unified-clippy-c6d5edba',
    'p8-runtime-build-c6d5edba',
    'p8-runtime-short-c6d5edba',
    'validation-scale-20261009/capacity-build-c6d5edba',
    'validation-scale-20261009/capacity-preflight-1000-c6d5edba',
    'validation-scale-20261009/capacity-preflight-10000-c6d5edba',
    'validation-scale-20261009/capacity-observations-1000-c6d5edba',
    'validation-scale-20261009/capacity-observations-10000-c6d5edba',
    'validation-scale-20261009/capacity-preflight-c6d5edba-summary.json',
    'validation-scale-20261009/capacity-preflight-observer.py',
    'p8-platform-independent-review-6956',
    'p8-platform-independent-review-f69acbbe',
    'p8-platform-independent-review-88c580cd',
    'p8-platform-observer-f69acbbe',
    'p8-final-independent-source-review/intake-6956b095.json',
    'p8-final-independent-source-review/intake-bce4f18c.json',
    'p8-final-independent-source-review/intake-570b2afb.json',
    'p8-final-independent-source-review/inventory_git_inputs.py',
    'p8-final-independent-source-review/engineering-log-integrity-review.json',
    'p8-final-independent-source-review/verify_engineering_logs.py',
    'p8-final-independent-source-review/portable-scale-controls-eef27cc9',
    'p8-final-independent-source-review/portable-scale-review-eef27cc9.json',
    'p8-platform-review-final.stdout',
    'p8-platform-review-final.stderr',
    'p8-platform-review-verbose.stdout',
    'p8-platform-review-verbose.stderr',
    'p8-platform-review-verbose-recovery.stdout',
    'p8-platform-review-verbose-recovery.stderr',
    'p8-target-measurements-cleanup-20261009',
    'p8-worktree-cleanup-20261009',
    'p8-final-python-bce4f18c',
    'p8-final-python-570b2afb',
    'p8-final-runtime-controller-bce4f18c',
    'p8-runtime-build-bce4f18c',
    'p8-runtime-short-bce4f18c',
    'validation-scale-20261009/capacity-build-bce4f18c',
]


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def write(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write('\n')


def command(argv, cwd=None):
    result = subprocess.run(argv, cwd=cwd, capture_output=True, text=True)
    record = dict(argv=argv, exit_code=result.returncode, stdout=result.stdout, stderr=result.stderr)
    assert result.returncode == 0, record
    return record


def files_under(path):
    if path.is_file():
        return [path]
    assert path.is_dir() and not path.is_symlink(), path
    found = []
    for child in sorted(path.rglob('*')):
        mode = child.lstat().st_mode
        assert stat.S_ISDIR(mode) or stat.S_ISREG(mode), child
        if stat.S_ISREG(mode):
            found.append(child)
    return found


def run():
    bundle = OUTPUT / 'sources.bundle'
    verify_bundle = command(['git', 'bundle', 'verify', str(bundle)], cwd=REPO)
    listed = command(['git', 'bundle', 'list-heads', str(bundle)], cwd=REPO)
    listed_heads = {line.split()[1]: line.split()[0] for line in listed['stdout'].splitlines()}
    assert listed_heads == HEADS
    with bundle.open('rb') as stream:
        header = []
        while True:
            line = stream.readline()
            assert line, 'missing bundle header terminator'
            if line == b'\n':
                break
            header.append(line.decode().rstrip('\n'))
    prerequisites = [line[1:].split()[0] for line in header if line.startswith('-')]
    assert prerequisites == [BASE]
    assert bundle.stat().st_size < 90 * 1024 * 1024
    write(OUTPUT / 'sources.bundle.json', dict(
        scope='Incremental local engineering source history before this archive commit; not final P2 approval',
        heads=HEADS, prerequisites=prerequisites, header=header,
        bytes=bundle.stat().st_size, sha256=digest(bundle),
        verification=verify_bundle, list_heads=listed))

    entries = {}
    groups = {}
    for relative in ROOTS:
        path = WORKSPACE / relative
        files = files_under(path)
        groups[relative] = dict(file_count=len(files), bytes=0)
        for file in files:
            key = file.relative_to(WORKSPACE).as_posix()
            assert key not in entries, key
            mode = file.lstat().st_mode
            assert stat.S_ISREG(mode), file
            entry = dict(bytes=file.stat().st_size, sha256=digest(file), mode=stat.S_IMODE(mode), source_group=relative)
            entries[key] = entry
            groups[relative]['bytes'] += entry['bytes']

    # Uncompressed file bytes stay below 85 MiB per part. Every actual compressed
    # part is independently checked against the stricter user-facing 90 MiB cap.
    parts = [[]]
    size = 0
    for key in sorted(entries):
        length = entries[key]['bytes']
        assert length < 85 * 1024 * 1024, key
        if size + length > 85 * 1024 * 1024:
            parts.append([])
            size = 0
        parts[-1].append(key)
        size += length

    archives = {}
    for index, names in enumerate(parts, 1):
        name = f'evidence-part-{index:03}.tar.gz'
        archive = OUTPUT / name
        with archive.open('xb') as raw:
            with gzip.GzipFile(filename='', fileobj=raw, mode='wb', mtime=0, compresslevel=6) as zipped:
                with tarfile.open(fileobj=zipped, mode='w', format=tarfile.PAX_FORMAT, dereference=True) as tar:
                    for key in names:
                        path = WORKSPACE / key
                        info = tar.gettarinfo(str(path), arcname=key)
                        assert info.isfile() and info.size == entries[key]['bytes']
                        with path.open('rb') as stream:
                            tar.addfile(info, stream)
                        entries[key]['archive'] = name
        assert archive.stat().st_size < 90 * 1024 * 1024, archive
        archives[name] = dict(bytes=archive.stat().st_size, sha256=digest(archive), file_count=len(names))

    verification = dict(scope='Archive byte preservation, exact membership, seals, and paired offline verification only',
                        tar_parts=[], runtime_pairs=[], source_files_rechecked=len(entries))
    seen = set()
    with tempfile.TemporaryDirectory(prefix='p8-archive-verify-', dir=WORKSPACE) as temporary:
        extracted = Path(temporary)
        for name in archives:
            count = 0
            with tarfile.open(OUTPUT / name, 'r:gz') as tar:
                for member in tar:
                    assert member.isfile() and member.name in entries and member.name not in seen, member.name
                    expected = entries[member.name]
                    assert expected['archive'] == name and member.size == expected['bytes']
                    assert member.mode == expected['mode']
                    relative = Path(member.name)
                    assert not relative.is_absolute() and '..' not in relative.parts
                    destination = extracted / relative
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    actual_hash = hashlib.sha256()
                    actual_size = 0
                    with tar.extractfile(member) as incoming, destination.open('xb') as out:
                        for block in iter(lambda: incoming.read(1024 * 1024), b''):
                            out.write(block)
                            actual_hash.update(block)
                            actual_size += len(block)
                    destination.chmod(member.mode)
                    assert actual_size == expected['bytes'] and actual_hash.hexdigest() == expected['sha256']
                    seen.add(member.name)
                    count += 1
            verification['tar_parts'].append(dict(archive=name, exact_membership_and_bytes='passed', files=count))
        assert seen == set(entries)
        for source in ('c6d5edba', 'bce4f18c'):
            build = extracted / f'p8-runtime-build-{source}'
            observation = extracted / f'p8-runtime-short-{source}'
            for directory in (build, observation):
                seal = json.loads((directory / 'seal.json').read_text())
                recorded = seal['artifact_inventory']
                actual = {p.relative_to(directory).as_posix(): dict(bytes=p.stat().st_size, sha256=digest(p))
                          for p in files_under(directory) if p != directory / 'seal.json'}
                assert recorded == actual, str(directory)
            observer = build / 'observer-source/scripts/p8_runtime.py'
            result = command([sys.executable, '-B', str(observer), 'verify', '--output', str(observation), '--build-output', str(build)])
            parsed = json.loads(result['stdout'])
            assert parsed['status'] == 'sealed_artifacts_verified'
            verification['runtime_pairs'].append(dict(source=source, build_seal_files=len(json.loads((build/'seal.json').read_text())['artifact_inventory']), observation_seal_files=parsed['files'], command=result))
    for key, expected in entries.items():
        path = WORKSPACE / key
        assert path.stat().st_size == expected['bytes'] and digest(path) == expected['sha256'], key

    write(OUTPUT / 'files.manifest.json', dict(schema_version=1, path_scope='Paths relative to original workspace; extract every tar part into one directory', files=entries))
    write(OUTPUT / 'verification.json', verification)
    write(OUTPUT / 'summary.json', dict(
        schema_version=1, created_utc=datetime.now(timezone.utc).isoformat(),
        kind='original_engineering_evidence_archive',
        scope='Immutable failures, cancellation, correctness controls, short engineering observations and explicit not_run records; not formal scale/runtime/platform acceptance or final P2 review',
        changes_to_original_evidence=False, todo_status_changes=False,
        source_bundle_heads=HEADS, source_bundle_prerequisites=[BASE],
        evidence_files=len(entries), evidence_bytes=sum(e['bytes'] for e in entries.values()),
        groups=groups, archives=archives,
        excluded_by_scope=['Cargo target caches', 'large working-tree source copies', 'future archive commit', 'unfinished final P2 approval'],
        preserved_outcomes={
            '0832118b_release_compile': 'failed: rusqlite u64 ordinal binding',
            'sql_ordinal_release_compile': 'controller-cancelled exit 130',
            '6956_release_compile': 'passed: five binaries',
            '6956_release_controls': '42 passed; no performance certification',
            'clippy_6956': 'failed: capacity predicate lint',
            'clippy_be9c74ff': 'failed: statistics test helper lint',
            'clippy_c6d5edba': 'passed: original workspace/all-targets gate',
            'capacity_1k_10k_c6d5edba': 'not_run: registered free-space threshold unmet',
            'platform_6956_independent_review': 'blocked: eight reproducible acceptance counterexamples',
            'final_python_bce4f18c': '348 passed plus fmt/diff/roadmap/facts exit 0',
            'final_python_570b2afb': '350 passed plus fmt/diff/roadmap/facts exit 0',
            'platform_88c580cd_independent_review': '55 platform plus 24 recovery Python controls passed; no formal platform build matrix claim',
            'runtime_short_scope': 'engineering-only short mixed schedule; no long soak or final release certification',
        },
        verification='All archived members and original files rehashed; both retained runtime build/observation pairs passed their original offline verifier; bundle verify passed with exactly one prerequisite'))
    print(json.dumps(dict(files=len(entries), archives=archives, bundle_bytes=bundle.stat().st_size), indent=2))


if __name__ == '__main__':
    run()
