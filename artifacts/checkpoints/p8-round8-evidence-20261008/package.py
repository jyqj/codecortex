#!/usr/bin/env python3
"""Freeze completed inspections and the first registered local1k raw cell."""
import datetime
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent
SCRATCH = Path('/workspace/scratch/2eaa00d0f93a')
COHORT = SCRATCH / 'p8-local-G-1k-20261008'

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def archive(name, paths):
    assert len({target for _, target in paths}) == len(paths)
    rows = []
    output = HERE / name
    with output.open('xb') as target:
        with gzip.GzipFile(filename='', mode='wb', fileobj=target, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode='w') as result:
                for path, member_name in sorted(paths, key=lambda item: item[1]):
                    assert path.is_file() and not path.is_symlink()
                    assert not member_name.startswith('/') and '..' not in Path(member_name).parts
                    raw = path.read_bytes()
                    info = tarfile.TarInfo(member_name)
                    info.size = len(raw)
                    info.mode = path.stat().st_mode & 0o777
                    info.mtime = 0
                    result.addfile(info, io.BytesIO(raw))
                    rows.append({'name': member_name, 'source_path': str(path), 'bytes': len(raw),
                                 'sha256': sha(raw), 'mode': info.mode})
    expected = {row['name']: row for row in rows}
    with tarfile.open(output, 'r:gz') as result:
        members = result.getmembers()
        assert len(members) == len(expected)
        assert {member.name for member in members} == set(expected)
        for member in members:
            assert member.isfile()
            row = expected[member.name]
            raw = result.extractfile(member).read()
            assert len(raw) == row['bytes'] and sha(raw) == row['sha256']
            assert member.mode == row['mode']
            assert sha(Path(row['source_path']).read_bytes()) == row['sha256']
    raw = output.read_bytes()
    return {'path': name, 'bytes': len(raw), 'sha256': sha(raw),
            'git_blob': hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest(),
            'files': rows, 'uncompressed_bytes': sum(row['bytes'] for row in rows),
            'roundtrip_all_bytes_modes_verified': True, 'all_sources_unchanged_after_packaging': True}

def collect(root, prefix, recursive=True, exclude_zip=True):
    items = root.rglob('*') if recursive else root.iterdir()
    return [(p, prefix + '/' + p.relative_to(root).as_posix()) for p in items
            if p.is_file() and (not exclude_zip or p.suffix != '.zip')]

