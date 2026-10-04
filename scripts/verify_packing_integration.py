#!/usr/bin/env python3
"""Verify pinned packing source, complete crate inputs, imported evidence, open gates."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REPORT = ROOT / 'docs/checkpoints/2026-10-03-packing-integration'
SOURCE = '90858afae647a513537bf118932a7ba5020ee98b'


def blob(ref, path):
    return subprocess.check_output(['git', 'show', f'{ref}:{path}'], cwd=ROOT)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    manifest = json.loads((REPORT / 'source-manifest.json').read_text())
    assert manifest['fixed_product_source'] == SOURCE
    # Actions checkout is shallow. Fetch only named provenance objects when
    # absent, through normal origin; never replace the worktree or pin values.
    refs = {r['source_commit'] for r in manifest['crate_inputs'] + manifest['evidence_files']}
    refs.add('156e3ac13ddc6aa2b0208c8805c8c79bd2a0a38d')
    missing = [r for r in sorted(refs) if subprocess.run(
        ['git', 'cat-file', '-e', r + '^{commit}'], cwd=ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode]
    if missing:
        subprocess.run(['git', 'fetch', '--no-tags', 'origin', *missing], cwd=ROOT, check=True)
    expected = {r['path']: r for r in manifest['crate_inputs']}
    actual = set(subprocess.check_output(['git', 'ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock'], cwd=ROOT, text=True).splitlines())
    assert actual == set(expected), 'crate input inventory changed'
    disk = {p.relative_to(ROOT).as_posix() for p in (ROOT / 'crates').rglob('*') if p.is_file() and '__pycache__' not in p.parts}
    assert disk == {p for p in expected if p.startswith('crates/')}, 'untracked crate inputs'
    for path, row in expected.items():
        raw = (ROOT / path).read_bytes()
        assert sha(raw) == row['sha256'], f'current input changed: {path}'
        original = blob(row['source_commit'], path)
        if row.get('adaptation') == 'four_cloned_ref_to_slice_refs':
            assert sha(original) == row['original_sha256']
            assert (REPORT / 'original-tests/qname_db_independent_review.rs').read_bytes() == original
            head, tail = original.split(b'&[original.clone()]', 1)
            original = head + b'&[original.clone()]' + tail.replace(
                b'&[original.clone()]', b'std::slice::from_ref(&original)').replace(
                b'&[repeated.clone()]', b'std::slice::from_ref(&repeated)')
        assert raw == original, f'input provenance changed: {path}'
        if row['category'] == 'product':
            assert row['source_commit'] == SOURCE
    source_paths = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', SOURCE, '--', 'crates', 'Cargo.toml', 'Cargo.lock'], cwd=ROOT, text=True).splitlines()
    def is_product(p):
        return p in ('Cargo.toml', 'Cargo.lock') or '/src/' in p or (p.startswith('crates/') and p.endswith(('/Cargo.toml', '/build.rs')))
    assert {p for p in source_paths if is_product(p)} == {p for p, r in expected.items() if r['category'] == 'product'}, 'production inventory changed'
    for row in manifest['evidence_files']:
        raw = (ROOT / row['path']).read_bytes()
        assert sha(raw) == row['sha256'], f'evidence fixity: {row["path"]}'
        assert raw == blob(row['source_commit'], row['path'])
    for row in manifest['unchanged_workflows']:
        assert (ROOT / row['path']).read_bytes() == blob('156e3ac13ddc6aa2b0208c8805c8c79bd2a0a38d', row['path'])
    tasks = json.loads((ROOT / 'docs/roadmap/code-index-v2/tasks.json').read_text())
    assert {t['id']: t['status'] for t in tasks['tasks']} == manifest['preserved_task_states']
    gates = json.loads((ROOT / 'docs/roadmap/code-index-v2/P7-REMAINING-GATES.json').read_text())
    assert gates['full_gate_status']['V19'] == 'open'
    print(json.dumps(dict(status='passed', fixed_product_source=SOURCE,
                         crate_inputs=len(expected), evidence_files=len(manifest['evidence_files']),
                         packing_independent_review='root_accepted_bounded_scoped_search_c0876c8', V19='open', P7='open')))


if __name__ == '__main__':
    main()
