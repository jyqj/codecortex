#!/usr/bin/env python3
"""Restore the original eleven archive files into a new directory, checking every byte."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import stat


def regular(path):
    assert stat.S_ISREG(path.lstat().st_mode), f'Nonregular transport file: {path}'
    return path


def checked_path(root, relative):
    path = PurePosixPath(relative)
    assert not path.is_absolute() and '..' not in path.parts and path.parts, relative
    value = root.joinpath(*path.parts)
    assert value.resolve(strict=True).is_relative_to(root)
    return regular(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path, help='New directory; existing paths are never overwritten')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    manifest = json.loads(regular(root / 'transport-manifest.json').read_text())
    assert manifest['schema_version'] == 1 and manifest['chunk_size_bytes'] == 4194303
    assert manifest['chunk_size_bytes'] % 3 == 0 and manifest['chunk_size_multiple_of_three'] is True
    entries = manifest['files']
    assert len(entries) == 11
    expected_chunked = {'evidence-part-001.tar.gz', 'evidence-part-002.tar.gz', 'sources.bundle'}
    assert {name for name, entry in entries.items() if entry['kind'] == 'chunked'} == expected_chunked
    output = args.output.absolute()
    output.mkdir(parents=True, exist_ok=False)
    restored = {}
    seen_chunks = set()
    for name, entry in sorted(entries.items()):
        assert PurePosixPath(name).name == name and name not in ('', '.', '..'), name
        aggregate = hashlib.sha256()
        total = 0
        destination = output / name
        if entry['kind'] == 'direct':
            assert entry['path'] == name
            blocks = [dict(path=entry['path'], index=1, offset=0, bytes=entry['bytes'], sha256=entry['sha256'])]
        else:
            assert entry['kind'] == 'chunked' and entry['chunks']
            blocks = entry['chunks']
        with destination.open('xb') as out:
            for index, chunk in enumerate(blocks, 1):
                assert type(chunk['index']) is int and chunk['index'] == index
                assert type(chunk['offset']) is int and chunk['offset'] == total
                data = checked_path(root, chunk['path']).read_bytes()
                assert len(data) == chunk['bytes'] and hashlib.sha256(data).hexdigest() == chunk['sha256'], chunk['path']
                if entry['kind'] == 'chunked':
                    assert chunk['path'] not in seen_chunks
                    seen_chunks.add(chunk['path'])
                    assert 0 < len(data) <= manifest['chunk_size_bytes']
                    assert index == len(blocks) or len(data) == manifest['chunk_size_bytes']
                out.write(data)
                aggregate.update(data)
                total += len(data)
        assert total == entry['bytes'] and aggregate.hexdigest() == entry['sha256'], name
        assert type(entry['mode']) is int and 0 <= entry['mode'] <= 0o777
        destination.chmod(entry['mode'])
        restored[name] = dict(bytes=total, sha256=aggregate.hexdigest())
    print(json.dumps(dict(status='original_archive_reassembled', files=restored, chunk_count=len(seen_chunks), output=str(output)), indent=2))


if __name__ == '__main__':
    main()
