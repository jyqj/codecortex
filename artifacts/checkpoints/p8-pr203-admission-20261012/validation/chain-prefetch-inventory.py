import ast
import collections
import datetime
import json
import os
import pathlib
import subprocess

repo = pathlib.Path('/workspace/scratch/2509e1756d27/codecortex')
out = repo.parent / 'pr-audit-20261012'
env = {**os.environ, 'GIT_NO_LAZY_FETCH': '1', 'GIT_OPTIONAL_LOCKS': '0', 'GIT_TERMINAL_PROMPT': '0'}
pairs = collections.defaultdict(set)
fullrefs = collections.defaultdict(list)
diagnostics = []

def readj(p):
    return json.loads((repo / p).read_bytes())

def add(r, p, why):
    assert len(r) == 40 and all(c in '0123456789abcdef' for c in r)
    assert p.startswith(('crates/', 'Cargo.', 'scripts/', '.github/', 'artifacts/', 'docs/'))
    assert '..' not in pathlib.PurePosixPath(p).parts and not pathlib.PurePosixPath(p).is_absolute()
    pairs[(r, p)].add(why)

def literal(node, names):
    if isinstance(node, ast.Name):
        return names[node.id]
    if isinstance(node, ast.Constant):
        return node.value
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
        return literal(node.left, names) + literal(node.right, names)
    if isinstance(node, (ast.List, ast.Tuple, ast.Set)):
        cls = {ast.List: list, ast.Tuple: tuple, ast.Set: set}[type(node)]
        return cls(literal(x, names) for x in node.elts)
    if isinstance(node, ast.Dict):
        return {literal(k, names): literal(v, names) for k, v in zip(node.keys, node.values)}
    raise ValueError(type(node).__name__)

def constants(path):
    d = {}
    for n in ast.parse((repo / path).read_text()).body:
        if not isinstance(n, ast.Assign) or len(n.targets) != 1:
            continue
        target = n.targets[0]
        try:
            v = literal(n.value, d)
            if isinstance(target, ast.Name):
                d[target.id] = v
            elif isinstance(target, ast.Subscript) and isinstance(target.value, ast.Name):
                d[target.value.id][literal(target.slice, d)] = v
        except (ValueError, TypeError, KeyError):
            continue
    return d

def git(*args, input=None):
    return subprocess.run(['git', *args], cwd=repo, input=input, capture_output=True, env=env, timeout=30)

v1 = constants('scripts/verify_current_source.py')
packing = readj('docs/checkpoints/2026-10-03-packing-integration/source-manifest.json')
bounded = readj(v1['B'] + 'source-guard.json')
python = readj(v1['P'] + 'source-guard.json')
migration = readj(v1['P'] + 'migration.json')
for row in packing['crate_inputs']:
    for r in (row['source_commit'], v1['PACKING']):
        add(r, row['path'], 'v1 packing crate_inputs')
for p, row in bounded['source_files'].items():
    for r in (row['source'], v1['BOUNDED']):
        add(r, p, 'v1 bounded source_files')
for p, r in bounded['review_imports'].items():
    for ref in (v1['REVIEW_REFS'].get(r, r), v1['BOUNDED']):
        add(ref, p, 'v1 bounded review_imports exact declared path')
add(v1['REVIEW_REFS']['bc0d0a25'], 'crates/cc-index/tests/provenance_review_support/prototype_compatibility.rs', 'v1 compatibility relocation')
for entry in python['imports']:
    for p in entry['paths']:
        for r in (entry['source'], entry['integrated']):
            add(r, p, 'v1 Python declared import')
for k in ('historical_source_commit', 'historical_integrated_commit'):
    add(migration[k], migration['path'], 'v1 exact R1 migration')
for k in ('PACKING', 'BOUNDED', 'BASE', 'PRODUCT'):
    fullrefs[v1[k]].append('verify_current_source.py complete input loop ' + k)
for version in (1, 2, 3):
    reg = readj('scripts/current-source-registry-v' + str(version) + '.json')
    for p, row in reg['records'].items():
        add(row['commit'], p, 'v' + str(version) + ' exact pinned registry record')
v2 = constants('scripts/verify_current_source_v2.py')
fullrefs[v2['CAPTURE']].append('v2 complete CAPTURE input loop')
fullrefs[v2['PRODUCT']].append('v2 complete PRODUCT input loop')
for p in v2['CAPTURE_PATHS']:
    add(v2['ORIGINAL'], p, 'v2 ORIGINAL exact CAPTURE_PATHS')
