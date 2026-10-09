#!/usr/bin/env python3
"""One scoped transport test; no extraction, measurement, or source mutation."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess
import time

BASE = Path('/workspace/scratch/a217aaae3bde')
OWNER = BASE / 'p8-original-artifact-custody'
OUT = OWNER / 'independent-review'
RAM = Path('/dev/shm/a217aaae3bde/custody-restore-backfill-11610619483-review')
SOURCE = BASE / 'original-C3ff-runtime/artifacts/11610619483/11610619483.zip'
MANIFEST = OWNER / 'manifests/11610619483.json'
RESTORE = OWNER / 'restore_original_zip.py'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def info(path):
    st = path.lstat()
    assert stat.S_ISREG(st.st_mode), str(path)
    h = hashlib.sha256()
    git = hashlib.sha1(f'blob {st.st_size}\0'.encode())
    with path.open('rb') as f:
        while b := f.read(1024 * 1024):
            h.update(b)
            git.update(b)
    end = path.lstat()
    seal = lambda s: (s.st_dev, s.st_ino, s.st_size, s.st_mtime_ns, s.st_ctime_ns, s.st_mode, s.st_nlink)
    assert seal(st) == seal(end), str(path)
    return dict(path=str(path), bytes=st.st_size, sha256=h.hexdigest(), git_blob=git.hexdigest(),
                mode=oct(stat.S_IMODE(st.st_mode)), dev=st.st_dev, inode=st.st_ino,
                nlink=st.st_nlink, mtime_ns=st.st_mtime_ns, ctime_ns=st.st_ctime_ns)

def save(name, value):
    with (OUT / name).open('x', encoding='utf-8') as f:
        json.dump(value, f, ensure_ascii=False, sort_keys=True, indent=2)
        f.write('\n')

def invoke(label, output):
    cmd = ['python3', '-B', str(RESTORE), str(RAM / 'manifests/11610619483.json'), '--output', str(output)]
    t = time.monotonic()
    result = subprocess.run(cmd, capture_output=True, timeout=60)
    elapsed = time.monotonic() - t
    for suffix, data in [('stdout', result.stdout), ('stderr', result.stderr)]:
        with (OUT / f'{label}.{suffix}').open('xb') as f:
            f.write(data)
    return dict(command=cmd, returncode=result.returncode, duration_seconds=elapsed,
                stdout=info(OUT / f'{label}.stdout'), stderr=info(OUT / f'{label}.stderr'))

def main():
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    assert not OUT.exists(), str(OUT)
    assert not RAM.exists(), str(RAM)
    OUT.mkdir()
    protected = [SOURCE, MANIFEST, RESTORE, OWNER / 'prepare_custody.py', OWNER / 'catalog-planned.json']
    before = [info(p) for p in protected]
    original = before[0]
    mbytes = MANIFEST.read_bytes()
    manifest = json.loads(mbytes)
    assert original['bytes'] == manifest['original_zip_bytes'] == 7835686
    assert original['sha256'] == manifest['original_zip_sha256']
    assert len(manifest['chunks']) == 4
    sv = os.statvfs(RAM.parent)
    free = sv.f_bavail * sv.f_frsize
    needed = 2 * original['bytes'] + len(mbytes) + 16 * 1024 * 1024
    assert free >= needed, (free, needed)
    RAM.mkdir()
    (RAM / 'chunks').mkdir()
    (RAM / 'manifests').mkdir()
    with (RAM / 'manifests/11610619483.json').open('xb') as f:
        f.write(mbytes)
    with SOURCE.open('rb') as source:
        for chunk in manifest['chunks']:
            assert source.tell() == chunk['offset']
            data = source.read(chunk['bytes'])
            assert len(data) == chunk['bytes'] and sha(data) == chunk['sha256']
            assert hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest() == chunk['git_blob']
            with (RAM / chunk['path']).open('xb') as f:
                f.write(data)
        assert not source.read(1)
    output = RAM / 'restored.zip'
    success = invoke('success', output)
    assert success['returncode'] == 0
    restored = info(output)
    assert restored['bytes'] == original['bytes'] and restored['sha256'] == original['sha256']
    with SOURCE.open('rb') as a, output.open('rb') as b:
        while left := a.read(1024 * 1024):
            assert left == b.read(len(left))
        assert not b.read(1)
    duplicate = invoke('existing-output-rejected', output)
    assert duplicate['returncode'] != 0
    assert b'FileExistsError' in (OUT / 'existing-output-rejected.stderr').read_bytes()
    assert info(output) == restored
    assert not output.with_name(output.name + '.partial').exists()
    damaged = RAM / manifest['chunks'][0]['path']
    clean_chunk = info(damaged)
    with damaged.open('r+b') as f:
        first = f.read(1)
        f.seek(0)
        f.write(bytes([first[0] ^ 1]))
    bad_chunk = info(damaged)
    assert bad_chunk['bytes'] == clean_chunk['bytes'] and bad_chunk['sha256'] != clean_chunk['sha256']
    bad_output = RAM / 'corrupt-output.zip'
    corruption = invoke('corrupt-chunk-rejected', bad_output)
    assert corruption['returncode'] != 0
    assert b'chunk byte count or digest mismatch' in (OUT / 'corrupt-chunk-rejected.stderr').read_bytes()
    assert not bad_output.exists() and not bad_output.with_name(bad_output.name + '.partial').exists()
    assert info(output) == restored
    after = [info(p) for p in protected]
    assert after == before
    current = sorted(p for p in RAM.rglob('*') if p.is_file())
    expected = sorted([RAM / 'manifests/11610619483.json', output] + [RAM / c['path'] for c in manifest['chunks']])
    assert current == expected
    assert all(not p.is_symlink() for p in RAM.rglob('*'))
    candidate = [info(p) for p in current]
    save('cleanup-candidate.json', dict(root=str(RAM), files=candidate,
         source_original_preserved=True, modified_test_chunk=str(damaged),
         active_writers=False, boundary='Only this process-created transport test directory; all child CLIs exited.'))
    assert [info(p) for p in current] == candidate
    assert sorted(p for p in RAM.rglob('*') if p.is_file()) == current
    for p in current:
        p.unlink()
    for p in sorted((p for p in RAM.rglob('*') if p.is_dir()), key=lambda p: len(p.parts), reverse=True):
        p.rmdir()
    RAM.rmdir()
    assert [info(p) for p in protected] == before
    save('execution-and-cleanup.json', dict(schema='original-zip-custody-restore-controls-v1',
         started_at=started, completed_at=datetime.datetime.now(datetime.timezone.utc).isoformat(),
         status='passed_scoped_transport_controls', native_execution=False, task_statuses_changed=False,
         original=original, manifest=before[1], restore_script=before[2], prepare_script=before[3],
         catalog=before[4], ram_free_before=free, ram_required=needed, tests=[success, duplicate, corruption],
         restored=restored, exact_source_bytes_equal=True, clean_chunk=clean_chunk, damaged_chunk=bad_chunk,
         protected_before=before, protected_after=after, protected_final_unchanged=True,
         cleanup=dict(removed_files=len(current), removed_bytes=sum(x['bytes'] for x in candidate),
                      root_removed=not RAM.exists(), only_created_test_files=True),
         scope='One original backfill ZIP transport restoration; no archive extraction, native measurement, or task acceptance replay.'))
    print(json.dumps({'status':'passed','tests':3,'artifact_id':11610619483,'bytes':original['bytes'],
                      'output':str(OUT / 'execution-and-cleanup.json')}))

if __name__ == '__main__':
    main()
