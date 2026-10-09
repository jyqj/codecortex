#!/usr/bin/env python3
"""Reconstruct exact Git objects already published through the GitHub connector."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE = Path(__file__).resolve().parent
REPO = Path('/workspace/scratch/2eaa00d0f93a/codecortex')
ENV = dict(os.environ, GIT_OBJECT_DIRECTORY=str(HERE / 'objects'),
           GIT_ALTERNATE_OBJECT_DIRECTORIES=str(REPO / '.git/objects'))

def git(*args, data=None, extra=None):
    return subprocess.check_output(['git', '-C', str(REPO), *args], input=data,
                                   env=dict(ENV, **(extra or {})))

def import_commit(role, entries):
    meta = json.loads((HERE / (role + '-git-commit.json')).read_text())
    parents = [x['sha'] for x in meta['parents']]
    assert len(parents) == 1 and meta['verification']['signature'] is None
    idx = HERE / (role + '-git.index')
    assert not idx.exists(), 'do not overwrite an existing import index'
    env = {'GIT_INDEX_FILE': str(idx)}
    git('read-tree', parents[0], extra=env)
    for entry in entries:
        raw = Path(entry['local']).read_bytes()
        oid = git('hash-object', '-w', '--stdin', data=raw).decode().strip()
        assert oid == entry['sha'], entry['path']
        git('update-index', '--add', '--cacheinfo', entry['mode'], oid, entry['path'], extra=env)
    tree = git('write-tree', extra=env).decode().strip()
    assert tree == meta['tree']['sha'], (tree, meta['tree'])
    # GitHub's JSON converts the actual stored timestamps to UTC. Reconstruct
    # the recorded +0800 timezone, accepting it only after exact SHA matching.
    def person(value):
        stamp = int(datetime.datetime.fromisoformat(value['date'].replace('Z', '+00:00')).timestamp())
        return f"{value['name']} <{value['email']}> {stamp} +0800"
    raw = ('tree ' + tree + '\n' + ''.join('parent ' + p + '\n' for p in parents)
           + 'author ' + person(meta['author']) + '\n'
           + 'committer ' + person(meta['committer']) + '\n\n' + meta['message']).encode()
    oid = hashlib.sha1(b'commit ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()
    assert oid == meta['sha'], 'API metadata does not reproduce exact stored commit'
    assert git('hash-object', '-t', 'commit', '-w', '--stdin', data=raw).decode().strip() == oid
    (HERE / (role + '-git-commit.raw')).write_bytes(raw)
    return {'sha': oid, 'tree': tree, 'parent': parents[0], 'objects': str(HERE / 'objects')}

if __name__ == '__main__':
    role = sys.argv[1]
    entries = json.loads(Path(sys.argv[2]).read_text())
    print(json.dumps(import_commit(role, entries)))