v3 = constants('scripts/verify_current_source_v3.py')
for k in ('BASE', 'SOURCE', 'DELIVERY', 'REVIEW'):
    fullrefs[v3[k]].append('v3 complete input loop ' + k)
files = ['scripts/verify_reviewed_source.py', 'scripts/verify_reviewed_source_p7_v9.py', 'scripts/verify_reviewed_source_joint22_v10.py']
files += [f'scripts/verify_reviewed_source_v{i}.py' for i in range(10, 16)]
for f in files:
    d = constants(f)
    for k in ('BASE', 'PRODUCT', 'JOINT_BASE'):
        if k in d:
            fullrefs[d[k]].append(f + ' complete input loop ' + k)
    if 'REVIEW' in d and 'REVIEW_PATH' in d:
        add(d['REVIEW'], d['REVIEW_PATH'], f + ' exact independent review')
    for name, pin in d.get('APPROVED', {}).items():
        add(pin['review'], pin['review_path'], f + ' APPROVED ' + name + ' review')
        for p in pin['paths']:
            add(pin['source'], p, f + ' APPROVED ' + name + ' source path')
            if 'base' in pin:
                add(pin['base'], p, f + ' APPROVED ' + name + ' optional existing base path')
    for p in d.get('HISTORICAL_HELPERS', []):
        add(d['BASE'], p, f + ' exact helper byte pin')

# These are exactly the domains of the original source guard complete input loops.
# The script reads tree metadata and cache presence only, never blob contents.
for ref, why in fullrefs.items():
    p = git('ls-tree', '-r', '-z', ref, '--', 'crates', 'Cargo.toml', 'Cargo.lock')
    if p.returncode:
        diagnostics.append({'ref': ref, 'operation': 'full input tree', 'reason': why, 'error': p.stderr.decode()})
        continue
    for item in p.stdout.split(b'\0'):
        if item:
            _, name = item.split(b'\t', 1)
            for w in why:
                add(ref, name.decode(), w)
groups = collections.defaultdict(set)
for ref, p in pairs:
    groups[ref].add(p)
resolved = []
optional_absent = []
for ref, paths in groups.items():
    p = git('ls-tree', '-r', '-z', ref, '--', *sorted(paths))
    if p.returncode:
        diagnostics.append({'ref': ref, 'operation': 'declared path tree', 'paths': sorted(paths), 'error': p.stderr.decode()})
        continue
    tree = {}
    for item in p.stdout.split(b'\0'):
        if not item:
            continue
        hdr, name = item.split(b'\t', 1)
        mode, typ, oid = hdr.decode().split()
        tree[name.decode()] = (mode, typ, oid)
    for path in sorted(paths):
        if path not in tree:
            optional = all('optional existing base path' in w for w in pairs[(ref, path)])
            target = optional_absent if optional else diagnostics
            target.append({'ref': ref, 'path': path, 'reason': sorted(pairs[(ref, path)]), 'error': 'absent path; guard checks existence first' if optional else 'absent required path'})
            continue
        mode, typ, oid = tree[path]
        resolved.append({'ref': ref, 'path': path, 'mode': mode, 'type': typ, 'oid': oid, 'reason': sorted(pairs[(ref, path)])})
oids = sorted({r['oid'] for r in resolved})
p = git('cat-file', '--batch-check', input=('\n'.join(oids) + '\n').encode())
assert p.returncode == 0, p.stderr
missing = [line.split()[0] for line in p.stdout.decode().splitlines() if line.endswith(' missing')]
r = {
    'timestamp_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'scope': 'Only original source guard complete crates/Cargo input loops and individually declared import/review/record paths; no corpus body discovery, no standalone benchmark replay, no guard execution',
    'full_input_refs': dict(fullrefs), 'unique_refs': len(groups), 'resolved_path_bindings': len(resolved),
    'unique_oids': len(oids), 'missing_count': len(missing), 'missing_oids': missing,
    'unavailable': diagnostics, 'optional_base_paths_absent': optional_absent,
    'resolved': resolved, 'prefetch_performed': False, 'guard_execution_performed': False,
}
(out / 'round4-chain-missing-objects.json').write_text(json.dumps(r, indent=2) + '\n')
(out / 'round4-chain-missing-oids.txt').write_text('\n'.join(missing) + '\n' if missing else '')
print(json.dumps({k: r[k] for k in ['unique_refs', 'resolved_path_bindings', 'unique_oids', 'missing_count', 'unavailable', 'optional_base_paths_absent']}, indent=2))
