#!/usr/bin/env python3
"""Restore the exact original artifact ZIP from verified ordered byte parts."""
import argparse
import hashlib
import json
from pathlib import Path

def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    spec = json.loads((here / 'round11-manifest.json').read_bytes())['original_peer_zip']
    offset = 0
    for part in spec['parts']:
        path = here / part['path']
        assert path.is_file() and not path.is_symlink()
        assert part['offset'] == offset and path.stat().st_size == part['bytes']
        assert digest(path) == part['sha256'], 'part digest differs'
        offset += part['bytes']
    assert offset == spec['bytes']
    actual = hashlib.sha256()
    written = 0
    with args.output.open('xb') as output:
        for part in spec['parts']:
            with (here / part['path']).open('rb') as source:
                while block := source.read(1024 * 1024):
                    output.write(block)
                    actual.update(block)
                    written += len(block)
    assert written == spec['bytes'] and actual.hexdigest() == spec['sha256']
    assert digest(args.output) == spec['sha256']
    print(json.dumps({'original_artifact_id': spec['artifact_id'], 'output': str(args.output), 'bytes': written, 'sha256': actual.hexdigest()}))

if __name__ == '__main__':
    main()
