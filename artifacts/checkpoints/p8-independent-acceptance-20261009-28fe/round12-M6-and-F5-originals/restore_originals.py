#!/usr/bin/env python3
"""Reconstruct the exact reviewed 90-file logical package; execute no payload code."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def relative(value):
    if not isinstance(value, str) or not value or '\\' in value:
        raise ValueError('invalid path')
    path = PurePosixPath(value)
    if path.is_absolute() or path.as_posix() != value or any(x in ('', '.', '..') for x in path.parts):
        raise ValueError('noncanonical relative path')
    return path


def source_file(root, value):
    path = root / relative(value)
    if any(p.is_symlink() for p in [path, *list(path.parents)[:len(relative(value).parts) - 1]]):
        raise ValueError('symlink input')
    if not path.is_file() or not path.resolve(strict=True).is_relative_to(root):
        raise ValueError('missing/nonregular input')
    return path


def copy_checked(source, destination, expected):
    digest = hashlib.sha256()
    count = 0
    with source.open('rb') as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(data); count += len(data); destination.write(data)
    if count != expected['bytes'] or digest.hexdigest() != expected['sha256']:
        raise ValueError('source segment size or SHA differs')
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True,
                        help='new directory that must not already exist')
    args = parser.parse_args()
    root = Path(__file__).resolve().parent
    manifest = json.loads((root / 'logical-package-manifest.json').read_text(), object_pairs_hook=unique_object)
    if manifest['schema'] != 'p8-original-runtime-segment-transport-v1' or manifest['logical_file_count'] != 90:
        raise ValueError('unexpected fixed transport schema')
    entries = manifest['files']
    if len(entries) != 90 or len({x['path'] for x in entries}) != 90:
        raise ValueError('missing or duplicate logical entry')
    for entry in entries:
        relative(entry['path'])
        if not re.fullmatch('[0-9a-f]{64}', entry['sha256']) or not re.fullmatch('[0-9a-f]{40}', entry['git_blob']):
            raise ValueError('invalid digest')
        if type(entry['bytes']) is not int or entry['bytes'] < 0 or entry['mode'] != '100644':
            raise ValueError('invalid logical metadata')
        if ('payload' in entry) == ('segments' in entry):
            raise ValueError('ambiguous transport')
        if 'payload' in entry:
            source_file(root, entry['payload'])
        else:
            parts = entry['segments']
            if not parts or [x['ordinal'] for x in parts] != list(range(len(parts))):
                raise ValueError('segment order or count')
            if sum(x['bytes'] for x in parts) != entry['bytes']:
                raise ValueError('segment total differs')
            for part in parts:
                source_file(root, part['path'])
    output = args.output.absolute()
    if output.exists() or output.is_symlink() or not output.parent.is_dir():
        raise ValueError('output must be a new directory beneath an existing parent')
    receipt_path = output.with_name(output.name + '.restore-receipt.json')
    receipt_stream = receipt_path.open('x')
    receipt = {'schema_version':1, 'status':'failed', 'output':str(output),
               'payload_code_executed':False, 'native_rerun':False, 'files':[]}
    try:
        output.mkdir(exist_ok=False)
        for entry in entries:
            destination = output / relative(entry['path'])
            destination.parent.mkdir(parents=True, exist_ok=True)
            with destination.open('xb') as stream:
                if 'payload' in entry:
                    copy_checked(source_file(root, entry['payload']), stream, entry)
                else:
                    for part in entry['segments']:
                        copy_checked(source_file(root, part['path']), stream, part)
            destination.chmod(0o644)
            digest, blob = hashlib.sha256(), hashlib.sha1()
            blob.update(b'blob ' + str(entry['bytes']).encode() + b'\0')
            with destination.open('rb') as stream:
                for data in iter(lambda: stream.read(1024 * 1024), b''):
                    digest.update(data); blob.update(data)
            if destination.stat().st_size != entry['bytes'] or digest.hexdigest() != entry['sha256'] or blob.hexdigest() != entry['git_blob']:
                raise ValueError('restored logical file differs')
            receipt['files'].append({'path':entry['path'], 'bytes':entry['bytes'],
                                     'sha256':digest.hexdigest(), 'git_blob':blob.hexdigest()})
        if sorted(p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()) != sorted(x['path'] for x in entries):
            raise ValueError('restored file inventory differs')
        receipt.update(status='exact_reviewed_logical_package_restored',
                       logical_file_count=90, logical_bytes=sum(x['bytes'] for x in entries),
                       original_todos_closed=0, remaining_todos=29)
    except BaseException as error:
        receipt['error_type'] = type(error).__name__
        raise
    finally:
        json.dump(receipt, receipt_stream, indent=2)
        receipt_stream.write('\n'); receipt_stream.close()
    print(json.dumps({k:receipt[k] for k in ('status','logical_file_count','logical_bytes','output')}))


if __name__ == '__main__':
    main()
