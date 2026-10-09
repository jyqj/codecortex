#!/usr/bin/env python3
"""Retain all cache engineering attempts and real small-probe raw records."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent
ROOTS = {
    'candidate': Path('/dev/shm/p8-soak-cache-candidate-G2'),
    'independent-review': Path('/dev/shm/p8-soak-cache-independent-review'),
    'actual-small-probe': Path('/dev/shm/p8-soak-cache-real-probe-root'),
}
EXCLUDED = {'actual-small-probe/retained-G-codecortex'}

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    files = {}
    data = {}
    for prefix, root in ROOTS.items():
        for path in sorted(root.rglob('*')):
            assert not path.is_symlink(), path
            if not path.is_file():
                continue
            name = prefix + '/' + path.relative_to(root).as_posix()
            if name in EXCLUDED:
                continue
            raw = path.read_bytes()
            data[name] = raw
            files[name] = {'bytes': len(raw), 'sha256': sha(raw), 'mode': path.stat().st_mode & 0o777}
    assert len(files) > 80
    assert sum(x['bytes'] for x in files.values()) < 32 * 1024 * 1024
    archive = HERE / 'cache-engineering-originals.tar.gz'
    with archive.open('xb') as output:
        with gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0) as gz:
            with tarfile.open(fileobj=gz, mode='w') as tar:
                for name, raw in sorted(data.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(raw), files[name]['mode'], 0
                    tar.addfile(info, io.BytesIO(raw))
    raw = archive.read_bytes()
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        assert len(members) == len(files) and {x.name for x in members} == set(files)
        for member in members:
            assert member.isfile() and not member.name.startswith('/') and '..' not in Path(member.name).parts
            body = tar.extractfile(member).read()
            assert len(body) == files[member.name]['bytes'] and sha(body) == files[member.name]['sha256']
            assert member.mode == files[member.name]['mode']
    manifest = {
        'schema_version': 1, 'scope': 'Source review, protocol controls and an explicitly separate small actual-product probe; not formal soak acceptance',
        'archive': archive.name, 'archive_bytes': len(raw), 'archive_sha256': sha(raw),
        'file_count': len(files), 'original_bytes': sum(x['bytes'] for x in files.values()),
        'all_members_actual_roundtrip_verified': True, 'files': files,
        'binary_reference': {'included_in_this_archive': False, 'source': 'bb9a96d71622458c39a143055360cc97f0d11d78',
                             'actions_artifact_id': 11573313154, 'member': 'product/codecortex',
                             'bytes': 27060952, 'sha256': '9ebbd85103ae3d4e88fc13d2fcf6144438a0dfbcfba44cbe6a4e0232bf8a8f7e'},
        'preserves_earlier_failed_and_superseded_attempts': True,
        'todo_closed': 0, 'todo_remaining': 29,
    }
    (HERE / 'cache-engineering-manifest.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    print(json.dumps({k: v for k, v in manifest.items() if k not in ('files', 'binary_reference')}))

if __name__ == '__main__':
    main()
