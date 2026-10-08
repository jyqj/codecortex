#!/usr/bin/env python3
"""Bounded artifact/input identity verification; emits only aggregates/hashes."""
import json
import subprocess
from review import HERE, REPO, PRODUCT, sha, blob

def main():
    manifest = json.loads((HERE/'artifact-manifest.json').read_text())
    for path, expected in manifest['files'].items():
        raw = (HERE/path).read_bytes()
        assert sha(raw) == expected['sha256'] and len(raw) == expected['bytes'], path
    inputs = json.loads((HERE/'input-manifest.json').read_text())
    for entry, expected in inputs.items(): assert sha(blob(entry)) == expected, entry
    changed = subprocess.check_output(['git','diff','--name-only',PRODUCT],cwd=REPO).decode().splitlines()
    prefix = str(HERE.relative_to(REPO))+'/'
    assert all(p.startswith(prefix) for p in changed)
    print(json.dumps({'artifacts_verified':len(manifest['files']), 'inputs_verified':len(inputs),
                      'changes_outside_owned_directory':0,
                      'aggregate_sha256':sha((HERE/'aggregate.json').read_bytes()),
                      'manifest_sha256':sha((HERE/'artifact-manifest.json').read_bytes())}))

if __name__ == '__main__': main()
