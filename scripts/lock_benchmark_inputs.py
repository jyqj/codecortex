#!/usr/bin/env python3
"""Prepare or verify an offline evidence-input lock, without executing a benchmark.

This complements cc-eval's existing suite/run manifests. It preserves supplied
build/execution receipt relationships; it does not authenticate those receipts,
certify a release, collect process environment, or turn unknown values into zero.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat

REQUIRED_ROLES = {
    'binary', 'config', 'corpus', 'queries', 'scoring', 'model', 'environment',
    'source', 'source_receipt', 'build_receipt', 'execution_receipt', 'report',
}
ALLOWED_ROLES = REQUIRED_ROLES | {'supporting_evidence'}
FORMAT = 'codecortex-evidence-input-lock-v1'


class LockError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise LockError(message)


def json_bytes(value):
    return (json.dumps(value, sort_keys=True, indent=2, ensure_ascii=True, allow_nan=False) + '\n').encode()


def parse_json(data):
    def pairs(values):
        result = {}
        for key, value in values:
            require(key not in result, 'duplicate JSON key')
            result[key] = value
        return result

    def constant(_):
        raise LockError('non-finite JSON value')

    return json.loads(data, object_pairs_hook=pairs, parse_constant=constant)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def relative(value):
    require(isinstance(value, str) and value and '\\' not in value, 'input path must be a relative POSIX path')
    path = PurePosixPath(value)
    require(not path.is_absolute() and '..' not in path.parts and str(path) == value and value != '.', 'non-canonical or escaping input path')
    value.encode('utf-8')
    return path


def rooted(root, name):
    path = root
    for part in relative(name).parts:
        path /= part
        require(not path.is_symlink(), 'symlink input is not admitted')
    return path


def validate_spec(spec):
    require(isinstance(spec, dict) and set(spec) == {'schema_version', 'scope', 'inputs', 'bindings', 'unresolved'}, 'invalid specification fields')
    require(type(spec['schema_version']) is int and spec['schema_version'] == 1 and spec['scope'] == 'preparation_only', 'only preparation_only schema version 1 is supported')
    require(isinstance(spec['unresolved'], list) and all(isinstance(item, str) and item for item in spec['unresolved']), 'unresolved must retain explicit nonempty descriptions')
    require(isinstance(spec['inputs'], list) and spec['inputs'], 'inputs are required')
    inputs, paths, roles = {}, set(), set()
    for item in spec['inputs']:
        require(isinstance(item, dict) and set(item) == {'id', 'path', 'kind', 'roles'}, 'invalid input fields')
        require(isinstance(item['id'], str) and re.fullmatch(r'[a-z][a-z0-9_]*', item['id']) and item['id'] not in inputs, 'input IDs must be unique identifiers')
        relative(item['path'])
        require(item['path'] not in paths and item['kind'] in ('file', 'tree'), 'duplicate input path or invalid kind')
        require(isinstance(item['roles'], list) and item['roles'] and all(role in ALLOWED_ROLES for role in item['roles']) and len(set(item['roles'])) == len(item['roles']), 'invalid or duplicate input roles')
        require('binary' not in item['roles'] or item['kind'] == 'file', 'a binary must be one explicit file')
        inputs[item['id']] = item
        paths.add(item['path'])
        roles.update(item['roles'])
    require(REQUIRED_ROLES <= roles, 'missing required input roles: ' + ','.join(sorted(REQUIRED_ROLES - roles)))
    require(isinstance(spec['bindings'], list) and spec['bindings'], 'receipt bindings are required')

    def selector(ref):
        require(isinstance(ref, dict) and set(ref) == {'input', 'pointer'}, 'invalid JSON selector')
        require(ref['input'] in inputs and inputs[ref['input']]['kind'] == 'file' and isinstance(ref['pointer'], str), 'JSON selector must name an explicit file')
        require(ref['pointer'] == '' or ref['pointer'].startswith('/'), 'JSON pointer must be empty or absolute')

    binary_receipts = {name: {} for name, item in inputs.items() if 'binary' in item['roles']}
    for binding in spec['bindings']:
        require(isinstance(binding, dict), 'invalid binding')
        kind = binding.get('kind')
        if kind in ('sha256', 'tree_sha256'):
            require(set(binding) == {'kind', 'receipt', 'artifact'}, 'invalid hash binding')
            selector(binding['receipt'])
            require(binding['artifact'] in inputs, 'unknown binding artifact')
            require(inputs[binding['artifact']]['kind'] == ('file' if kind == 'sha256' else 'tree'), 'hash binding artifact kind mismatch')
            if binding['artifact'] in binary_receipts:
                receipt = binding['receipt']['input']
                for role in inputs[receipt]['roles']:
                    binary_receipts[binding['artifact']].setdefault(role, set()).add(receipt)
        elif kind == 'equal':
            require(set(binding) == {'kind', 'left', 'right'}, 'invalid equality binding')
            selector(binding['left'])
            selector(binding['right'])
        elif kind == 'value':
            require(set(binding) == {'kind', 'at', 'expected'}, 'invalid fixed-value binding')
            selector(binding['at'])
        else:
            raise LockError('unknown binding kind')
    for receipts in binary_receipts.values():
        build = receipts.get('build_receipt', set())
        execution = receipts.get('execution_receipt', set())
        require(build and execution and any(left != right for left in build for right in execution), 'each binary needs SHA256 relationships to distinct supplied build and execution receipts')
    return inputs


def pointer(value, location):
    if location == '':
        return value
    for token in location[1:].split('/'):
        require(re.search(r'~(?![01])', token) is None, 'invalid JSON pointer escape')
        token = token.replace('~1', '/').replace('~0', '~')
        if isinstance(value, list):
            require(re.fullmatch(r'0|[1-9][0-9]*', token) is not None and int(token) < len(value), 'missing JSON array item')
            value = value[int(token)]
        else:
            require(isinstance(value, dict) and token in value, 'missing JSON field')
            value = value[token]
    return value


def file_record(path, collect=False):
    require(stat.S_ISREG(path.lstat().st_mode), 'only regular input files are admitted')
    digest, content = hashlib.sha256(), bytearray()
    # O_NOFOLLOW also covers a last-component replacement between lstat and open.
    fd = os.open(path, os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0))
    with os.fdopen(fd, 'rb') as stream:
        before = os.fstat(stream.fileno())
        require(stat.S_ISREG(before.st_mode), 'input changed file type')
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
            if collect:
                content.extend(chunk)
                require(len(content) <= 16 * 1024 * 1024, 'JSON binding input exceeds 16 MiB')
        after = os.fstat(stream.fileno())
    identity = lambda value: (value.st_dev, value.st_ino, value.st_size, value.st_mtime_ns, value.st_ctime_ns)
    require(identity(before) == identity(after) == identity(path.lstat()), 'input changed while being read')
    return {'kind': 'file', 'bytes': after.st_size, 'mode': stat.S_IMODE(after.st_mode), 'sha256': digest.hexdigest()}, bytes(content)


def snapshot(root, spec):
    inputs = validate_spec(spec)
    require(root.is_dir() and not root.is_symlink(), 'root must be an explicit real directory')
    refs = set()
    for binding in spec['bindings']:
        for key in ('receipt', 'left', 'right', 'at'):
            if key in binding:
                refs.add(binding[key]['input'])
    records, json_inputs = {}, {}
    for name, item in inputs.items():
        path = rooted(root, item['path'])
        if item['kind'] == 'file':
            record, data = file_record(path, name in refs)
            records[name] = record
            if name in refs:
                json_inputs[name] = parse_json(data)
        else:
            require(path.is_dir(), 'tree input must be a directory')
            members = {}

            def walk(directory):
                require(stat.S_ISDIR(directory.lstat().st_mode), 'tree directory changed type')
                for child in sorted(directory.iterdir()):
                    local = child.relative_to(path).as_posix()
                    relative(local)
                    mode = child.lstat().st_mode
                    require(not stat.S_ISLNK(mode), 'symlink tree member is not admitted')
                    if stat.S_ISDIR(mode):
                        members[local] = {'kind': 'directory', 'mode': stat.S_IMODE(mode)}
                        walk(child)
                    else:
                        members[local] = file_record(child)[0]

            walk(path)
            records[name] = {'kind': 'tree', 'mode': stat.S_IMODE(path.lstat().st_mode), 'members': members}

    def value(ref):
        return pointer(json_inputs[ref['input']], ref['pointer'])

    for binding in spec['bindings']:
        if binding['kind'] == 'sha256':
            actual, expected = records[binding['artifact']]['sha256'], value(binding['receipt'])
        elif binding['kind'] == 'tree_sha256':
            actual = {name: record['sha256'] for name, record in records[binding['artifact']]['members'].items() if record['kind'] == 'file'}
            expected = value(binding['receipt'])
        elif binding['kind'] == 'equal':
            actual, expected = value(binding['left']), value(binding['right'])
        else:
            actual, expected = value(binding['at']), binding['expected']
        require(json_bytes(actual) == json_bytes(expected), 'receipt binding mismatch: ' + binding['kind'])
    return records


def prepare(root, spec, output):
    inputs = validate_spec(spec)
    resolved_output = output.resolve()
    for item in inputs.values():
        source = rooted(root, item['path']).resolve()
        require(resolved_output != source and not (item['kind'] == 'tree' and resolved_output.is_relative_to(source)), 'lock output must be outside all selected inputs')
    records = snapshot(root, spec)
    require(records == snapshot(root, spec), 'inputs changed during lock preparation')
    lock = {
        'format': FORMAT, 'scope': 'preparation_only', 'release_candidate': False,
        'execution_attestation': False,
        'root_contract': 'All specification paths are relative to the caller-supplied root. Each tree includes every file and directory; no exclusions. Symlinks and special files are rejected.',
        'provenance_contract': 'Supplied build/execution receipt values are checked against the selected bytes. Receipt authenticity, actual past execution, source/build completeness and release acceptance are not established by this lock.',
        'environment_contract': 'No process environment is collected. Only explicitly selected existing input files/JSON fields are used; unknown values remain unchanged.',
        'specification': spec, 'records': records,
    }
    data = json_bytes(lock)
    with output.open('xb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    return {'status': 'prepared', 'lock_sha256': sha(data), 'input_count': len(records), 'scope': 'preparation_only', 'release_candidate': False, 'execution_attestation': False}


def verify(root, path, expected_sha256):
    require(isinstance(expected_sha256, str) and re.fullmatch('[0-9a-f]{64}', expected_sha256), 'an externally pinned lock SHA256 is required')
    data = path.read_bytes()
    require(sha(data) == expected_sha256, 'lock SHA256 differs from the external pin')
    lock = parse_json(data)
    require(isinstance(lock, dict), 'lock must be a JSON object')
    require(lock.get('format') == FORMAT and lock.get('scope') == 'preparation_only' and lock.get('release_candidate') is False and lock.get('execution_attestation') is False, 'unsupported lock or promoted acceptance claim')
    current = snapshot(root, lock['specification'])
    require(current == lock['records'], 'selected input bytes, modes or directory membership changed')
    require(current == snapshot(root, lock['specification']), 'inputs changed during verification')
    return {'status': 'matching_preparation_inputs', 'lock_sha256': expected_sha256, 'input_count': len(current), 'checked_receipt_relationships': len(lock['specification']['bindings']), 'unresolved': lock['specification']['unresolved'], 'scope': 'preparation_only', 'release_candidate': False, 'execution_attestation': False}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='action', required=True)
    create = sub.add_parser('prepare', help='write a new lock; never overwrite an old one')
    create.add_argument('--root', type=Path, required=True)
    create.add_argument('--spec', type=Path, required=True)
    create.add_argument('--out', type=Path, required=True)
    check = sub.add_parser('verify', help='compare current inputs to an externally pinned lock')
    check.add_argument('--root', type=Path, required=True)
    check.add_argument('--lock', type=Path, required=True)
    check.add_argument('--expected-lock-sha256', required=True)
    args = parser.parse_args()
    try:
        if args.action == 'prepare':
            result = prepare(args.root, parse_json(args.spec.read_bytes()), args.out)
        else:
            result = verify(args.root, args.lock, args.expected_lock_sha256)
    except (LockError, OSError, ValueError, TypeError, KeyError) as error:
        parser.exit(2, f'evidence input lock rejected: {type(error).__name__}: {error}\n')
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
