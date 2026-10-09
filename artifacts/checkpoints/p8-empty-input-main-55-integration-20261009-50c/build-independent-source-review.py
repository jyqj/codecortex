#!/usr/bin/env python3
"""Seal /root/pr_audit's immutable P4 source and validation input review.

This audit reader imports no repository modules and runs no project validator
or product test. All Git reads forbid lazy fetching. Only these scratch audit
outputs are written. Old product executions retain their original identities.
"""
from pathlib import Path
from collections import Counter
import ast
import datetime
import hashlib
import json
import os
import re
import subprocess
import tomllib

D = Path(__file__).resolve().parent
REPO = Path(os.environ.get('CODECORTEX_REVIEW_REPO', '/workspace/scratch/50c364fd60b1/codecortex'))
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0')
P = '31a42daeb12da6936695abb08eb3912d4f3c6064'
P_TREE = '286cca0c09c796227dfadc030af298b4f1c4e575'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
MAIN = '55aa2bcf355441585bcf980e1d6f4fab8eebe59d'
FD9 = 'fd9ca5db6485b575872c722995c451db926d05c9'
B4 = '27e60348f01d16298dd4d28f48dd1e2d47ee687c'
P2 = 'b4fef72211e5967f4fba729d25d0ca2958094fd5'
P2_R = '98fe910f22eb92f8d9f9c8a8043492ba45dc997c'
P3 = '034982202bdccc90b4938c6a6d6a56a36d4678af'
P3_R = '299d2eaca3bb525f309929b74342a65a7cdae9c0'
UPSTREAM_P = '49a03e1f9fa33b7b85cbc9680b47f521afbd8abd'
UPSTREAM_R = 'bd9f7979ee76bf1ca3f0d8be284fd1a8f39fea22'
UPSTREAM_LOCAL_P = 'b069c73a0d76736e1dfb3eb64e79cad6980b773f'
PREFIX = 'artifacts/checkpoints/p8-empty-input-main-55-integration-20261009-50c/'
UPSTREAM_PREFIX = 'artifacts/checkpoints/p8-install-compat-36fea-20261009/'
UPSTREAM_REVIEW_PATH = UPSTREAM_PREFIX + 'published-independent-source-review.json'
UPSTREAM_LOCAL_REVIEW_PATH = UPSTREAM_PREFIX + 'independent-source-review.json'
P2_REVIEW_PATH = 'artifacts/checkpoints/p8-empty-input-preparation-20261009-50c/independent-source-review.json'
P3_REVIEW_PATH = 'artifacts/checkpoints/p8-empty-input-main-f7-integration-20261009-50c/independent-source-review.json'
OURS = (
    'crates/cc-index/src/dispatch_synthesis/interface_dispatch.rs',
    'crates/cc-index/src/dispatch_synthesis/interface_dispatch_tests.rs',
    'crates/cc-index/src/indexer_phases/snapshot.rs',
    'crates/cc-index/tests/full_snapshot_empty_config.rs',
)
NEW_MAIN = (
    'Cargo.lock',
    'crates/cc-search/src/engine.rs',
    'crates/cc-server/Cargo.toml',
    'crates/cc-server/src/cli.rs',
    'crates/cc-server/src/installer/helpers.rs',
    'crates/cc-server/src/installer/targets/codex_cli.rs',
    'crates/cc-server/tests/installer_cli.rs',
)
DOCS = (
    'README.md', 'docs/roadmap/code-index-v2/tasks.json',
    'docs/roadmap/code-index-v2/05-TODO.md',
    'docs/roadmap/code-index-v2/08-HANDOFF.md',
    'docs/roadmap/code-index-v2/README.md',
    'docs/roadmap/code-index-v2/PLAN-CHECK.json',
)
SUPPORTING = (
    'crates/cc-index/src/indexer_phases/config_link.rs',
    'crates/cc-index/src/indexer_phases/resolve.rs',
    'crates/cc-index/src/indexer_phases/postprocess.rs',
    'crates/cc-index/src/resolver/resolve_core.rs',
    'crates/cc-index/src/resolver/types.rs',
    'crates/cc-index/src/config_linker.rs',
    'crates/cc-index/src/indexer_phases/write.rs',
    'crates/cc-index/src/build_plan.rs',
    'crates/cc-db/src/snapshot_write_txn.rs',
    'crates/cc-index/src/project_model/mod.rs',
    'crates/cc-db/src/sql/index_v1.sql',
    'crates/cc-db/src/index_migrate.rs',
    'crates/cc-db/src/index_db_graph.rs',
    'crates/cc-db/src/index_db_edges.rs',
    'crates/cc-index/src/dispatch_synthesis/mod.rs',
    'crates/cc-index/src/synthesis_pipeline.rs',
    'crates/cc-server/src/installer/mod.rs',
    'crates/cc-server/src/main.rs',
    'crates/cc-search/src/engine_lane_tests.rs',
    'crates/cc-search/src/lib.rs',
)
COMMANDS = []
VERIFIED_CACHE_READS = []


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def git_blob(raw):
    return hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), ensure_ascii=False).encode()


