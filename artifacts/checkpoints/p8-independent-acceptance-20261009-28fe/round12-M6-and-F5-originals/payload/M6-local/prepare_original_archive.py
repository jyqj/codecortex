#!/usr/bin/env python3
"""Preserve exact M6 originals; neither execute nor connect to a database."""
from pathlib import Path
import datetime
import hashlib
import json
import os
import stat
import tarfile
import time

BASE = Path('/tmp/codecortex-p8-m6-28fe-20261009')
OUT = Path('/workspace/scratch/28fef0db5e01/integration-validation/round12-M6-original-archive')
OUT.mkdir(exist_ok=False)


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def observed(path):
    a = path.lstat()
    if not stat.S_ISREG(a.st_mode):
        raise RuntimeError('unexpected nonregular file: ' + str(path))
    sha = digest(path)
    b = path.lstat()
    before = (a.st_dev, a.st_ino, a.st_size, a.st_mtime_ns, a.st_mode)
    after = (b.st_dev, b.st_ino, b.st_size, b.st_mtime_ns, b.st_mode)
    if before != after:
        raise RuntimeError('file changed during observation: ' + str(path))
    return dict(bytes=b.st_size, sha256=sha, inode=b.st_ino, device=b.st_dev,
                mtime_ns=b.st_mtime_ns, mode=stat.S_IMODE(b.st_mode))


def selected(roots):
    files = []
    for root in roots:
        if root.is_symlink():
            raise RuntimeError('unexpected symlink: ' + str(root))
        paths = [root] if root.is_file() else sorted(root.rglob('*'))
        for p in paths:
            if p.is_symlink():
                raise RuntimeError('unexpected symlink: ' + str(p))
            if p.is_file():
                files.append(p)
    if len(set(files)) != len(files):
        raise RuntimeError('duplicate selected input')
    return sorted(files)


def archive(name, roots, scope):
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    files = selected(roots)
    before = {p.relative_to(BASE).as_posix(): observed(p) for p in files}
    manifest = dict(schema_version=1, started_at=started, scope=scope,
                    source_commit='e95fd750d2f5052d0308c970cd5032c48b765cde',
                    files=before, file_count=len(files),
                    uncompressed_file_bytes=sum(v['bytes'] for v in before.values()),
                    native_execution=False, sqlite_connections_opened=0,
                    original_receipts_modified=False, target_caches_included=False)
    receipt = OUT / (name + '.json')
    with receipt.open('x') as stream:
        json.dump(manifest, stream, indent=2)
        stream.write('\n')
    target = OUT / (name + '.tar.xz')
    timer = time.monotonic()
    with tarfile.open(target, 'x:xz', preset=3) as tar:
        for p in files:
            tar.add(p, arcname=p.relative_to(BASE).as_posix(), recursive=False)
    after = {p.relative_to(BASE).as_posix(): observed(p) for p in files}
    if before != after or files != selected(roots):
        raise RuntimeError('source inventory changed; retained archive is not approved')
    member_inventory = {}
    with tarfile.open(target, 'r:xz') as tar:
        for member in tar:
            if not member.isfile() or member.name in member_inventory:
                raise RuntimeError('archive type or duplicate')
            stream = tar.extractfile(member)
            h = hashlib.sha256()
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                h.update(block)
            member_inventory[member.name] = dict(bytes=member.size, sha256=h.hexdigest())
    expected = {k:dict(bytes=v['bytes'], sha256=v['sha256']) for k,v in before.items()}
    if member_inventory != expected:
        raise RuntimeError('archive membership or bytes differ')
    manifest.update(status='preserved_exact_observed_bytes', source_before_after_equal=True,
                    complete_member_inventory_equal=True,
                    archive=dict(path=target.name, bytes=target.stat().st_size, sha256=digest(target)),
                    completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    elapsed_seconds=time.monotonic()-timer)
    receipt.write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps({k:manifest[k] for k in ('status','file_count','uncompressed_file_bytes','archive','elapsed_seconds')}), flush=True)
    return manifest


completed_roots = [BASE / name for name in ('build', 'backfill', 'c1', 'c4', 'c8', 'c16', 'runtime-parent-recovery')]
completed_roots += sorted(p for p in BASE.iterdir() if p.is_file())
completed = archive('m6-completed-and-initial-failures', completed_roots,
    'Actual successful fresh build, four mixed run+verify pairs, and fake semantic backfill; '
    'original failed parentless invocations and original/recovery wrappers retained separately. '
    'Recovery receipt retains its running hour and has no synthetic terminal; mixed resource coverage remains false.')
hour = BASE / 'soak/runtime'
assert not (hour / 'report.json').exists() and not (hour / 'seal.json').exists()
unresolved = archive('m6-unresolved-hour-observation', [BASE / 'soak'],
    'Read-only byte observation of the unresolved local hour after the execution session became unavailable. '
    'No CLI/native terminal, endpoint parity, seal, SQLite-consistency or global-process-quiescence claim. '
    'Includes exact observed DB/WAL/SHM bytes without opening SQLite; stable size/mtime/inode/hash only describe these reads. '
    'Known >5-second resource gaps remain failures of the observed prefix; no task closure.')
assert not (hour / 'report.json').exists() and not (hour / 'seal.json').exists()
index = dict(schema_version=1, archives=[completed['archive'], unresolved['archive']],
             original_todos_completed=0, remaining_todos=29, task_ledger_changed=False,
             completed_scope='fresh build + four actual mixed cells + fake semantic backfill only',
             hour_status='unknown_session_unavailable_no_terminal',
             generated_at=datetime.datetime.now(datetime.timezone.utc).isoformat())
(OUT / 'archive-index.json').write_text(json.dumps(index, indent=2) + '\n')
print(json.dumps(index), flush=True)
