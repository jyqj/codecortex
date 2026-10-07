#!/usr/bin/env python3
"""Validate the immutable ten-file historical DEV corpus, not current quality.

Only Git blobs from the commit that recorded the existing locks are admitted.
No checkout, freeze, relock, retrieval, private corpus or holdout access occurs.
The current cc-eval validator checks the unchanged manifest in a fresh tree.
"""
import argparse
import hashlib
from pathlib import Path, PurePosixPath
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
ANCHOR = '277f2490fad3fa30f2812b5547bad033867c9ea5'
MANIFEST = 'crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json'
QUERY = 'crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl'
PINS = {
    MANIFEST: '232f557a444db30e4f5050b6d4bdb0e5e012c010cb8659e4853491dea6444110',
    QUERY: 'b728333d92ed930452f35c9de848df4c2df8e76dde315e40f390e80600e37c02',
    'crates/cc-db/src/index_migrate.rs': '8987b0d2d7fc46634e45cd2f04e15b97756833108c0b8bd199d4e7cba991b55d',
    'crates/cc-index/src/dirty_closure.rs': 'd26072bd26bb5baa2db25500508b66e79730f3afba796036a5253301210048f2',
    'crates/cc-index/src/indexer_phases/dirty.rs': '20821b6045e42e16ab0cc53c8bf3d21befae74929b6afb7bd97aab8882daf484',
    'crates/cc-model/src/id.rs': 'bc1431f2c004ebabff2960dac1ecdaf55e67169d90dfd141e26b84d2b7f152b0',
    'crates/cc-model/src/public_surface.rs': '24ce031b84ab2a399c940b9ed23cdd402715ce18d84cf7b33e7815d2fd9bd177',
    'crates/cc-model/src/retrieval.rs': 'bb228628e1e63edd74c5314dde157209da2279d3064198ea256f6a54bece20e0',
    'crates/cc-parsers/src/chunker.rs': 'ea81138fe6473cfca6aa9b17b176126c7d1d179a4ae1a157204f86483e4335a5',
    'crates/cc-search/src/plan.rs': 'b4d32673260d653a8137f293cdc27acca2dbf3b81c8ecd14f70c809bf72ce6f0',
    'crates/cc-search/src/preselect.rs': '11741090fd395c97ca01db3b63754f3d18de289595e24aac8a3416d26981f639',
    'crates/cc-search/src/scope.rs': '01b4e01f95d8328eaba08477f48bc30dd1f6019afbbdc883ae45429a1b8bf646',
}
OTHER_SUITES = ('p0-smoke', 'p1b-exact', 'p1c-intents', 'p1c-bm25', 'p1c-softscope')


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT, timeout=60)


def ensure_anchor():
    if subprocess.run(['git', 'cat-file', '-e', ANCHOR + '^{commit}'], cwd=ROOT,
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                      timeout=60).returncode:
        subprocess.run(['git', 'fetch', '--no-tags', 'origin', ANCHOR],
                       cwd=ROOT, check=True, timeout=120)
    if git('rev-parse', ANCHOR + '^{commit}').decode().strip() != ANCHOR:
        raise ValueError('stale historical anchor')


def destination(root, path):
    parts = PurePosixPath(path).parts
    if (path not in PINS or not parts or PurePosixPath(path).is_absolute()
            or any(p in ('.', '..') for p in path.split('/')) or '\\' in path):
        raise ValueError('path outside fixed historical allowlist')
    dest = root / path
    current = root
    for part in parts:
        current = current / part
        if current.is_symlink():
            raise ValueError('historical symlink path escape')
    if not dest.resolve().is_relative_to(root.resolve()):
        raise ValueError('historical path escape')
    return dest


def check_bytes(path, raw):
    if len(raw) > 1_000_000 or hashlib.sha256(raw).hexdigest() != PINS[path]:
        raise ValueError('historical bytes differ: ' + path)


def materialize(root):
    """Caller supplies a fresh empty temporary directory; exactly 12 files result."""
    if root.is_symlink() or not root.is_dir() or any(root.iterdir()):
        raise ValueError('fresh empty historical tree required')
    ensure_anchor()
    # Validate all objects before writing. ls-tree rejects symlinks/submodules.
    blobs = {}
    for path in PINS:
        entry = git('ls-tree', ANCHOR, '--', path).decode().split('\t')
        if len(entry) != 2 or entry[0].split()[:2] != ['100644', 'blob'] or entry[1].strip() != path:
            raise ValueError('missing or non-file historical input: ' + path)
        raw = git('show', ANCHOR + ':' + path)
        check_bytes(path, raw)
        blobs[path] = raw
    # The live manifest/query must still be the immutable recorded inputs.
    for path in (MANIFEST, QUERY):
        live = destination(ROOT, path)
        if live.is_symlink():
            raise ValueError('symlink historical control input')
        check_bytes(path, live.read_bytes())
    for path, raw in blobs.items():
        dest = destination(root, path)
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(raw)
    return root / MANIFEST


def validate(validator, manifest, include_other=True):
    argv = [str(validator.resolve()), 'validate', '--suite', str(manifest)]
    if include_other:
        for name in OTHER_SUITES:
            argv += ['--suite', str(ROOT / ('crates/cc-eval/benchmarks/manifests/' + name + '.json'))]
    subprocess.run(argv, cwd=ROOT, check=True, timeout=120)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--validator', type=Path, default=ROOT / 'target/debug/cc-eval')
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='p0-historical-dev-') as temp:
        manifest = materialize(Path(temp))
        print('Historical DEV corpus validation; source anchor=' + ANCHOR, flush=True)
        validate(args.validator, manifest)


if __name__ == '__main__':
    main()
