#!/usr/bin/env python3
"""Verify all archived observations and direct mirrors without extraction."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import tarfile

ROOT = Path(__file__).resolve().parent

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def main():
    manifest = json.loads((ROOT / 'checkpoint-files.json').read_bytes())
    archive = manifest['archive']
    raw = (ROOT / archive['path']).read_bytes()
    assert len(raw) == archive['bytes'] and sha(raw) == archive['sha256']
    records = {row['path']: row for row in manifest['files']}
    assert len(records) == len(manifest['files']) == manifest['original_file_count'] == 25
    mirrors = []
    with tarfile.open(ROOT / archive['path'], 'r:gz') as tar:
        members = tar.getmembers()
        assert len(members) == len(records) and {m.name for m in members} == set(records)
        for member in members:
            assert member.isfile() and PurePosixPath(member.name).name == member.name
            row = records[member.name]
            body = tar.extractfile(member).read()
            assert len(body) == row['bytes'] and sha(body) == row['sha256']
            if row['also_direct']:
                assert (ROOT / member.name).read_bytes() == body
                mirrors.append(member.name)
    assert sorted(mirrors) == sorted(manifest['direct_mirrors'])
    print(json.dumps({'status': 'passed', 'original_files_verified': len(records),
                      'direct_mirrors_verified': len(mirrors), 'archive_sha256': archive['sha256'],
                      'scope': 'Storage fixity only; not a test/product rerun or new acceptance.'}, sort_keys=True))

if __name__ == '__main__':
    main()
