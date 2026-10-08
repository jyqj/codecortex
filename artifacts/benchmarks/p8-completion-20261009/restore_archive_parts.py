"""Restore an exact evidence archive without extracting any of its members.

Usage: python restore_archive_parts.py path/to/archive-parts.json [output-path]
Existing output files are never overwritten.
"""
import hashlib
import json
from pathlib import Path
import sys


manifest_path = Path(sys.argv[1]).resolve()
manifest = json.loads(manifest_path.read_text())
assert manifest['kind'] == 'ordered_byte_exact_archive_parts'
name = manifest['restore_filename']
assert Path(name).name == name and name not in ('', '.', '..')
output = Path(sys.argv[2]) if len(sys.argv) == 3 else manifest_path.parent.parent / name
assert len(sys.argv) in (2, 3)
assert not output.exists(), 'Refusing to overwrite an existing output file'
parts = manifest['parts']
assert parts and len({p['path'] for p in parts}) == len(parts)
for part in parts:
    assert Path(part['path']).name == part['path'] and part['path'] not in ('', '.', '..')
    assert (manifest_path.parent / part['path']).stat().st_size == part['bytes']
created = False
try:
    whole, total = hashlib.sha256(), 0
    with output.open('xb') as target:
        created = True
        for part in parts:
            digest, count = hashlib.sha256(), 0
            with (manifest_path.parent / part['path']).open('rb') as source:
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    target.write(block)
                    digest.update(block)
                    whole.update(block)
                    count += len(block)
                    total += len(block)
            assert count == part['bytes'] and digest.hexdigest() == part['sha256'], part['path']
    assert total == manifest['original_archive_bytes']
    assert whole.hexdigest() == manifest['original_archive_sha256'] == manifest['combined_parts_sha256']
except BaseException:
    if created:
        output.unlink()
    raise
print(json.dumps({'restored': str(output), 'bytes': total, 'sha256': whole.hexdigest(),
                  'archive_members_extracted': False}, indent=2))
