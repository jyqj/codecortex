#!/usr/bin/env python3
"""Verify the exact inspection archive without extracting or executing it."""
import hashlib
import json
from pathlib import Path
import tarfile

root = Path(__file__).resolve().parent
manifest = json.loads((root / 'manifest.json').read_bytes())
archive = root / manifest['archive']['path']
data = archive.read_bytes()
assert len(data) == manifest['archive']['bytes']
assert hashlib.sha256(data).hexdigest() == manifest['archive']['sha256']
expected = {row['name']: row for row in manifest['files']}
assert len(expected) == len(manifest['files']) == manifest['archive']['files']
total = 0
with tarfile.open(archive, 'r:gz') as source:
    members = source.getmembers()
    assert len(members) == len(expected)
    assert {member.name for member in members} == set(expected)
    for member in members:
        assert member.isfile()
        value = source.extractfile(member).read()
        row = expected[member.name]
        assert len(value) == row['bytes']
        assert hashlib.sha256(value).hexdigest() == row['sha256']
        total += len(value)
assert total == manifest['archive']['uncompressed_file_bytes']
print(json.dumps({'verified_files': len(expected), 'verified_file_bytes': total,
                  'task_complete': False, 'remaining_original_todos': 29}))