def git(*args, input=None):
    completed = subprocess.run(['git', *args], cwd=REPO, env=ENV, input=input,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    COMMANDS.append({'argv': ['git', *args], 'exit_code': completed.returncode})
    if completed.returncode:
        raise RuntimeError(completed.stderr.decode(errors='replace'))
    return completed.stdout


def tree(ref):
    result = {}
    for row in git('ls-tree', '-r', '-z', ref).split(b'\0'):
        if not row:
            continue
        metadata, path = row.split(b'\t', 1)
        mode, kind, oid = metadata.decode().split()
        result[path.decode()] = {'mode': mode, 'type': kind, 'blob': oid}
    return result


def product(rows):
    return {p: r for p, r in rows.items() if p.startswith('crates/') or p in ('Cargo.toml', 'Cargo.lock')}


def changed_paths(before, after):
    return sorted(p for p in set(before) | set(after) if before.get(p) != after.get(p))


commit_before = git('show', '-s', '--format=%H%n%T%n%P', P).decode().splitlines()
assert commit_before == [P, P_TREE, B4 + ' ' + MAIN]
refs = {'P4': P, 'BASE': BASE, 'main55': MAIN, 'fd9': FD9, 'B4': B4,
        'P2': P2, 'P3': P3, 'upstream_product': UPSTREAM_P, 'upstream_review': UPSTREAM_R}
trees = {k: tree(v) for k, v in refs.items()}
guard_raw = git('show', P + ':scripts/verify_reviewed_source_v15.py')
constants = {}
for node in ast.parse(guard_raw).body:
    if not isinstance(node, ast.Assign) or len(node.targets) != 1 or not isinstance(node.targets[0], ast.Name):
        continue
    name = node.targets[0].id
    if name in ('BASE', 'VERSION', 'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256', 'FROZEN', 'VALIDATION_ROOTS'):
        constants[name] = ast.literal_eval(node.value)
    if name == 'VALIDATION_EXCLUSIONS':
        assert isinstance(node.value, ast.Call) and isinstance(node.value.func, ast.Name) and node.value.func.id == 'frozenset'
        constants[name] = frozenset(ast.literal_eval(node.value.args[0]))
assert constants['BASE'] == BASE
assert constants['VERSION'] == 'p8-completion-source-20261009-v15'
assert constants['PRODUCT'] == UPSTREAM_P and constants['REVIEW'] == UPSTREAM_R
assert constants['REVIEW_PATH'] == UPSTREAM_REVIEW_PATH
assert constants['REGISTRY_SHA256'] == '9e34425eecba1928424e415e8397e5c5b8bfc42be504c3a1cb6a80d4283e95ce'
assert set(constants['VALIDATION_ROOTS']) == {'scripts', '.github/workflows', 'tests/source_integrity'}
assert constants['VALIDATION_EXCLUSIONS'] == frozenset({
    'scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json', '.github/workflows/ci.yml'})


def validation(rows):
    return {p: r for p, r in rows.items()
            if p.startswith(tuple(x + '/' for x in constants['VALIDATION_ROOTS']))
            and p not in constants['VALIDATION_EXCLUSIONS']
            and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}


products = {k: product(v) for k, v in trees.items()}
validations = {k: validation(v) for k, v in trees.items()}
base, current, vals = products['BASE'], products['P4'], validations['P4']
assert set(base) <= set(current)
for rows in [*products.values(), *validations.values()]:
    assert all(r['type'] == 'blob' and r['mode'] in ('100644', '100755') for r in rows.values())
changed = changed_paths(base, current)
assert changed == sorted(git('diff', '--name-only', BASE, P, '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())
assert changed_paths(products['fd9'], products['main55']) == sorted(NEW_MAIN)
assert changed_paths(products['main55'], current) == sorted(OURS)
assert all(current[p] == products['P2'][p] == products['P3'][p] == products['B4'][p] for p in OURS)
assert all(current[p] == products['main55'][p] for p in current if p not in OURS)
assert products['main55'] == products['upstream_product']
assert validations['P4'] == validations['main55'] == validations['upstream_product'] == validations['P3']

needed = {r['blob'] for label in ('P4', 'BASE', 'main55', 'fd9', 'P2', 'P3')
          for rows in (products[label], validations[label]) for r in rows.values()}
extra_paths = {
    UPSTREAM_REVIEW_PATH, P2_REVIEW_PATH, P3_REVIEW_PATH,
    'scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json',
    '.github/workflows/ci.yml', 'docs/roadmap/code-index-v2/tasks.json',
    *(UPSTREAM_PREFIX + 'validation/review/' + name for name in (
        'installer-independent-review.json', 'engine-independent-review.json', 'cleanup-owner-inventory.json')),
}
needed.update(trees['P4'][p]['blob'] for p in extra_paths)
needed.add(trees['P3']['scripts/verify_reviewed_source_v15.py']['blob'])
ordered = sorted(needed)
batch = git('cat-file', '--batch', input=''.join(oid + '\n' for oid in ordered).encode())
bodies = {}
offset = 0
for expected in ordered:
    end = batch.index(b'\n', offset)
    header = batch[offset:end].decode().split()
    assert len(header) == 3 and header[0] == expected and header[1] == 'blob', header
    size = int(header[2])
    raw = batch[end + 1:end + 1 + size]
    assert len(raw) == size and batch[end + 1 + size:end + 2 + size] == b'\n'
    assert git_blob(raw) == expected
    bodies[expected] = raw
    offset = end + 2 + size
assert offset == len(batch)


def digest_map(rows):
    return {p: sha(bodies[r['blob']]) for p, r in sorted(rows.items())}


def fixed_raw(path, label='P4'):
    return bodies[trees[label][path]['blob']]


def extra_historical_raw(path):
    """An earlier API blob cache may supply history, never source input maps."""
    oid = trees['P4'][path]['blob']
    if oid in bodies:
        return bodies[oid]
    cached = D / 'main55-inputs' / path
    if cached.is_file():
        raw = cached.read_bytes()
        assert git_blob(raw) == oid
        VERIFIED_CACHE_READS.append({'path': path, 'blob': oid, 'sha256': sha(raw),
                                     'bytes': len(raw),
                                     'method': 'Earlier official GitHub blob response, verified against the fixed P4 Git tree; original bytes, no reconstruction'})
        return raw
    raw = git('cat-file', 'blob', oid)
    assert git_blob(raw) == oid
    VERIFIED_CACHE_READS.append({'path': path, 'blob': oid, 'sha256': sha(raw),
                                 'bytes': len(raw), 'method': 'Existing fixed local Git blob'})
    return raw


complete = digest_map(current)
validation_inputs = digest_map(vals)
delta = {p: {'before_sha256': sha(bodies[base[p]['blob']]) if p in base else None,
             'sha256': complete[p]} for p in changed}
rebuilt = {p: bodies[r['blob']] for p, r in base.items()}
rebuilt.update({p: bodies[current[p]['blob']] for p in changed})
assert set(rebuilt) == set(current)
assert all(raw == bodies[current[p]['blob']] for p, raw in rebuilt.items())
assert [len(complete), len(validation_inputs), len(delta)] == [1092, 139, 53]

upstream_raw = fixed_raw(UPSTREAM_REVIEW_PATH)
upstream = json.loads(upstream_raw)
p2_raw, p3_raw = fixed_raw(P2_REVIEW_PATH), fixed_raw(P3_REVIEW_PATH)
p2_review, p3_review = json.loads(p2_raw), json.loads(p3_raw)
registry_raw = fixed_raw('scripts/reviewed-source-registry-v15.json')
registry = json.loads(registry_raw)
assert sha(upstream_raw) == '1c29eb3e9f2509de538cb0b737ded099a7ee771c44b2afdde24d868525ce9eca'
assert sha(p3_raw) == '22ff761a39e5b6d40cadb242a3807dd2f85eb2513a121a279f62ce20940a49c0'
assert trees['P4'][UPSTREAM_REVIEW_PATH] == trees['main55'][UPSTREAM_REVIEW_PATH] == trees['upstream_review'][UPSTREAM_REVIEW_PATH]
assert git('rev-parse', P2_R + ':' + P2_REVIEW_PATH).decode().strip() == trees['P4'][P2_REVIEW_PATH]['blob']
assert git('rev-parse', P3_R + ':' + P3_REVIEW_PATH).decode().strip() == trees['P4'][P3_REVIEW_PATH]['blob']
for doc, source in ((upstream, UPSTREAM_P), (p2_review, P2), (p3_review, P3)):
    assert doc['source'] == source and doc['base'] == BASE and doc['verdict'] == 'accepted_scoped'
assert upstream['complete_inputs'] == registry['complete_inputs'] == digest_map(products['main55'])
assert upstream['validation_inputs'] == registry['validation_inputs'] == validation_inputs
assert upstream['paths'] == registry['delta'] == {p: delta[p] for p in upstream['paths']}
assert sha(registry_raw) == constants['REGISTRY_SHA256']
assert set(changed) == set(upstream['paths']) | set(OURS)
assert not (set(upstream['paths']) & set(OURS))
assert len(upstream['paths']) == 49 and len(upstream['complete_inputs']) == 1090
assert {p: complete[p] for p in OURS} == {p: p2_review['complete_inputs'][p] for p in OURS} == {p: p3_review['complete_inputs'][p] for p in OURS}
assert {p: delta[p] for p in OURS} == {p: p2_review['paths'][p] for p in OURS} == {p: p3_review['paths'][p] for p in OURS}
assert p3_review['complete_inputs'] == digest_map(products['P3'])
assert p3_review['validation_inputs'] == validation_inputs

artifacts = {k: {p: r for p, r in v.items() if p.startswith('artifacts/')} for k, v in trees.items()}
common_artifacts = set(artifacts['B4']) & set(artifacts['main55'])
assert all(artifacts['B4'][p] == artifacts['main55'][p] for p in common_artifacts)
b4_only = sorted(set(artifacts['B4']) - set(artifacts['main55']))
assert len(b4_only) == 200
assert artifacts['P4'] == dict(artifacts['main55'], **artifacts['B4'])
assert len(artifacts['P4']) == 22076
overlay = changed_paths(trees['main55'], trees['P4'])
assert overlay == sorted([*b4_only, *OURS, *DOCS]) and len(overlay) == 210
expected_tree = dict(trees['main55'])
expected_tree.update({p: trees['B4'][p] for p in [*b4_only, *OURS]})
expected_tree.update({p: trees['P4'][p] for p in DOCS})
assert expected_tree == trees['P4']
assert all(trees['P4'][p] == trees['BASE'][p] for p in constants['FROZEN'])
assert len(constants['FROZEN']) == 6
guard_paths = ('scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json')
assert all(trees['P4'][p] == trees['main55'][p] for p in guard_paths)
assert all(trees['P4'][p] == trees['main55'][p] == trees['P3'][p] for p in (
    '.github/workflows/ci.yml', 'scripts/verify_current_source.py',
    'scripts/v15_historical_context.py', 'tests/source_integrity/v15_historical_test_adapter.py'))


def strip_four_bindings(raw):
    return re.sub(rb'^(PRODUCT|REVIEW|REVIEW_PATH|REGISTRY_SHA256) = .*$',
                  rb'\1 = <binding>', raw, flags=re.M)


assert strip_four_bindings(guard_raw) == strip_four_bindings(fixed_raw('scripts/verify_reviewed_source_v15.py', 'P3'))
dependencies = {}
for path in SUPPORTING:
    assert current[path] == products['main55'][path] == products['P3'][path]
    dependencies[path] = dict(current[path], sha256=complete[path], bytes=len(bodies[current[path]['blob']]))

old_lock = tomllib.loads(fixed_raw('Cargo.lock', 'fd9').decode())
new_lock = tomllib.loads(fixed_raw('Cargo.lock').decode())
old_server = next(p for p in old_lock['package'] if p['name'] == 'cc-server')
new_server = next(p for p in new_lock['package'] if p['name'] == 'cc-server')
assert 'toml_edit' not in old_server['dependencies'] and 'toml_edit' in new_server['dependencies']
rebuilt_lock = json.loads(json.dumps(new_lock))
next(p for p in rebuilt_lock['package'] if p['name'] == 'cc-server')['dependencies'].remove('toml_edit')
assert rebuilt_lock == old_lock
toml_package = next(p for p in new_lock['package'] if p['name'] == 'toml' and p['version'] == '0.8.23')
assert 'toml_edit' in toml_package['dependencies']
toml_edit_package = next(p for p in new_lock['package'] if p['name'] == 'toml_edit' and p['version'] == '0.22.27')
index_manifest = tomllib.loads(fixed_raw('crates/cc-index/Cargo.toml').decode())
server_manifest = tomllib.loads(fixed_raw('crates/cc-server/Cargo.toml').decode())
assert index_manifest['dependencies']['toml'] == '=0.8.23'
assert server_manifest['dependencies']['toml_edit'] == '0.22.27'
assert not [p for p, row in current.items() if b'append_if_missing' in bodies[row['blob']]]

engine_raw = (D / 'engine-static-review.json').read_bytes()
engine_review = json.loads(engine_raw)
assert engine_review['comparison'] == [FD9, MAIN]
assert engine_review['production_bytes_equal'] and engine_review['production_bytes'] == 18720
assert engine_review['removed_duplicate_tests'] == 18 and engine_review['removed_duplicate_helpers'] == 1
assert engine_review['tests_before'] == 35 and engine_review['tests_after'] == 17
engine_prefix = fixed_raw('crates/cc-search/src/engine.rs').split(b'#[cfg(test)]\nmod tests', 1)[0]
assert engine_prefix == fixed_raw('crates/cc-search/src/engine.rs', 'fd9').split(b'#[cfg(test)]\nmod tests', 1)[0]
assert sha(engine_prefix) == engine_review['production_sha256']
assert sha(fixed_raw('crates/cc-search/src/engine_lane_tests.rs')) == 'f6313e0a098f04be0692c9791927f0190d5cf6c3f3ae11a089372e584eb0e0a8'

tasks_raw = fixed_raw('docs/roadmap/code-index-v2/tasks.json')
tasks_document = json.loads(tasks_raw)
status_counts = dict(Counter(t['status'] for t in tasks_document['tasks']))
assert len(tasks_document['tasks']) == 192 and status_counts.get('done') == 163
assert sha(tasks_raw) == 'e75c68981943d85e31f8830331670f27401d3ab5ca13ae339d2134a693f23063'
assert git_blob(tasks_raw) == '435f27d39b8244468f62601063d4375d23bbfd79'

local_review_raw = extra_historical_raw(UPSTREAM_LOCAL_REVIEW_PATH)
local_review = json.loads(local_review_raw)
assert sha(local_review_raw) == '187d9b22ab7f09ed0bf52c25a16f514c1f3e93e13827e221c96d2dc3a7e571d5'
assert local_review['source'] == UPSTREAM_LOCAL_P
old_static = local_review['review_execution']['fixed_P_local_static_checks']
assert old_static['source'] == UPSTREAM_LOCAL_P
assert len(old_static['completed_commands_observed_before_disk_exhaustion']) == 2

findings = [
    {'id': 'INSTALLER_STRUCTURAL_TOML', 'paths': [NEW_MAIN[5]], 'status': 'accepted_scoped',
     'finding': 'The complete main55 target and original FD9 delta were read. DocumentMut parses before mutation or write; only NotFound is treated as absent. Syntax, invalid UTF-8, wrong table shape, URL transport and non-UTF-8 binary-path failures propagate without rewriting the configuration. Normal, inline and dotted TOML tables are handled through table-like access. Installation addresses the exact parsed codecortex key and replaces only command and args. Existing value decoration and in-place item replacement preserve comments and quoted key formatting; env, timeout, enabled fields and other servers remain. Serialization escapes binary-path contents. Uninstallation removes only the exact parsed codecortex subtree, including nested fields; literal dotted names and header-looking multiline text are not mistaken for target sections. Absent config/server returns before writing. This source still uses an ordinary file write and introduces no atomic-replacement, cross-target rollback or concurrent-editor guarantee.'},
    {'id': 'INSTALLER_CLI_FAILURE', 'paths': [NEW_MAIN[3], NEW_MAIN[6]], 'status': 'accepted_scoped',
     'finding': 'The full CLI and new real-process controls were read with unchanged installer orchestration and main. current_exe errors now propagate. Per-target reporting and processing of other targets remain unchanged; any collected target error returns CcError::Config, which the unchanged main converts to nonzero exit. Two Unix-only CLI tests use the Cargo-built executable and isolated HOME, USERPROFILE and XDG_CONFIG_HOME to check malformed-config failure without rewriting and exact install/uninstall preservation. These source controls do not claim a new Windows CLI execution or reversal of successfully updated other targets.'},
    {'id': 'INSTALLER_CONTROLS', 'paths': [NEW_MAIN[5], NEW_MAIN[6]], 'status': 'accepted_scoped',
     'finding': 'The added controls cover escaped literal paths; exact server-key matching; reinstall fields, prefix/value comments and idempotence; normal, inline and dotted tables; nested and multiline removal; malformed UTF-8/TOML/types without writes; byte-preserving absent-server removal; URL-transport refusal; and Unix non-UTF-8 binary-path refusal. The final in-place key replacement and explicit prefix-comment assertions preserve the earlier independently found and corrected comment-loss regression. Source review of these controls is distinct from their original or new execution.'},
    {'id': 'INSTALLER_HELPER_AND_DEPENDENCY', 'paths': [NEW_MAIN[0], NEW_MAIN[2], NEW_MAIN[4]], 'status': 'accepted_scoped',
     'finding': 'The removed pub(crate) append_if_missing helper had only the replaced Codex production consumer; no reference remains in any actual product blob. The server adds toml_edit 0.22.27, already locked indirectly through cc-index -> toml 0.8.23 -> toml_edit. Parsing both complete lockfiles and removing only the new cc-server dependency edge reconstructs the old lockfile data exactly: no package version, checksum or transitive resolution changes. This is a source/dependency declaration check, not a new offline build result.'},
    {'id': 'SEARCH_TEST_OWNERSHIP', 'paths': [NEW_MAIN[1]], 'status': 'accepted_scoped',
     'finding': 'The 18,720-byte production prefix before cfg(test) is byte-identical, SHA-256 4cc88b93e3db27bc5de0b9de4865df9db1299d7b9cc1d43b37ea5c2a51abc3fa. The remaining 17 test bodies and four helper bodies are byte-identical. Eighteen removed tests and one removed helper already reside in the unchanged, registered engine_lane_tests module. Seventeen duplicate tests and the helper match after indentation; lexical_lane_adapter_matches_inline_ranking differs only in its explanatory comment, with all executable bytes identical. No production scoring, RRF API or search policy changes. Broader duplicated local lane ownership remains outside this cleanup and no whole P8-017 closure is claimed.'},
    {'id': 'CONFIG_EMPTY', 'paths': [OURS[2], OURS[3]], 'status': 'accepted_scoped',
     'finding': 'The current main55-to-P4 delta and both controls were reread, and their full bodies match the original P2 and previously independently reviewed P3. Signature then fallible scan then typed-config exclusion still precede the empty-token decision. Only unused pure full symbol/file preparation is skipped; common payload metadata, token cache, writes and normal nonempty-token builder retain their inputs and behavior. The controls retain linked-to-empty full replacement, later incremental mutation/relinking and typed TS alias resolution with no heuristic tokens. No new execution of every snapshot adapter is claimed.'},
    {'id': 'INTERFACE_EMPTY', 'paths': [OURS[0], OURS[1]], 'status': 'accepted_scoped',
     'finding': 'The current main55-to-P4 delta and complete eight controls were reread; full bodies match P2 and P3. Disabled behavior and cleanup ownership are unchanged. Typed CALL reads plus prior in-round CALL overlays precede symbols, which precede implements. Existing no-call and no-interface read boundaries remain. All selected rows still undergo the old typed decoding before the new empty-implements return. The persisted unique symbol_uid contract makes direct interface/trait membership equivalent to the old map projection, including omitted empty UIDs. Only unused full lookup maps move; no SQL-read reduction or changed error precedence is claimed. The normal fanout cap, insertion/deduplication and edge fields are unchanged. Malformed CALL/symbol/implements controls and three corrected expected expect_err assertions remain intact.'},
    {'id': 'P4_COMPOSITION', 'paths': [*NEW_MAIN, *OURS], 'status': 'accepted_scoped',
     'finding': 'The seven main55 changes and four retained P2 product paths are disjoint. Installer and CLI changes do not alter resolver, config-link, database-write or synthesis contracts. The engine change is confined to duplicate test ownership. The previous #175 statistics and #179 resolver product changes remain exactly inherited through the main55 canonical, with no source or test execution relabeled. Twenty selected supporting paths retain complete mode/type/blob identity across P3, main55 and P4. All 1092 current product files, 139 validation files and all 53 BASE before/after deltas are independently read and hashed below; this complete inventory check is not a claim that every unchanged file was freshly semantically reread.'},
]
finding_by_path = {p: [f['id'] for f in findings if p in f['paths']] for p in [*NEW_MAIN, *OURS]}
source_reviews = {}
for path in changed:
    row = {
        'reviewer': '/root/pr_audit', 'current_source': P,
        'current_mode_type_blob': current[path], 'sha256': complete[path],
        'base_source': BASE, 'base_mode_type_blob': base.get(path),
        'before_sha256': delta[path]['before_sha256'],
        'current_findings': finding_by_path.get(path, []),
    }
    if path in OURS:
        row.update({
            'origin_source': P2, 'composition_parent': B4,
            'origin_review_source': P2_R, 'origin_review_path': P2_REVIEW_PATH,
            'origin_review_sha256': sha(p2_raw),
            'previous_combination_source': P3, 'previous_combination_review': P3_R,
            'previous_combination_review_path': P3_REVIEW_PATH,
            'previous_combination_review_sha256': sha(p3_raw),
            'inherited_original_review_record': p2_review['source_path_reviewers'][path] if 'source_path_reviewers' in p2_review else p2_review['paths'][path],
            'semantic_review': 'Current non-author reread of the actual main55-to-P4 change and complete retained controls; unchanged complete P2/P3 body, full BASE before/after hashes and prior current-session semantic approval independently rechecked.'})
    else:
        row.update({
            'origin_source': UPSTREAM_P, 'composition_parent': MAIN,
            'origin_review_source': UPSTREAM_R, 'origin_review_path': UPSTREAM_REVIEW_PATH,
            'origin_review_sha256': sha(upstream_raw),
            'inherited_original_review_record': upstream['source_path_reviewers'][path],
            'semantic_review': (
                'Current non-author semantic review of the complete installer/CLI/new-test bodies or exact removed/helper/dependency delta and relevant unchanged contracts, followed by actual P4 byte and BASE-delta verification.'
                if path in NEW_MAIN else
                'Inherited semantic approval from the fixed main55 published canonical and its original non-author/recusal chain. This session read and hashed the actual full body and checked complete inheritance, but does not claim to semantically reread this entire unchanged historical component.')})
    source_reviews[path] = row
validation_reviews = {
    path: {'reviewer': '/root/pr_audit', 'current_source': P,
           'origin_source': UPSTREAM_P, 'composition_parent': MAIN,
           'mode_type_blob': vals[path], 'sha256': digest,
           'origin_review_source': UPSTREAM_R, 'origin_review_path': UPSTREAM_REVIEW_PATH,
           'inherited_original_review_record': upstream['validation_path_reviewers'][path],
           'semantic_review': 'Actual complete byte inventory equals the fixed main55 original source and prior P3 validation approval. No validation algorithm or entry point changes; no assertion of fresh execution or semantic rereading of every unchanged validator.'}
    for path, digest in validation_inputs.items()
}
original_reviews = []
for path in sorted(extra_paths):
    if path.startswith(UPSTREAM_PREFIX + 'validation/review/'):
        raw = fixed_raw(path)
        original_reviews.append({'path': path, 'mode_type_blob': trees['P4'][path],
                                 'bytes': len(raw), 'sha256': sha(raw),
                                 'identity': 'Original external-session non-author evidence, unchanged; not a P4 execution'})
semantic_summary = {
    'schema': 'P4-current-semantic-composition-review-v1',
    'source': P, 'tree': P_TREE, 'parents': [B4, MAIN], 'reviewer': '/root/pr_audit',
    'verdict': 'accepted_scoped', 'unresolved_blockers': [],
    'scope': 'Current seven main55 deltas plus four retained P2 paths and selected unchanged contracts; complete immutable source composition',
    'semantic_findings': findings, 'supporting_dependencies': dependencies,
    'engine_mechanical_review': {'file_name': 'engine-static-review.json', 'bytes': len(engine_raw), 'sha256': sha(engine_raw)},
    'inherited_original_reviews': original_reviews,
    'full_input_counts': {'products': len(complete), 'validation': len(validation_inputs), 'BASE_delta': len(delta)},
    'product_execution_by_this_reviewer': False, 'project_validator_execution_by_this_reviewer': False,
    'P4_engineering_results': None,
    'execution_boundary': 'Root owns separately captured actual P4 engineering. No old b069/P2/P3/f7/G source result is relabeled as P4.',
    'TODO_done': 163, 'TODO_remaining': 29, 'TODO_closed': 0,
}
semantic_raw = (json.dumps(semantic_summary, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
review = {
    'schema_version': 1, 'reviewed_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'source': P, 'source_tree': P_TREE, 'tree': P_TREE,
    'source_parent': B4, 'source_parents': [B4, MAIN], 'base': BASE,
    'scope': 'independently_reviewed_source_and_validation_inputs', 'verdict': 'accepted_scoped',
    'independent_reviewers': ['/root/pr_audit'], 'unresolved_blockers': [],
    'complete_input_count': len(complete), 'changed_source_input_count': len(delta),
    'validation_input_count': len(validation_inputs),
    'source_manifest_sha256': sha(canonical(complete)),
    'source_manifest_digest_encoding': 'UTF-8 JSON, sorted keys, separators=(comma,colon), ensure_ascii=False, no trailing newline',
    'paths': delta, 'complete_inputs': complete, 'validation_inputs': validation_inputs,
    'actual_input_mode_blob_inventory': {'source': current, 'validation': vals},
    'source_path_reviewers': source_reviews, 'validation_path_reviewers': validation_reviews,
    'review_basis': {
        'current_semantic_review': 'Current /root/pr_audit reviewed the seven main55 production/test/manifest deltas and full relevant installer/CLI bodies, reread the four retained P2 deltas and controls, and checked their preserved contracts. Actual full source blobs were independently read for complete inventory verification. This does not claim fresh semantic rereading of all 1092 product files or all 139 validation files.',
        'complete_inventory_review': 'Immutable local Git trees independently enumerated in the unchanged original domains; all distinct required Git blobs read with cat-file --batch, Git blob SHA-1 and content SHA-256 recomputed, complete path/mode/type/blob inventories compared. No copied digest map substitutes for source reads. GIT_NO_LAZY_FETCH=1 forbids implicit network hydration.',
        'original_product_roots': ['crates', 'Cargo.toml', 'Cargo.lock'],
        'original_validation_roots': list(constants['VALIDATION_ROOTS']),
        'original_validation_exclusions': sorted(constants['VALIDATION_EXCLUSIONS']),
        'only_generated_validation_exclusion': '__pycache__ path segment together with .pyc suffix',
        'base_reconstruction': {
            'base_input_count': len(base), 'actual_BASE_inventory_sha256': sha(canonical(base)),
            'all_inherited_paths_retained': True, 'removed_product_paths': [],
            'new_product_paths': sorted(set(current) - set(base)),
            'all_53_before_and_after_hashes_recomputed': True,
            'complete_BASE_plus_53_deltas_equals_actual_P4': True},
        'read_unique_Git_blob_count': len(bodies), 'read_unique_Git_blob_bytes': sum(map(len, bodies.values())),
        'additional_historical_reads': VERIFIED_CACHE_READS,
        'supporting_dependencies': dependencies,
        'semantic_summary': {'file_name': 'semantic-composition-review.json', 'bytes': len(semantic_raw), 'sha256': sha(semantic_raw)},
        'engine_mechanical_review': {'file_name': 'engine-static-review.json', 'bytes': len(engine_raw), 'sha256': sha(engine_raw)},
        'inherited_original_reviews': original_reviews,
    },
    'inheritance_evidence': {
        'fixed_main': MAIN, 'fixed_main_tree': git('rev-parse', MAIN + '^{tree}').decode().strip(),
        'inherited_product': UPSTREAM_P, 'inherited_review': UPSTREAM_R,
        'inherited_review_path': UPSTREAM_REVIEW_PATH, 'inherited_review_blob': trees['P4'][UPSTREAM_REVIEW_PATH]['blob'],
        'inherited_review_sha256': sha(upstream_raw), 'inherited_review_bytes': len(upstream_raw),
        'inherited_registry_sha256': sha(registry_raw),
        'inherited_independent_reviewers': upstream['independent_reviewers'],
        'inherited_author_boundaries_verbatim_record': upstream['author_independence'],
        'main55_and_original_product_domains_equal_by_path_mode_type_blob': True,
        'inherited_all_1090_product_and_139_validation_hashes_recomputed': True,
        'inherited_all_49_BASE_delta_rows_recomputed': True,
        'P4_other_1088_main55_product_paths_unchanged': True,
        'P4_validation_139_equals_main55_and_P3': True,
        'P2_four_path_origin': {
            'product': P2, 'review': P2_R, 'review_path': P2_REVIEW_PATH, 'review_sha256': sha(p2_raw),
            'previous_combination_product': P3, 'previous_combination_review': P3_R,
            'previous_combination_review_path': P3_REVIEW_PATH, 'previous_combination_review_sha256': sha(p3_raw),
            'paths': list(OURS), 'actual_complete_bodies_equal_P2_P3_B4_P4': True},
        'all_six_frozen_v14_paths_equal_original_BASE': {p: dict(trees['P4'][p], sha256=validation_inputs[p]) for p in constants['FROZEN']},
        'current_v15_guard_and_registry_selected_from_main55_unchanged': True,
        'guard_complete_bytes_equal_P3_except_four_binding_assignments': True,
        'CI_selector_current_source_entry_and_v15_history_bytes_equal_P3': True,
        'current_guard_constants': {k: v for k, v in constants.items() if k not in ('FROZEN', 'VALIDATION_ROOTS', 'VALIDATION_EXCLUSIONS')},
        'current_P4_binding_status': 'P4 retains main55 trust metadata before rebinding. Final P4/R4/G4 must use the new matching review, registry and four constants before original source admission; existing main55 binding is not a P4 source-proof pass.'},
    'complete_root_composition': {
        'ordered_parents': [B4, MAIN], 'root_leaf_count': len(trees['P4']),
        'root_leaf_inventory_sha256': sha(canonical(trees['P4'])),
        'main55_overlay_count': len(overlay),
        'overlay_counts': {'B4_only_artifacts': len(b4_only), 'P2_Rust_paths': len(OURS), 'merged_tasks_and_derived_documents': len(DOCS)},
        'overlay': {p: trees['P4'][p] for p in overlay},
        'P4_equals_entire_main55_plus_only_the_exact_210_overlay_paths': True,
        'six_documents': {p: trees['P4'][p] for p in DOCS},
        'artifact_union': {
            'main55_count': len(artifacts['main55']), 'B4_count': len(artifacts['B4']),
            'common_count': len(common_artifacts), 'conflicting_common_paths': [],
            'B4_only_count': len(b4_only),
            'main55_only_count': len(set(artifacts['main55']) - set(artifacts['B4'])),
            'P4_count': len(artifacts['P4']), 'P4_is_exact_union': True,
            'evidence_method': 'Complete path/mode/type/Git-blob inventories compared; original archives unchanged, no claim of rerunning or semantically rereading all old proof artifacts'}},
    'semantic_findings': findings,
    'author_independence': {
        'current_reviewer': '/root/pr_audit',
        'current_session': 'This reviewer authored none of the seven main55 product changes, four P2 product files, inherited validation logic or P4 composition, and changed no repository source, guard, index, ref or worktree during this review.',
        'current_composition_author': '/root',
        'historical_names': 'Repeated agent names in external-session archives retain those original sessions and roles. They are not reassigned to this session or treated as newly observed execution.',
        'inherited_recusals': 'Original non-author/recusal records are preserved through the exact fixed main55 and P2 reviews; byte identity never converts authorship into independent semantic approval.'},
    'review_execution': {
        'actual_immutable_Git_reads': True, 'all_required_source_blob_SHA1_and_SHA256_recomputed': True,
        'mutable_HEAD_used_as_source_identity': False,
        'Cargo_or_rustfmt_or_product_tests_executed_by_this_reviewer': False,
        'native_historical_or_source_guard_executed_by_this_reviewer': False,
        'network_fetch_or_Git_object_write_by_this_reviewer': False,
        'repository_source_index_ref_or_worktree_mutation_by_this_reviewer': False,
        'writes': 'Only this scratch source reader, canonical, semantic summary and read receipt',
        'actual_P4_engineering_results': None,
        'actual_P4_engineering_owner': '/root separately captures commands, before/after source identity and original results; this source review grants no engineering status'},
    'historical_execution_identity': {
        'upstream_local_product': UPSTREAM_LOCAL_P,
        'upstream_local_review_path': UPSTREAM_LOCAL_REVIEW_PATH,
        'upstream_local_review_git_blob': trees['P4'][UPSTREAM_LOCAL_REVIEW_PATH]['blob'],
        'upstream_local_review_sha256': sha(local_review_raw),
        'upstream_remote_product': UPSTREAM_P,
        'old_fmt_Clippy_observation_source': old_static['source'],
        'old_fmt_Clippy_preserved_observations': old_static['completed_commands_observed_before_disk_exhaustion'],
        'old_receipt_note_verbatim': old_static['receipt_note'],
        'upstream_workspace_limit': 'The archived unfiltered b069 workspace attempt reached ENOSPC; no full workspace pass or reconstructed numeric result is granted. The original mutable commands.json is empty. Preserved prior reviewer observations and retained logs have distinct evidence strength.',
        'upstream_source_proof_limit': 'The published remote G-prime v15 CLI and seven current-v15 controls preserve their actual remote identity and scope; they are not a P4 Rust run, full native suite result or new product-source admission.',
        'earlier_P2_failure': 'The original P2 unfiltered workspace exit 101 and repeated sampler live_child failure remain failures. Subsequent GitHub/source-proof records keep their actual checkout and environment.',
        'P3_results': 'P3 engineering and G3 source CLI retain their original P3/G3 identity; none is relabeled as P4.'},
    'acceptance_boundary': [
        'Accepted only for fixed P4 product source and validation inputs. No original TODO, unfinished dependency, release, runtime/scale/quality/rollback requirement or full-workspace result is completed by this source review.',
        'All old #175, #179, #185, G275, Gc8, P2, P3 and remote G evidence retains original source/run/artifact identity. Current P4 engineering and final G4 source/CI executions must be evaluated separately.',
        'The inventory reads every actual required body, but semantic review is scoped to changed and interacting code with explicit inherited non-author approval for unchanged historical components.',
        'Later R4 may add this review and bounded evidence outside the original product/validation domains. Later G4 may change only the v15 registry and four PRODUCT/REVIEW/REVIEW_PATH/REGISTRY_SHA256 bindings. BASE, VERSION, exclusions, six frozen v14 files, validation algorithms, CI selection and historical-proof rules stay unchanged.',
        'The main55 guard retained in P4 intentionally still names its old product/review. Its success cannot be borrowed for the new P4 binding; the original final source-admission command must run on actual G4.',
        'Root separately derives documents and records original plan/historical checks. Current actual ledger is 192 total, 163 done, 29 remaining, zero new completions.'
    ],
    'intended_repository_review_path': PREFIX + 'independent-source-review.json',
    'TODO_closed': 0, 'TODO_done': 163, 'TODO_remaining': 29,
    'task_ledger': {'path': 'docs/roadmap/code-index-v2/tasks.json',
                    'git_blob': trees['P4']['docs/roadmap/code-index-v2/tasks.json']['blob'],
                    'sha256': sha(tasks_raw), 'bytes': len(tasks_raw), 'status_counts': status_counts,
                    'identity_equals_separately_reviewed_main_first_B4_append_merge': True},
}
commit_after = git('show', '-s', '--format=%H%n%T%n%P', P).decode().splitlines()
assert commit_before == commit_after
raw_review = (json.dumps(review, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
receipt = {
    'schema': 'P4-independent-source-review-read-receipt-v1', 'reviewer': '/root/pr_audit',
    'source': P, 'source_tree': P_TREE, 'source_parents': [B4, MAIN],
    'fixed_commit_before': commit_before, 'fixed_commit_after': commit_after,
    'mutable_HEAD_read_or_required': False,
    'review_file': 'independent-source-review.json', 'review_bytes': len(raw_review),
    'review_sha256': sha(raw_review), 'review_git_blob': git_blob(raw_review),
    'reader_file': Path(__file__).name, 'reader_sha256': sha(Path(__file__).read_bytes()),
    'semantic_summary_file': 'semantic-composition-review.json',
    'semantic_summary_bytes': len(semantic_raw), 'semantic_summary_sha256': sha(semantic_raw),
    'read_unique_Git_blob_count': len(bodies), 'read_unique_Git_blob_bytes': sum(map(len, bodies.values())),
    'additional_historical_reads': VERIFIED_CACHE_READS,
    'complete_input_count': len(complete), 'changed_source_input_count': len(delta),
    'validation_input_count': len(validation_inputs),
    'commands': COMMANDS, 'all_assertions_passed': True, 'prohibited_actions_performed': [],
    'product_or_guard_execution': False,
}
outputs = {
    'independent-source-review.json': raw_review,
    'semantic-composition-review.json': semantic_raw,
    'independent-source-review-read-receipt.json': (json.dumps(receipt, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode(),
}
assert all(not (D / name).exists() for name in outputs), 'Do not overwrite fixed review outputs'
for name, raw in outputs.items():
    (D / name).write_bytes(raw)
print(json.dumps({k: receipt[k] for k in (
    'source', 'source_tree', 'review_bytes', 'review_sha256', 'review_git_blob',
    'read_unique_Git_blob_count', 'read_unique_Git_blob_bytes',
    'complete_input_count', 'changed_source_input_count', 'validation_input_count',
    'all_assertions_passed')}))
