#!/usr/bin/env python3
"""Verify both completed evidence archives without extracting or executing them."""
import hashlib
import json
from pathlib import Path
import tarfile

root = Path(__file__).resolve().parent
manifest = json.loads((root / 'manifest.json').read_bytes())
count = 0
for item in manifest['archives']:
    archive = root / item['path']
    data = archive.read_bytes()
    assert len(data) == item['bytes']
    assert hashlib.sha256(data).hexdigest() == item['sha256']
    rows = {row['name']: row for row in item['files']}
    assert len(rows) == len(item['files'])
    total = 0
    with tarfile.open(archive, 'r:gz') as source:
        members = source.getmembers()
        assert len(members) == len(rows)
        assert {member.name for member in members} == set(rows)
        for member in members:
            assert member.isfile()
            assert not member.name.startswith('/') and '..' not in Path(member.name).parts
            raw = source.extractfile(member).read()
            row = rows[member.name]
            assert len(raw) == row['bytes'] and hashlib.sha256(raw).hexdigest() == row['sha256']
            assert member.mode == row['mode']
            total += len(raw)
    assert total == item['uncompressed_bytes']
    count += len(rows)
print(json.dumps({'verified_archives': len(manifest['archives']), 'verified_files': count,
                  'original_todos': manifest['original_todos']}))
