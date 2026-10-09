#!/usr/bin/env python3
"""Deterministic, complete-file PR and CI evidence snapshot; no remote writes."""
import datetime
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import tarfile

WORK = Path('/workspace/scratch/a217aaae3bde')
AUDIT = Path('/dev/shm/a217aaae3bde/platform-review')
OUT = WORK / 'checkpoint-round16-prep/pr-management-ci-payload'
TARGET = OUT / 'pr-management-ci-evidence.tar.gz'
assert not TARGET.exists()
selected = {}
excluded = []

def add(path, member):
    path = Path(path)
    assert path.is_file() and not path.is_symlink(), path
    assert member not in selected and not member.startswith('/') and '..' not in Path(member).parts
    selected[member] = path

for directory, prefix in [(AUDIT / 'pr165-ci-review', 'pr165/ci'),
                          (AUDIT / 'pr166-archive-review', 'pr166/archive-review')]:
    for path in sorted(directory.iterdir()):
        if not path.is_file():
            continue
        if path.name == 'official-status-polls.jsonl' or path.name.endswith('-last-poll-state.json'):
            add(path, 'monitor-snapshots/' + prefix + '/' + path.name)
        else:
            add(path, prefix + '/' + path.name)

directory = AUDIT / 'pr167-execution'
for path in sorted(directory.iterdir()):
    if not path.is_file():
        continue
    if path.name == 'official-status-polls.jsonl' or path.name.endswith('-last-poll-state.json'):
        add(path, 'monitor-snapshots/pr167/' + path.name)
    elif path.name.endswith('-official.json') and path.stat().st_size == 0:
        excluded.append({'path': str(path), 'reason': 'Prior ENOSPC interrupted metadata write; zero bytes is not an official response or completed evidence.'})
    else:
        add(path, 'pr167/' + path.name)

for path in sorted((directory / 'p7-e-originals-review').iterdir()):
    if path.is_file():
        add(path, 'pr167/p7-e-originals-review/' + path.name)
for relative in ['gates-e-review/review.json',
                 'recovery-e-review/recovery-e-review.json',
                 'recovery-e-review/recovery-e-011-review.json',
                 'platform-e-complete-review/platform-e-cell-review.json',
                 'platform-e-complete-review/independent-collected-matrix.json',
                 'platform-e-cli-collector/matrix.json']:
    add(directory / relative, 'pr167/' + relative)
for path in [Path('/dev/shm/a217aaae3bde/runtime-review/e-p7-helper-scoped-independent-review.json'),
             Path('/dev/shm/a217aaae3bde/runtime-review/e-platform-helper-scoped-independent-review.json'),
             Path('/dev/shm/a217aaae3bde/runtime-review/e-recovery-helpers-scoped-independent-review.json'),
             Path('/dev/shm/a217aaae3bde/scale-combined-E-review/gates-helper-independent-review/independent-review.json')]:
    add(path, 'pr167/independent-helper-reviews/' + (path.name if path.name != 'independent-review.json' else 'gates-independent-review.json'))

root165 = WORK / 'archive-relocation-pr'
for name in ['independent-actual-tree-review.json', 'root-proposed-tree-review.json',
             'official-base-tree-metadata.json', 'created-commit-and-base.json',
             'created-branch-result.json', 'created-pr-result.json',
             'normal-merge-attempt-1791508026862.json',
             'main-after165-1791508119771.json', 'merge165-commit-1791508119771.json']:
    add(root165 / name, 'pr165/root-management/' + name)
for path in sorted((root165 / 'payload').rglob('*')):
    if path.is_file():
        add(path, 'pr165/provenance-proposal/' + path.relative_to(root165 / 'payload').as_posix())
root159 = WORK / 'pr159-engineering-merge'
for name in ['normal-merge-result.json', 'actual-main-1791508336442.json',
             'actual-merge-commit-1791508336442.json', 'pr159-merged-1791508336442.json',
             'issue154-1791508336442.json', 'ready-result.json']:
    add(root159 / name, 'pr159/root-management/' + name)
candidate = WORK / 'combined-scale-cache-candidate'
for name in ['published-pr-result.json', 'official-execution-commit.json',
             'official-execution-root-tree.json', 'official-product-commit.json',
             'official-review-commit.json', 'execution-commit-created.json',
             'pr166-normal-merge-result.json', 'pr167-mark-ready-response.json']:
    add(candidate / name, 'pr167/root-management/' + name)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def blob(data):
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()

patterns = [re.compile(r'https?[^\s"<>]+[?&](?:sig|signature|X-Amz-Signature|token|access_token)=', re.I),
            re.compile(r'private-transfer', re.I),
            re.compile(r'"download_(?:ref|reference)"\s*:', re.I)]
