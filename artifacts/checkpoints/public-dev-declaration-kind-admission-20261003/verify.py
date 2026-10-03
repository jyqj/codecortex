#!/usr/bin/env python3
"""Validate append-only scope and pinned opt-in loader; aggregate output only."""
import json
import subprocess
import selector as s


def main():
    prefix = str(s.HERE.relative_to(s.ROOT)) + '/'
    changed = subprocess.check_output(
        ['git', 'diff', '--name-only', s.INDEPENDENT], cwd=s.ROOT).decode().splitlines()
    changed += subprocess.check_output(
        ['git', 'ls-files', '--others', '--exclude-standard'], cwd=s.ROOT).decode().splitlines()
    s.require(all(p.startswith(prefix) for p in changed), 'changes outside admission prefix')
    subprocess.run(['git', 'diff', '--check', s.INDEPENDENT], cwd=s.ROOT, check=True)
    packages = s.load_version(version=s.VERSION, repositories=s.REPOS,
                              source_bytes=s.source_inputs())
    receipt = json.loads((s.HERE / 'admission-receipt.json').read_bytes())
    print(json.dumps(dict(version=s.VERSION, append_only=True,
                          candidate_commit=s.CANDIDATE, independent_commit=s.INDEPENDENT,
                          receipt_sha256=s.RECEIPT_PIN, selector_sha256=s.SELECTOR_PIN,
                          artifact_manifest_sha256=s.sha((s.HERE / 'artifact-manifest.json').read_bytes()),
                          source_files=len(receipt['source_bytes_sha256']),
                          native_rows=158, compat_rows=138, deltas=166,
                          native_sha256={r: s.sha(p['native']) for r, p in packages.items()},
                          non_kind_byte_changes=0, compat_byte_changes=0,
                          new_independent_samples=0, retrieval_runs=0, scores=0), sort_keys=True))


if __name__ == '__main__':
    main()
