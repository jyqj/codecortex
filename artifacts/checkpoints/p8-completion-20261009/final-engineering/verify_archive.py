#!/usr/bin/env python3
"""Verify all delivery hashes and tar members; optionally verify preserved runtime pairs."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile


def sha(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--runtime', action='store_true', help='Extract to an owned temporary directory and invoke both preserved offline verifiers; no product load')
    parser.add_argument('--repo', type=Path, help='Existing Git repository containing the declared BASE prerequisite, for git bundle verify')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    delivery = json.loads((root / 'DELIVERABLES.json').read_text())['files']
    for name, record in delivery.items():
        path = root / name
        assert path.is_file() and path.stat().st_size == record['bytes'] and sha(path) == record['sha256'], name
    manifest = json.loads((root / 'files.manifest.json').read_text())['files']
    seen = set()
    pairs = []
    with tempfile.TemporaryDirectory(prefix='p8-engineering-archive-check-') as temporary:
        extracted = Path(temporary)
        for archive in sorted({entry['archive'] for entry in manifest.values()}):
            with tarfile.open(root / archive, 'r:gz') as tar:
                for member in tar:
                    assert member.isfile() and member.name in manifest and member.name not in seen, member.name
                    relative = Path(member.name)
                    assert not relative.is_absolute() and '..' not in relative.parts
                    record = manifest[member.name]
                    assert record['archive'] == archive and member.size == record['bytes'] and member.mode == record['mode']
                    actual_hash = hashlib.sha256()
                    actual_size = 0
                    destination = extracted / relative
                    output = None
                    if args.runtime:
                        destination.parent.mkdir(parents=True, exist_ok=True)
                        output = destination.open('xb')
                    try:
                        with tar.extractfile(member) as incoming:
                            for block in iter(lambda: incoming.read(1024 * 1024), b''):
                                actual_hash.update(block)
                                actual_size += len(block)
                                if output is not None:
                                    output.write(block)
                    finally:
                        if output is not None:
                            output.close()
                            destination.chmod(member.mode)
                    assert actual_size == record['bytes'] and actual_hash.hexdigest() == record['sha256'], member.name
                    seen.add(member.name)
        assert seen == set(manifest), 'Missing archived evidence'
        if args.runtime:
            for source in ('c6d5edba', 'bce4f18c'):
                build = extracted / f'p8-runtime-build-{source}'
                output = extracted / f'p8-runtime-short-{source}'
                observer = build / 'observer-source/scripts/p8_runtime.py'
                command = [sys.executable, '-B', str(observer), 'verify', '--output', str(output), '--build-output', str(build)]
                result = subprocess.run(command, capture_output=True, text=True)
                assert result.returncode == 0, dict(command=command, stdout=result.stdout, stderr=result.stderr)
                verified = json.loads(result.stdout)
                assert verified['status'] == 'sealed_artifacts_verified'
                pairs.append(dict(source=source, result=verified))
    bundle = None
    if args.repo:
        result = subprocess.run(['git', 'bundle', 'verify', str(root / 'sources.bundle')], cwd=args.repo, capture_output=True, text=True)
        assert result.returncode == 0, result.stderr
        bundle = dict(exit_code=0, stdout=result.stdout, stderr=result.stderr)
    print(json.dumps(dict(status='archive_bytes_verified', files=len(seen), deliverables=len(delivery), runtime_pairs=pairs, bundle_verification=bundle), indent=2))


if __name__ == '__main__':
    main()
