import hashlib
import json
from pathlib import Path
import subprocess

REPO = Path('/workspace/scratch/a217aaae3bde/codecortex-recovered')
OUT = Path('/workspace/scratch/a217aaae3bde/p8-db-lock-observation-integration')
CONFIG = json.loads((OUT / 'composition-inputs.json').read_text())
P, PARENT, BASE = (CONFIG[key] for key in ('product', 'parent', 'base'))
EXCLUSIONS = {'scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json', '.github/workflows/ci.yml'}


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=REPO, input=data)


def canonical(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + '\n').encode()


def leaves(ref):
    result = {}
    for row in git('ls-tree', '-r', '-z', ref).split(b'\0'):
        if row:
            header, path = row.decode().split('\t', 1)
            mode, kind, oid = header.split()
            result[path] = dict(mode=mode, type=kind, sha=oid)
    return result


def product(path):
    return path in {'Cargo.toml', 'Cargo.lock'} or path.startswith('crates/')


def validation(path):
    generated = '__pycache__' in Path(path).parts and path.endswith('.pyc')
    return path.startswith(('scripts/', '.github/workflows/', 'tests/source_integrity/')) and path not in EXCLUSIONS and not generated


def blobs(oids):
    ordered = sorted(set(oids))
    stream = git('cat-file', '--batch', data=('\n'.join(ordered) + '\n').encode())
    result = {}
    cursor = 0
    for expected in ordered:
        end = stream.index(b'\n', cursor)
        oid, kind, size = stream[cursor:end].decode().split()
        length = int(size)
        raw = stream[end + 1:end + 1 + length]
        assert oid == expected and kind == 'blob' and len(raw) == length
        assert stream[end + 1 + length:end + 2 + length] == b'\n'
        assert hashlib.sha1(b'blob ' + str(length).encode() + b'\0' + raw).hexdigest() == oid
        result[oid] = (raw, hashlib.sha256(raw).hexdigest())
        cursor = end + length + 2
    assert cursor == len(stream)
    return result


current, parent, historical = leaves(P), leaves(PARENT), leaves(BASE)
expected = {row['path']: {key: row[key] for key in ('mode', 'type', 'sha')} for row in CONFIG['records']}
changes = {path for path in current.keys() | parent.keys() if current.get(path) != parent.get(path)}
assert len(changes) == 15 and changes == set(expected)
assert all(current[path] == row for path, row in expected.items())
assert not set(parent) - set(current)
header = git('cat-file', '-p', P).decode().split('\n\n', 1)[0]
assert [line[7:] for line in header.splitlines() if line.startswith('parent ')] == [PARENT]
assert git('rev-parse', P + '^{tree}').decode().strip() == CONFIG['tree']
domains = {p: r for p, r in current.items() if product(p) or validation(p)}
before_product = {p: r for p, r in historical.items() if product(p)}
assert set(before_product) <= {p for p in domains if product(p)}
task = 'docs/roadmap/code-index-v2/tasks.json'
assert current[task] == parent[task]
assert current[task]['sha'] == 'f400eb2b5a3f8c524978bee1dfa87f98efe4bce8'
oids = [r['sha'] for r in domains.values()] + [r['sha'] for r in before_product.values()]
oids += [current[task]['sha']] + [current[p]['sha'] for p in changes]
contents = blobs(oids)
for path, row in domains.items():
    assert row['mode'] in {'100644', '100755'} and row['type'] == 'blob'
for row in CONFIG['records']:
    raw, digest = contents[row['sha']]
    assert len(raw) == row['bytes'] and digest == row['sha256']
    assert (parent.get(row['path']) or {}).get('sha') == row['before_git_blob']
complete = {p: contents[r['sha']][1] for p, r in sorted(domains.items()) if product(p)}
validated = {p: contents[r['sha']][1] for p, r in sorted(domains.items()) if validation(p)}
delta = {}
for path, digest in complete.items():
    before = before_product.get(path)
    if before != current[path]:
        delta[path] = dict(before_sha256=None if before is None else contents[before['sha']][1], sha256=digest)
assert set(delta) == set(git('diff', '--name-only', BASE, P, '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())
assert len(complete) == 1094 and len(validated) == 142
assert contents[current[task]['sha']][1] == '6e2ab2e90cbd820228e8ff397d70c770561ba20d3110e6eca31338f29a945207'
full_leaves = {p: dict(r, bytes=len(contents[r['sha']][0]), sha256=contents[r['sha']][1]) for p, r in sorted(domains.items())}
proof = dict(
    schema_version=1, status='passed',
    scope='actual_git_product_composition_and_complete_blob_digest_verification',
    product=P, product_tree=CONFIG['tree'], sole_parent=PARENT, historical_base=BASE,
    changed_paths=sorted(changes), composition=CONFIG['records'],
    all_other_parent_leaves_and_modes_unchanged=True,
    parent_leaf_count=len(parent), product_leaf_count=len(current),
    complete_inputs=complete, validation_inputs=validated, delta=delta,
    complete_inputs_manifest_sha256=hashlib.sha256(canonical(complete)).hexdigest(),
    validation_inputs_manifest_sha256=hashlib.sha256(canonical(validated)).hexdigest(),
    full_leaf_manifest_sha256=hashlib.sha256(canonical(full_leaves)).hexdigest(),
    unique_actual_blob_count=len(contents), actual_blob_bytes=sum(len(v[0]) for v in contents.values()),
    tasks=dict(path=task, blob=current[task]['sha'], sha256=contents[current[task]['sha']][1],
               byte_identical_to_parent=True, total=192, done=163, remaining=29, newly_completed=0),
    actual_execution=dict(rust_format='not_run', rust_compile='not_run', rust_controls='not_run',
                          diagnostic_native='not_run', original_v15='pending_final_review_and_pins'),
    scale_scope='Original C3ff study remains fixed; this product identity is not substituted into any original measurement.',
)
for name, value in [('root-actual-product-proof.json', proof), ('product-domain.json', complete),
                    ('validation-domain.json', validated), ('full-leaf-manifest.json', full_leaves),
                    ('historical-delta.json', delta)]:
    (OUT / name).write_bytes(canonical(value))
raw = canonical(proof)
print(json.dumps(dict(status='passed', product=P, tree=CONFIG['tree'], changed_paths=len(changes),
                      product_inputs=len(complete), historical_delta=len(delta), validation_inputs=len(validated),
                      proof_bytes=len(raw), proof_sha256=hashlib.sha256(raw).hexdigest(),
                      product_manifest_sha256=proof['complete_inputs_manifest_sha256'],
                      validation_manifest_sha256=proof['validation_inputs_manifest_sha256'])))
