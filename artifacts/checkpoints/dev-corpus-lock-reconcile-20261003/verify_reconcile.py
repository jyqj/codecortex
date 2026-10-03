"""Limited DEV migration checks; run with python3, no retrieval/provider work."""
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MANIFEST = ROOT / 'crates/cc-eval/benchmarks/manifests/p0-codecortex-subset.json'
QUERIES = ROOT / 'crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl'
BIN = ROOT / 'target/debug/cc-eval'

def rows(path):
    return [json.loads(line) for line in path.read_text().splitlines()]

old = rows(HERE / 'historical-inputs/queries.jsonl')
new = rows(QUERIES)
assert len(old) == len(new) == 14
assert [r['id'] for r in old] == [r['id'] for r in new]
for before, after in zip(old, new):
    assert after['split'] == 'dev'
    b = {k: v for k, v in before.items() if k != 'annotations'}
    a = {k: v for k, v in after.items() if k != 'annotations'}
    if after['id'] == 'R09':
        b['query'] = 'PublicSurface::fingerprint'
        b['answers'][0]['alternatives'][0]['path'] = 'cc-model/src/public_surface.rs'
    assert a == b, after['id']
    digests = after['annotations']['source_digests_sha256']
    for path, digest in digests.items():
        assert hashlib.sha256((ROOT / 'crates' / path).read_bytes()).hexdigest() == digest
    aggregate = next(iter(digests.values())) if len(digests) == 1 else hashlib.sha256(
        json.dumps(digests, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    assert aggregate == after['annotations']['source_digest_sha256']
print('PASS: 14 source annotations; only authorized R09 query/answer changed')
surface = (ROOT / 'crates/cc-model/src/public_surface.rs').read_text()
dirty = (ROOT / 'crates/cc-index/src/indexer_phases/dirty.rs').read_text()
assert 'pub fn fingerprint(&self)' in surface
assert 'pub fn changed_from(&self' in surface
assert '.changed_from(old_surfaces.get(file_path))' in dirty
assert 'compute_fingerprint_for_unit' not in dirty
assert 'fn compute_fingerprint_for_unit' in (HERE / 'historical-inputs/pre-removal-dirty.rs').read_text()
print('PASS: historical obsolete R09 retained; current canonical definition/call verified')
m = json.loads(MANIFEST.read_text())
previous = json.loads((HERE / 'historical-inputs/manifest.json').read_text())
for key in previous.keys() - {'source', 'queries_digest', 'name'}:
    assert m[key] == previous[key], key
assert set(m['source']['files']) == set(previous['source']['files']) | {'cc-model/src/public_surface.rs'}
assert m['source']['root'] == previous['source']['root']
assert m['source']['commit'] == previous['source']['commit']
print('PASS: scoring, timeout, seed, bounds/config preserved; one admitted source added')
with tempfile.TemporaryDirectory(prefix='dev-lock-control-') as directory:
    temp = Path(directory)
    for file in m['source']['files']:
        dest = temp / 'source' / file
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes((ROOT / 'crates' / file).read_bytes())
    q = temp / 'queries.jsonl'
    q.write_bytes(QUERIES.read_bytes())
    m['source']['root'] = 'source'
    m['queries'] = 'queries.jsonl'
    manifest = temp / 'suite.json'
    manifest.write_text(json.dumps(m))
    def validate(expected, label):
        result = subprocess.run([str(BIN), 'validate', '--suite', str(manifest)], capture_output=True, text=True)
        assert result.returncode == expected, result.stdout + result.stderr
        if expected:
            assert 'source or query content lock drift' in result.stderr
        print(f'PASS: {label}; exit={result.returncode}')
    validate(0, 'unchanged isolated DEV inputs')
    target = temp / 'source/cc-model/src/public_surface.rs'
    original = target.read_bytes()
    target.write_bytes(original + b'\n// undeclared drift control\n')
    validate(2, 'undeclared source drift rejected')
    target.write_bytes(original)
    q.write_bytes(q.read_bytes() + b'\n')
    validate(2, 'undeclared query byte drift rejected')
print('All limited reconciliation checks passed; no retrieval-quality claim')