data_by_name = {}
members = []
for member, path in sorted(selected.items()):
    before = path.stat()
    content = path.read_bytes()
    after = path.stat()
    assert (before.st_ino, before.st_size, before.st_mtime_ns) == (after.st_ino, after.st_size, after.st_mtime_ns), path
    text = content.decode('utf-8')
    assert not any(pattern.search(text) for pattern in patterns), ('restricted transfer reference or credential URL', path)
    data_by_name[member] = content
    members.append({'member': member, 'source_path': str(path), 'source_mode': format(stat.S_IMODE(before.st_mode), '04o'),
                    'archive_mode': '0644', 'bytes': len(content), 'sha256': sha(content), 'git_blob_sha1': blob(content),
                    'monitor_snapshot_only': member.startswith('monitor-snapshots/')})

def encode_archive():
    destination = io.BytesIO()
    with gzip.GzipFile(fileobj=destination, mode='wb', filename='', mtime=0, compresslevel=9) as zipped:
        with tarfile.open(fileobj=zipped, mode='w', format=tarfile.USTAR_FORMAT) as archive:
            for member in sorted(data_by_name):
                content = data_by_name[member]
                info = tarfile.TarInfo(member)
                info.size = len(content)
                info.mode = 0o644
                info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ''
                archive.addfile(info, io.BytesIO(content))
    return destination.getvalue()

encoded = encode_archive()
second = encode_archive()
assert encoded == second
with TARGET.open('xb') as stream:
    stream.write(encoded)
assert TARGET.read_bytes() == encoded
with tarfile.open(TARGET, 'r:gz') as archive:
    actual = archive.getmembers()
    assert [row.name for row in actual] == sorted(data_by_name)
    for row in actual:
        assert row.isfile() and row.mode == 0o644 and row.uid == row.gid == row.mtime == 0
        assert row.uname == row.gname == ''
        readback = archive.extractfile(row).read()
        assert readback == data_by_name[row.name]
        expected = next(x for x in members if x['member'] == row.name)
        assert sha(readback) == expected['sha256'] and blob(readback) == expected['git_blob_sha1']
for member, path in selected.items():
    assert path.read_bytes() == data_by_name[member], ('source changed during packaging', path)

parts = []
if len(encoded) > 4 * 1024 * 1024:
    for index, offset in enumerate(range(0, len(encoded), 4 * 1024 * 1024)):
        data = encoded[offset:offset + 4 * 1024 * 1024]
        path = OUT / (TARGET.name + f'.part-{index:03d}')
        with path.open('xb') as stream:
            stream.write(data)
        assert path.read_bytes() == data
        parts.append({'path': path.name, 'offset': offset, 'bytes': len(data), 'sha256': sha(data), 'git_blob_sha1': blob(data)})
    assert b''.join((OUT / p['path']).read_bytes() for p in parts) == encoded

capacity = os.statvfs(OUT)
manifest = {
    'schema_version': 1,
    'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'packager': '/root/pr_audit',
    'scope': 'Complete immutable PR165 eight-job, PR166 archive-only, PR167 nineteen-job and scoped original artifact review files, with root normal merge identities. Dynamic monitor records are isolated point-in-time snapshots.',
    'source_and_workflow_identity': 'E=a23bb72d; original CI/engineering checkout75649 with tree58147 preserved; historical PR165/166 records retain their own original identities.',
    'evidence_archive': {'path': TARGET.name, 'bytes': len(encoded), 'sha256': sha(encoded), 'git_blob_sha1': blob(encoded),
        'mode': '100644', 'format': 'USTAR inside gzip, sorted names, file mode0644, uid/gid/mtime0, uname/gname/filename empty, compression9',
        'actual_members_reopened_and_verified': True, 'actual_second_generation_byte_equal': True, 'members': members},
    'delivery_parts_if_required': parts,
    'archive_whole_retained_locally_even_when_parts_used': True,
    'excluded_incomplete_metadata_writes': excluded,
    'no_original_file_bytes_redacted_or_truncated': True,
    'restricted_transfer_references_and_credential_URL_matches': 0,
    'public_GitHub_API_and_original_public_job_log_noncredential_URLs_retained': True,
    'original_source_files_reopened_after_packaging_unchanged': True,
    'not_included': ['Original artifact ZIPs and native binaries remain separately retained; this is a PR/CI/review evidence package, not an original-artifact replacement.',
                     'Derived extracted roots, collector-input hardlinks, copied native replay binaries and source checkout duplicates are excluded.'],
    'packaging_helper': {'path': Path(__file__).name, 'sha256': sha(Path(__file__).read_bytes()), 'git_blob_sha1': blob(Path(__file__).read_bytes())},
    'capacity_after_write': {'bavail_bytes': capacity.f_bavail * capacity.f_frsize, 'bfree_bytes': capacity.f_bfree * capacity.f_frsize, 'effective_uid': os.geteuid()},
    'remote_writes_performed': False,
    'task_completion_certified': False, 'done': 163, 'remaining': 29,
}
manifest_path = OUT / 'payload-manifest.json'
manifest_path.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n')
print(json.dumps({'archive': TARGET.name, 'bytes': len(encoded), 'sha256': sha(encoded), 'git_blob_sha1': blob(encoded),
                  'members': len(members), 'raw_member_bytes': sum(x['bytes'] for x in members), 'parts': len(parts),
                  'manifest_sha256': sha(manifest_path.read_bytes())}))