def main():
    first = json.loads((COHORT / 'controls/00/execution.json').read_text())
    assert first['status'] == 'passed_original_cell_and_validation' and first['cli_stopped_confirmed']
    check = Path('/dev/shm/p8-local-first-cell-independent-review/inspection.json')
    assert sha(check.read_bytes()) == 'e2a7dfd75d84830e7b4037889678e871bfb2f925eaf4a5d6fb4c1fb4b7eb26ff'
    roots = [
        ('p8-round7-G2-final-CI-independent', True),
        ('p8-formal-G2-soak-pr-audit', True),
        ('p8-peer-scale-hotspots', False),
        ('p8-728-controls-reception', True),
        ('p8-soak-cache-real-probe-independent', True),
        ('p8-exact-G-checkout-preparation', True),
        ('p8-local-G-cell-wrapper-independent-review', True),
        ('p8-local-first-cell-independent-review', True),
    ]
    reviews = []
    for name, recursive in roots:
        reviews.extend(collect(Path('/dev/shm') / name, name, recursive))
    reviews.append((Path('/dev/shm/p8-prefix-f9-1k-reception/audit.json'), 'p8-prefix-f9-1k-reception/audit.json'))
    runner = Path('/dev/shm/p8-local-G-1k-registration-root')
    for name in ['run_cell.py', 'run_cell_initial_blocked.py', 'run_remaining.py', 'publication.json']:
        reviews.append((runner / name, 'local1k-execution/' + name))
    results = [archive('completed-reviews.tar.gz', reviews)]
    first_raw = collect(COHORT / 'shard-1000-00', 'shard-1000-00', True, False)
    first_raw += collect(COHORT / 'controls/00', 'controls/00', True, False)
    first_raw.append((COHORT / 'controls/build-copy.json', 'controls/build-copy.json'))
    results.append(archive('local1k-repetition-0.tar.gz', first_raw))
    originals = [
        {'label': 'G2-soak', 'run': 37837227959, 'artifact': 11581055784,
         'path': '/dev/shm/p8-formal-G2-soak-pr-audit/artifact-11581055784.zip'},
        {'label': 'peer6e3-50k', 'run': 37824742267, 'artifact': 11576387472,
         'path': '/dev/shm/p8-peer-scale-hotspots/11576387472.zip'},
        {'label': 'peer599-10k', 'run': 37835810882, 'artifact': 11577518538,
         'path': '/dev/shm/p8-peer-scale-hotspots/11577518538.zip'},
        {'label': 'peer599-50k', 'run': 37835810882, 'artifact': 11580138693,
         'path': '/dev/shm/p8-peer-scale-hotspots/11580138693.zip'},
        {'label': 'engineering728-controls', 'run': 37842877692, 'artifact': 11579050976,
         'path': '/dev/shm/p8-728-controls-reception/11579050976.zip'},
        {'label': 'engineeringf9-1k', 'run': 37841189935, 'artifact': 11581506078,
         'path': '/dev/shm/p8-prefix-f9-1k-reception/11581506078.zip'},
    ]
    for record in originals:
        path = Path(record['path'])
        record.update(bytes=path.stat().st_size, sha256=sha(path.read_bytes()),
                      archived_here=False, preserved_locally=True,
                      actions_url=f"https://github.com/jyqj/codecortex/actions/runs/{record['run']}/artifacts/{record['artifact']}")
    manifest = {
        'schema': 'p8-round8-completed-evidence-checkpoint-v1',
        'created_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'parent': '24b0cba53178d14ef6c02355ee271fa0067be3e0',
        'archives': results, 'external_original_artifacts': originals,
        'local1k_registration_commit': 'b94a93a9d92a237e0c55a23fd8e931c5dae659f0',
        'local1k_original_build': {
            'run': 37830173594, 'artifact': 11574210551,
            'source': 'bb9a96d71622458c39a143055360cc97f0d11d78',
            'receipt_sha256': '414b1767f2ee3a1c94f05ef12f5f6b58fd604202cfaba66522a348f7a9627a02',
            'binary_sha256': '501cb11d46c332340a13875ca5ca8048e560917dc3796d8528b27cd12b9779a7',
            'binary_in_this_checkpoint': False},
        'limits': [
            'Completed evidence snapshot during round8; it does not assert the round or all30local cells has finished.',
            'No currently running cell, sequence log, or temporary fixture is sealed here. Only registered local1k repetition0 raw and its terminal controls are archived.',
            'The local1k first cell uses exact originalG source, original build and unchanged validators; 1/150 is not a complete scale matrix.',
            'G2 one-hour soak is real original-symbol-only evidence with cache hits/misses zero; it cannot substitute for the pendingG3 cache observation.',
            'Engineering and peer observations keep their own source/build/host/scale identities, cannot be pooled into a primaryG cohort, and do not establish causal speedup.',
            'Referenced GitHub ZIPs are preserved locally and on Actions, not duplicated into these two archives. This checkpoint does not claim durable all-originals export or P8-019 closure.',
            'Original task definitions, statuses, acceptance thresholds, CI logic and source guards are unchanged.'
        ],
        'original_todos': {'total': 192, 'done': 163, 'in_progress': 16, 'todo': 12, 'blocked': 1, 'remaining': 29, 'newly_closed': 0}
    }
    with (HERE / 'manifest.json').open('x') as stream:
        json.dump(manifest, stream, indent=2, sort_keys=True)
        stream.write('\n')
    print(json.dumps({'archives': [{k: row[k] for k in ('path','bytes','sha256','git_blob','uncompressed_bytes')} for row in results],
                      'manifest_sha256': sha((HERE / 'manifest.json').read_bytes()), 'todo_remaining': 29}, sort_keys=True))

if __name__ == '__main__':
    main()
