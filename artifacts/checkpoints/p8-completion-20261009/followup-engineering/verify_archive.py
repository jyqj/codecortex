#!/usr/bin/env python3
"""Verify all retained archive bytes without extracting or executing evidence."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tarfile

def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''): value.update(block)
    return value.hexdigest()

def require(value, message):
    if not value: raise ValueError(message)

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--directory', type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument('--git-repository', type=Path,
                        help='Optional existing repository containing the public G prerequisite')
    args = parser.parse_args()
    directory = args.directory.resolve(strict=True)
    manifest = json.loads((directory / 'manifest.json').read_text())
    delivery = json.loads((directory / 'DELIVERABLES.json').read_text())
    for name, record in delivery['files'].items():
        require(PurePosixPath(name).name == name, 'non-flat deliverable path')
        path = directory / name
        require(path.is_file() and not path.is_symlink(), 'missing/nonregular deliverable: ' + name)
        require(path.stat().st_size == record['bytes'] and digest(path) == record['sha256'],
                'changed deliverable: ' + name)
    expected_files, expected_dirs = manifest['files'], manifest['directories']
    seen_files, seen_dirs, total = set(), set(), 0
    with tarfile.open(directory / 'engineering-originals.tar.gz', 'r:gz') as archive:
        for member in archive:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and '..' not in path.parts and member.name.startswith('evidence/'),
                    'unsafe archive member')
            name = member.name.removeprefix('evidence/')
            if member.isdir():
                require(name in expected_dirs and name not in seen_dirs and member.mode == expected_dirs[name],
                        'directory inventory/mode mismatch: ' + name)
                seen_dirs.add(name)
                continue
            require(member.isfile() and name in expected_files and name not in seen_files,
                    'nonregular, duplicate or unlisted member: ' + name)
            record = expected_files[name]
            require(member.size == record['bytes'] and member.mode == record['mode'], 'size/mode mismatch: ' + name)
            stream = archive.extractfile(member)
            hashed, count = hashlib.sha256(), 0
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                hashed.update(block); count += len(block)
            require(count == record['bytes'] and hashed.hexdigest() == record['sha256'], 'byte mismatch: ' + name)
            seen_files.add(name)
            total += count
    require(seen_files == set(expected_files) and seen_dirs == set(expected_dirs), 'missing archive members')
    require(total == manifest['file_bytes'] and len(seen_files) == manifest['file_count'], 'file totals differ')
    bundle = directory / 'followup-local-commits.bundle'
    with bundle.open('rb') as stream:
        header = []
        while line := stream.readline():
            if line == b'\n': break
            require(len(line) < 8192 and len(header) < 20, 'oversized bundle header')
            header.append(line.decode().rstrip('\n'))
    prerequisites = [line[1:41] for line in header if line.startswith('-')]
    require(prerequisites == [manifest['source_identity']['public_prerequisite_G']], 'bundle prerequisites differ')
    git_checked = False
    if args.git_repository:
        result = subprocess.run(['git', '-C', str(args.git_repository), 'bundle', 'verify', str(bundle)],
                                capture_output=True, text=True, timeout=30)
        require(result.returncode == 0, 'Git rejected the bundle: ' + result.stderr)
        git_checked = True
    print(json.dumps({'status': 'verified_original_bytes', 'files': len(seen_files),
                      'directories': len(seen_dirs), 'bytes': total, 'git_bundle_verified': git_checked,
                      'product_or_new_source_execution_claim': False}))

if __name__ == '__main__': main()
