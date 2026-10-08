#!/usr/bin/env python3
"""Preserve selected original evidence, without running any product or test."""
from pathlib import Path, PurePosixPath
import gzip
import hashlib
import io
import json
import re
import tarfile
import urllib.parse
import zipfile

HERE = Path(__file__).resolve().parent
PREFIX = 'artifacts/checkpoints/p8-round11-evidence-20261008/'

def sha(b):
    return hashlib.sha256(b).hexdigest()

def load(path):
    return json.loads(Path(path).read_bytes())

def private_url_check(body):
    try:
        text = body.decode('utf-8')
    except UnicodeDecodeError:
        return
    for url in re.findall(r'https?://[^\s<>"\x27]+', text):
        keys = {k.lower() for k in urllib.parse.parse_qs(urllib.parse.urlsplit(url).query)}
        assert not (keys & {'sig', 'x-amz-signature', 'x-amz-credential', 'x-goog-signature', 'se'}), 'private signed URL found'

def main():
    members = {}
    originals = {}
    def add(name, path, identity=None):
        p = Path(path)
        assert p.is_file() and not p.is_symlink()
        relative = PurePosixPath(name)
        assert not relative.is_absolute() and '..' not in relative.parts and relative.as_posix() == name and name not in members
        b = p.read_bytes()
        if identity:
            assert len(b) == identity['bytes'] and sha(b) == identity['sha256'], name
        if p.suffix == '.zip':
            with zipfile.ZipFile(io.BytesIO(b)) as z:
                assert z.testzip() is None
                for i in z.infolist():
                    private_url_check(z.read(i))
        else:
            private_url_check(b)
        members[name] = b
        originals[name] = {'source_path': str(p), 'bytes': len(b), 'sha256': sha(b)}

    peer_list = Path('/workspace/scratch/2eaa00d0f93a/p8-scale-observations-20261008/round11-peer6e3-failure-whitelist.json')
    assert sha(peer_list.read_bytes()) == '3cf2c7f9cf4ec4878f2dab6d5eac5d3b2fdc2a522d9e0442a7b908dde2e62c7a'
    peer = load(peer_list)
    zip_row = next(r for r in peer['files'] if r['archive_path'].endswith('.zip'))
    for row in peer['files']:
        if row is not zip_row:
            add(row['archive_path'], row['source_path'], row)
    add('peer6e3-100k-failed/public-archive-allowlist.json', peer_list)

    failure_list = Path('/dev/shm/p8-D0-first-failure-100k-08/public-archive-allowlist.json')
    assert sha(failure_list.read_bytes()) == '79bfa039bb6ddd5daf92054d9b8ea5e2ed31b00e7dd35f5b3f00361609a165f4'
    for row in load(failure_list)['files']:
        add('D0-first-failure-100k-08/' + row['archive_name'], row['path'], row)
    add('D0-first-failure-100k-08/public-archive-allowlist.json', failure_list)

    groups = [
        ('soak-adapter', '/workspace/scratch/2eaa00d0f93a/p8-formal-evidence-G4/soak', ['inspect.py', 'cache_wire.py', 'build_adapter.py', 'adapter.diff', 'adapter-plan.json', 'cache-wire-controls.json']),
        ('soak-adapter-root-review', '/dev/shm/p8-G4-soak-adapter-root-review', ['review.py', 'review.json']),
        ('P7-engineering-original', '/dev/shm/p8-G4-P7engineering-pr-audit', ['artifact-11584817623.zip', 'job-113576447291.log', 'inspect_original.py', 'member-hashes.json', 'inspection.json', 'jobs.json', 'artifacts.json', 'run.json']),
        ('G4-CI-final', '/dev/shm/p8-G4-CI-reception-pr-audit', ['jobs-final.json', 'msrv-job-113576447831.log', 'msrv-audit.json']),
        ('G3-CI-completed', '/dev/shm/p8-G3-CI-reception-pr-audit', ['msrv-security-audit.json', 'check-job-113547079679.log', 'jobs-final.json', 'complete-CI-audit.json', 'merge-API.json', 'job-113547079711.log', 'job-113547079305.log']),
        ('PR165-independent-review', '/dev/shm/p8-pr165-independent-review', ['verification-execution.json', 'review.json', 'verify.stderr', 'verify.stdout', 'verify.py', 'new-reviews.json', 'relocated-root.json', 'original-subtree.json', 'new-archive.json', 'head-checkpoints.json', 'base-checkpoints.json', 'head-artifacts.json', 'base-artifacts.json', 'head-root.json', 'base-root.json', 'checks.json', 'base-commit.json', 'head-commit.json', 'pull.json']),
        ('round10-publication', '/workspace/scratch/2eaa00d0f93a/p8-round10-evidence-publication-root', ['upload-plan.json', 'uploaded.jsonl', 'published-tree.json', 'published-commit.json', 'published-ref.json', 'actual-publication-verification.json', 'PR160-round10-comment.md', 'published-comment.json']),
        ('round10-independent-review', '/dev/shm/p8-G4-receivers-independent-pr-audit', ['round10-archive-audit.json']),
        ('round11-PR-monitor', '/workspace/scratch/2eaa00d0f93a/p8-round11-pr-management-root', ['open-pr-and-queue-observation.json']),
        ('G4-runtime-monitor', '/workspace/scratch/2eaa00d0f93a/p8-formal-evidence-G4/root-monitor', ['observation-20261008-2343.json']),
    ]
    for prefix, root, names in groups:
        for name in names:
            add(prefix + '/' + name, Path(root) / name)

    original_zip = Path(zip_row['source_path']).read_bytes()
    assert len(original_zip) == zip_row['bytes'] and sha(original_zip) == zip_row['sha256']
    with zipfile.ZipFile(io.BytesIO(original_zip)) as z:
        assert z.testzip() is None and len(z.infolist()) == 12
        expected_members = load(Path(zip_row['source_path']).parent / 'member-hashes.json')
        assert set(z.namelist()) == set(expected_members)
        for name in z.namelist():
            b = z.read(name)
            assert len(b) == expected_members[name]['bytes'] and sha(b) == expected_members[name]['sha256']
            private_url_check(b)

    part_rows = []
    reassembled = hashlib.sha256()
    for index, offset in enumerate(range(0, len(original_zip), 2621440)):
        name = f'peer6e3-100k-failed.original.zip.part-{index:03d}'
        chunk = original_zip[offset:offset + 2621440]
        with (HERE / name).open('xb') as f:
            f.write(chunk)
        saved = (HERE / name).read_bytes()
        assert saved == chunk
        reassembled.update(saved)
        part_rows.append({'path': name, 'offset': offset, 'bytes': len(chunk), 'sha256': sha(chunk)})
    assert reassembled.hexdigest() == zip_row['sha256']

    archive = HERE / 'round11-evidence.tar.gz'
    with archive.open('xb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0, compresslevel=9) as gz, tarfile.open(fileobj=gz, mode='w|') as tar:
        for name, b in sorted(members.items()):
            item = tarfile.TarInfo(name)
            item.size = len(b)
            item.mode = 0o644
            item.mtime = 0
            tar.addfile(item, io.BytesIO(b))
    with tarfile.open(archive, 'r:gz') as tar:
        assert len(tar.getmembers()) == len(members)
        for item in tar.getmembers():
            assert item.isfile() and tar.extractfile(item).read() == members[item.name]
    for name, row in originals.items():
        b = Path(row['source_path']).read_bytes()
        assert len(b) == row['bytes'] and sha(b) == row['sha256']
    assert Path(zip_row['source_path']).read_bytes() == original_zip

    manifest = {
        'schema': 'p8-round11-evidence-checkpoint-v1',
        'TODO_closed': 0, 'TODO_remaining': 29,
        'scope': 'Original failed studies, scoped CI/engineering/source reviews and pending-receiver preparation. No original task is closed.',
        'archive': {'path': archive.name, 'bytes': archive.stat().st_size, 'sha256': sha(archive.read_bytes()), 'members': len(members), 'uncompressed_bytes': sum(map(len, members.values()))},
        'member_identities': originals,
        'original_peer_zip': {'artifact_id': 11586146906, 'run_id': 37824742267, 'source': '6e3eb4fd97edcf238774e74b88d397e059337bac', 'bytes': len(original_zip), 'sha256': sha(original_zip), 'member_count': 12, 'member_uncompressed_bytes': 20425548, 'parts': part_rows, 'restoration': 'Concatenate the ordered parts byte-for-byte; require the whole original ZIP SHA256. No original member is rewritten.'},
        'byte_roundtrip_verified': True, 'original_inputs_unchanged': True, 'signed_private_urls_included': False,
        'product_or_Cargo_execution': False, 'new_measurement': False,
    }
    (HERE / 'round11-manifest.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    print(json.dumps({'archive': manifest['archive'], 'original_zip_parts': part_rows, 'original_inputs_unchanged': True}))

if __name__ == '__main__':
    main()
