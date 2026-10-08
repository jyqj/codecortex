#!/usr/bin/env python3
"""Read fixed public DEV Git blobs; emit metadata-only coexistence evidence."""
import ast
from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess
import sys
import unicodedata

ROOT = Path('/workspace/scratch/031390cf22eb/codecortex-closeout')
OUT = Path('/workspace/scratch/031390cf22eb/round5-corpus-coexistence')
SOURCE = '56d15ed246eedd9add0c0d2843001b5f4e971ee8'
SOURCE_TREE = '2c772058a3d7a859db8770d5d7408acc6ce7440f'
BASE = '615662bd0e651dc40a9d1d0d6757bc28646b900d'
PARALLEL = 'a8ea19cd0d628dc6a62f9b06bde0c5d58e041229'
OURS = 'crates/cc-eval/benchmarks/public-dev-20261008/'
OTHER = 'crates/cc-eval/benchmarks/public-v19/'
INDEX = OURS + 'index.json'
OTHER_INDEX = 'crates/cc-eval/benchmarks/manifests/public-dev-20261008.dataset-index.json'
EVIDENCE = 'artifacts/benchmarks/p8-public-dev-reviewed-20261008/'
PROFILE = {'codecortex-native-v1': 'native', 'oce-compat-v1': 'compat'}
UPSTREAM = {
    'serde': Path('/workspace/scratch/031390cf22eb/session/upstream-serde'),
    'vite': Path('/workspace/scratch/031390cf22eb/upstream-vite'),
}
sha = lambda raw: hashlib.sha256(raw).hexdigest()
canonical = lambda v: json.dumps(v, ensure_ascii=False, sort_keys=True, separators=(',', ':')).encode()
cache, inputs, upstream_inputs = {}, {}, {}


def git(*args, root=ROOT):
    return subprocess.check_output(['git', '-C', str(root), *args])


def safe(p):
    assert isinstance(p, str) and p and not p.startswith('/')
    assert '..' not in PurePosixPath(p).parts and '\\' not in p
    assert not any(x in p.lower() for x in ('holdout', 'protected'))
    return p


def blob(path, commit=SOURCE, expected=None):
    safe(path)
    assert re.fullmatch('[0-9a-f]{40}', commit)
    key = commit + ':' + path
    if key not in cache:
        entry = git('ls-tree', '-z', commit, '--', ':(literal)' + path).split(b'\0')
        assert len(entry) == 2 and not entry[-1], key
        head, actual_path = entry[0].split(b'\t', 1)
        mode, kind, oid = head.decode().split()
        assert kind == 'blob' and mode in ('100644', '100755') and actual_path.decode() == path
        raw = git('show', key)
        cache[key] = raw
        inputs[key] = {'sha256': sha(raw), 'bytes': len(raw), 'mode': mode, 'git_blob': oid}
    if expected is not None:
        assert inputs[key]['sha256'] == expected, key
    return cache[key]


def j(path, commit=SOURCE, expected=None):
    return json.loads(blob(path, commit, expected))


def rows(raw):
    result = [json.loads(line) for line in raw.splitlines() if line.strip()]
    assert all(r['split'] == 'dev' for r in result)
    assert len({r['id'] for r in result}) == len(result)
    return result


def norm(text):
    return ' '.join(unicodedata.normalize('NFKC', text).casefold().split())


def differences(a, b, prefix=''):
    if isinstance(a, dict) and isinstance(b, dict):
        result = []
        for k in sorted(a.keys() | b.keys()):
            path = prefix + '.' + k
            if k not in a or k not in b:
                result.append(path)
            else:
                result.extend(differences(a[k], b[k], path))
        return result
    return [] if a == b else [prefix]


def fixed_upstream(name, commit, path, expected):
    root = UPSTREAM[name]
    safe(path)
    assert git('rev-parse', 'HEAD', root=root).decode().strip() == commit
    raw = git('show', commit + ':' + path, root=root)
    assert sha(raw) == expected
    key = name + ':' + commit + ':' + path
    upstream_inputs[key] = {'sha256': sha(raw), 'bytes': len(raw)}
    return raw


def tree_paths(prefix, commit=SOURCE):
    safe(prefix)
    return [p.decode() for p in git('ls-tree', '-r', '--name-only', '-z', commit, '--', prefix).split(b'\0') if p]


def compare_rows(left, right):
    a, b = ({r['id']: r for r in x} for x in (left, right))
    shared = sorted(a.keys() & b.keys())
    paths = Counter()
    records = []
    for ident in shared:
        delta = differences(a[ident], b[ident])
        paths.update(delta)
        records.append({'id': ident, 'changed_fields': delta,
                        'canonical_non_annotation_sha256': sha(canonical({k: v for k, v in a[ident].items() if k != 'annotations'}))})
        assert {k: v for k, v in a[ident].items() if k != 'annotations'} == {
            k: v for k, v in b[ident].items() if k != 'annotations'}, ident
    return {'left_rows': len(left), 'right_rows': len(right), 'shared_ids': len(shared),
            'unique_union_ids': len(a.keys() | b.keys()), 'changed_fields': dict(sorted(paths.items())),
            'shared_rows': records, 'right_only_ids': sorted(b.keys() - a.keys())}


def methods(raw):
    result = {}
    for node in ast.walk(ast.parse(raw)):
        if isinstance(node, ast.ClassDef):
            for f in node.body:
                if isinstance(f, (ast.FunctionDef, ast.AsyncFunctionDef)) and f.name.startswith('test_'):
                    result[node.name + '.' + f.name] = sha(ast.dump(f, include_attributes=False).encode())
    return result


assert git('rev-parse', SOURCE + '^{tree}').decode().strip() == SOURCE_TREE
observed_head_start = git('rev-parse', 'HEAD').decode().strip()
left_index = j(INDEX)
assert blob(INDEX) == blob(INDEX, BASE)
right_index = j(OTHER_INDEX)
assert blob(OTHER_INDEX) == blob(OTHER_INDEX, PARALLEL)
assert len(left_index['historical_inputs_sha256']) == 183
for entry, digest in left_index['historical_inputs_sha256'].items():
    ref, path = entry.split(':', 1)
    blob(path, ref, digest)
assert len(left_index['current_inputs_sha256']) == 40
actual_new_paths = set(tree_paths(OURS + 'serde/')) | set(tree_paths(OURS + 'vite/'))
assert actual_new_paths == set(left_index['current_inputs_sha256'])
for path, digest in left_index['current_inputs_sha256'].items():
    assert blob(path, SOURCE, digest) == blob(path, BASE)
for path, digest in left_index['helper_inputs_sha256'].items():
    blob(path, SOURCE, digest)
    assert (ROOT / path).read_bytes() == blob(path)
assert (ROOT / 'scripts/p8_corpus_audit.py').read_bytes() == blob('scripts/p8_corpus_audit.py')
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / 'scripts'))
import p8_corpus_audit as audit

left = {p: [] for p in ('native', 'compat')}
right = {p: [] for p in ('native', 'compat')}
left_sources, right_sources, repo_records = {}, {}, {}
suite_records = []
for name, record in left_index['historical_repositories'].items():
    source = {}
    for entry in record['suites']:
        ref, path = entry.split(':', 1)
        suite = j(path, ref)
        qpath = posixpath.normpath(posixpath.join(posixpath.dirname(path), suite['queries']))
        assert ref + ':' + qpath in left_index['historical_inputs_sha256']
        parsed = rows(blob(qpath, ref))
        profile = PROFILE[suite['scoring']]
        left[profile].extend((name, r) for r in parsed)
        assert suite['source']['commit'] is None
        source_base = posixpath.normpath(posixpath.join(posixpath.dirname(path), suite['source']['root']))
        assert source_base == OTHER + name + '/source'
        for rel in suite['source']['files']:
            sp = source_base + '/' + safe(rel)
            assert ref + ':' + sp in left_index['historical_inputs_sha256']
            raw = blob(sp, ref)
            assert rel not in source or source[rel] == raw
            source[rel] = raw
        suite_records.append({'registry': 'DEV600', 'repository': name, 'profile': profile,
                              'original_entry': entry, 'source_digest': suite['source']['digest'],
                              'queries_digest': suite['queries_digest'], 'snapshot_commit_null': True})
    left_sources[name] = source
    repo_records[name] = {'upstream_commit': record['upstream_sha']}

for name, record in left_index['new_repositories'].items():
    base = OURS + name + '/'
    manifest = j(base + 'source-manifest.json')
    source = {f['path']: fixed_upstream(name, record['upstream_sha'], f['path'], f['sha256']) for f in manifest['files']}
    for profile, path in record['suites'].items():
        suite = j(base + path)
        parsed = rows(blob(base + suite['queries']))
        left[profile].extend((name, r) for r in parsed)
        assert set(suite['source']['files']) == set(source)
        assert suite['source']['commit'] == record['upstream_sha']
        suite_records.append({'registry': 'DEV600', 'repository': name, 'profile': profile,
                              'entry': SOURCE + ':' + base + path, 'source_digest': suite['source']['digest'],
                              'queries_digest': suite['queries_digest'], 'upstream_commit': suite['source']['commit']})
    left_sources[name] = source
    repo_records[name] = {'upstream_commit': record['upstream_sha']}

for record in right_index['repositories']:
    name = record['repository']
    manifest = j(record['source_manifest']['path'], expected=record['source_manifest']['sha256'])
    assert record['upstream_commit'] == repo_records[name]['upstream_commit'] == manifest['upstream_commit']
    base = posixpath.dirname(record['source_manifest']['path']) + '/'
    assert base == OTHER + name + '/reviewed-dev-20261008/'
    source = {}
    assert {p[len(base + 'source/'):] for p in tree_paths(base + 'source/')} == {f['path'] for f in manifest['files']}
    for f in manifest['files']:
        raw = blob(base + 'source/' + safe(f['path']), expected=f['sha256'])
        assert len(raw) == f['bytes']
        if name in UPSTREAM:
            assert raw == fixed_upstream(name, record['upstream_commit'], f['path'], f['sha256'])
        source[f['path']] = raw
    for f in manifest['licenses']:
        blob(base + 'licenses/' + safe(f['path']), expected=f['sha256'])
    right_sources[name] = source
    for profile, item in record['profiles'].items():
        qraw = blob(item['queries']['path'], expected=item['queries']['sha256'])
        sraw = blob(item['suite']['path'], expected=item['suite']['sha256'])
        assert len(qraw) == item['queries']['bytes'] and len(sraw) == item['suite']['bytes']
        parsed, suite = rows(qraw), json.loads(sraw)
        assert len(parsed) == item['rows']
        assert suite['source']['commit'] is None and suite['source']['root'] == 'source'
        assert set(suite['source']['files']) == set(source)
        assert posixpath.normpath(posixpath.join(posixpath.dirname(item['suite']['path']), suite['queries'])) == item['queries']['path']
        right[profile].extend((name, r) for r in parsed)
        suite_records.append({'registry': 'DEV327', 'repository': name, 'profile': profile,
                              'entry': SOURCE + ':' + item['suite']['path'], 'source_digest': suite['source']['digest'],
                              'queries_digest': suite['queries_digest'], 'snapshot_commit_null': True})
    common = left_sources[name].keys() & source.keys()
    assert all(left_sources[name][p] == source[p] for p in common)
    repo_records[name].update({'DEV600_source_files': len(left_sources[name]), 'DEV327_source_files': len(source),
        'common_source_files_exact_bytes': len(common), 'DEV327_only_source_paths': sorted(source.keys() - left_sources[name].keys()),
        'unique_source_union_files': len(left_sources[name].keys() | source.keys())})

for entry in right_index['evidence'].values():
    raw = blob(entry['path'], expected=entry['sha256'])
    assert len(raw) == entry['bytes']

mechanical = []
for label, group, sources in (('DEV600', left, left_sources), ('DEV327', right, right_sources)):
    for name in repo_records:
        native = [r for repo, r in group['native'] if repo == name]
        compat = [r for repo, r in group['compat'] if repo == name]
        projection_errors, projected = audit.check_projection(native, compat)
        source_errors, spans = audit.source_check(native, sources[name], repo_records[name]['upstream_commit'])
        assert not projection_errors and not source_errors, (label, name, projection_errors, source_errors)
        mechanical.append({'registry': label, 'repository': name, 'native_rows': len(native), 'compat_rows': len(compat),
                           'primary_first_projection_rows_checked': projected, 'native_gold_alternative_spans_checked': spans,
                           'projection_errors': projection_errors, 'source_evidence_errors': source_errors})

comparisons = {p: compare_rows([r for _, r in left[p]], [r for _, r in right[p]]) for p in left}
assert comparisons['native']['shared_ids'] == 301 and comparisons['native']['unique_union_ids'] == 626
assert comparisons['compat']['shared_ids'] == 256 and comparisons['compat']['unique_union_ids'] == 580
allowed = {'.annotations.v19.global_family', '.annotations.v19.review_projection_provenance',
           '.annotations.v19.review_receipt_sha256', '.annotations.v19.review_status', '.annotations.v19.reviewer_id'}
assert set(comparisons['native']['changed_fields']) <= allowed
assert set(comparisons['compat']['changed_fields']) <= allowed

all_rows = left['native'] + right['native']
union = {}
normalized = defaultdict(set)
for name, r in all_rows:
    union[r['id']] = (name, r)
    normalized[norm(r['query'])].add(r['id'])
different_id_duplicates = [sorted(ids) for ids in normalized.values() if len(ids) > 1]
assert not different_id_duplicates

# Preserve both registries' declared correlations, including the original 280
# historical components. Family labels alone are not a claim of independence.
parent = {ident: ident for ident in union}
def find(x):
    while parent[x] != x:
        parent[x] = parent[parent[x]]
        x = parent[x]
    return x
def join(ids):
    ids = sorted(set(ids))
    for ident in ids[1:]:
        parent[find(ident)] = find(ids[0])
family_ids, global_ids = defaultdict(set), defaultdict(set)
for _, r in all_rows:
    family_ids[r['query_family']].add(r['id'])
    global_ids[r['annotations']['v19'].get('global_family', r['query_family'])].add(r['id'])
for group in list(family_ids.values()) + list(global_ids.values()):
    join(group)
historical_component_path = [p for p in left_index['historical_inputs_sha256']
    if p.startswith(left_index['historical_anchor'] + ':') and p.endswith('/global-components.json')]
assert len(historical_component_path) == 1
ref, path = historical_component_path[0].split(':', 1)
old_components = j(path, ref)['components']
new_components = j(EVIDENCE + 'global-components.json')['components']
for c in old_components + new_components:
    assert all(f in family_ids for f in c['members'])
    join([ident for f in c['members'] for ident in family_ids[f]])
components = defaultdict(list)
for ident in union:
    components[find(ident)].append(ident)
groups = sorted((sorted(v) for v in components.values()), key=lambda x: x[0])
assert len(groups) == 561
assert all({union[i][1]['split'] for i in g} == {'dev'} for g in groups)

extra = set(comparisons['native']['right_only_ids'])
overlaps = []
def spans(row):
    return [(a['path'], a['span']['start'], a['span']['end']) for g in row['answers'] for a in g['alternatives']]
for ident in sorted(extra):
    name, row = union[ident]
    for other_name, other in left['native']:
        if name != other_name:
            continue
        paths = sorted({p for p, lo, hi in spans(row) for q, x, y in spans(other) if p == q and max(lo, x) < min(hi, y)})
        if paths:
            overlaps.append({'new_PR153_id': ident, 'DEV600_id': other['id'], 'overlapping_gold_paths': paths})

new_reviews = []
for name in ('serde', 'vite'):
    path = EVIDENCE + name + '-source-gold-review.json'
    review = j(path)
    base = OTHER + name + '/intake/recovery-authoring-20261008/revision-2/'
    assert blob(path) == blob(base + 'independent-review.json')
    review_ids = {r['id'] for r in review['rows']}
    source_ids = {r['id'] for repo, r in right['native'] if repo == name}
    assert review_ids == source_ids and len(review['rows']) == len(source_ids)
    fixed = review['fixed_inputs']
    if isinstance(fixed, list):
        fixed = {item['role']: item for item in fixed}
    deltas = {}
    for profile in ('native', 'compat'):
        original = blob(base + 'queries.' + profile + '.candidate.dev.jsonl', expected=fixed[profile]['sha256'])
        candidate = rows(original)
        promoted = [r for repo, r in right[profile] if repo == name]
        detail = compare_rows(candidate, promoted)
        assert detail['shared_ids'] == len(candidate) == len(promoted)
        assert set(detail['changed_fields']) <= allowed
        deltas[profile] = {'original_query_sha256': sha(original), 'changed_fields': detail['changed_fields'],
                           'all_query_gold_non_annotation_fields_equal': True}
    new_reviews.append({'repository': name, 'rows': len(review_ids), 'ids': sorted(review_ids),
        'receipt_path': path, 'receipt_sha256': sha(blob(path)), 'author_as_recorded': review['author'],
        'reviewer_as_recorded': review['reviewer'], 'verdict_as_recorded': review['verdict'],
        'origin_thread': 'concurrent PR153 public-dev-reauthoring-20261008; not the current root thread',
        'current_auditor_claims_manual_source_gold_review_of_these_26': False,
        'promotion_deltas': deltas})

test_path = 'scripts/tests/test_p8_compat.py'
old_methods, new_methods = methods(blob(test_path, BASE)), methods(blob(test_path))
assert old_methods.keys() <= new_methods.keys()
assert all(new_methods[k] == v for k, v in old_methods.items())
interface_tests = []
unique_passed = set()
for stem in ('python-interface-controls', 'candidate-execution-discovery'):
    receipt_path = OUT / (stem + '.json')
    receipt = json.loads(receipt_path.read_text())
    log = (OUT / (stem + '.log')).read_text()
    assert sha(log.encode()) == receipt['log_sha256']
    for p, digest in receipt['inputs_sha256'].items():
        blob(p, expected=digest)
    found = re.findall(r'^(test_\S+) \(([^\n]+)\) \.\.\. (ok|ERROR|FAIL|skipped.*)$', log, re.M)
    statuses = Counter(v[2] for v in found)
    for _, full, status in found:
        if status == 'ok':
            unique_passed.add(full.removeprefix('scripts.tests.'))
    interface_tests.append({'receipt': str(receipt_path), 'receipt_sha256': sha(receipt_path.read_bytes()),
        'exit_code': receipt['exit_code'], 'elapsed_seconds': receipt['elapsed_seconds'],
        'method_status_counts': dict(statuses), 'log_sha256': receipt['log_sha256']})
assert len(unique_passed) == 72

current_path_diffs = []
for path in sorted({key.split(':', 1)[1] for key in left_index['historical_inputs_sha256']}):
    before = git('ls-tree', '-z', BASE, '--', ':(literal)' + path)
    after = git('ls-tree', '-z', SOURCE, '--', ':(literal)' + path)
    if before != after:
        current_path_diffs.append(path)

for path in [INDEX, OTHER_INDEX, 'scripts/p8_native_registry.py', 'scripts/p8_corpus_audit.py',
             'scripts/p8_release_evidence.py', 'scripts/p8_compat.py', 'scripts/p8_candidate_execution.py',
             'scripts/p7_build_identity.py', 'scripts/p7_stdio_build_receipt.py']:
    assert (ROOT / path).read_bytes() == blob(path), 'working file changed: ' + path

report = {
    'schema_version': 1, 'status': 'accepted_declared_corpus_coexistence_and_wrapper_scope',
    'auditor': '/root/p8_corpus_closeout', 'observed_at_utc': datetime.now(timezone.utc).isoformat(),
    'scope': 'fixed Git public DEV registry/data coexistence, preserved input locks, exact deduplication and wrapper controls',
    'reviewed_source_commit': SOURCE, 'reviewed_source_tree': SOURCE_TREE,
    'prior_600_source_commit': BASE, 'concurrent_327_source_commit': PARALLEL,
    'observed_worktree_head_at_start': observed_head_start,
    'observed_worktree_head_at_end': git('rev-parse', 'HEAD').decode().strip(),
    'head_is_not_used_to_replace_fixed_blob_identities': True,
    'script_sha256': sha(Path(__file__).read_bytes()),
    'registries': {'DEV600': {'path': INDEX, 'sha256': sha(blob(INDEX)), 'unchanged_from_prior_source': True,
                             'historical_blob_pins_verified': 183, 'current_inputs_verified': 40,
                             'historical_current_path_changes_since_prior_source': current_path_diffs},
                   'DEV327': {'path': OTHER_INDEX, 'sha256': sha(blob(OTHER_INDEX)), 'unchanged_from_PR153': True,
                              'original_rows': 301, 'new_rows': 26, 'new_serde_rows': 14, 'new_vite_rows': 12}},
    'comparisons': comparisons,
    'unique_union': {'native_questions': len(union), 'compat_projections': comparisons['compat']['unique_union_ids'],
        'compat_rows_are_not_additional_questions': True, 'no_answer_questions': sum(r['no_answer'] for _, r in union.values()),
        'normalized_query_algorithm': 'Unicode NFKC, casefold, split and join whitespace',
        'normalized_unique_query_texts': len(normalized), 'different_id_normalized_duplicates': different_id_duplicates,
        'all_occurrences_dev': True, 'query_family_labels': len(family_ids),
        'declared_relation_components_after_union': len(groups), 'declared_relation_components': groups,
        'historical_components': len(old_components), 'PR153_components': len(new_components),
        'all_declared_relation_components_single_dev_split': True,
        'family_labels_or_components_are_not_independent_samples': True,
        'cross_packet_new_vs_600_gold_overlap_pairs': len(overlaps),
        'cross_packet_new_ids_with_gold_overlap': len({r['new_PR153_id'] for r in overlaps}),
        'overlap_relations': overlaps,
        'overlap_scope': 'Actual gold byte intervals on identical fixed repository source; not a semantic equivalence decision or new grouping rule.'},
    'repositories': repo_records,
    'total_unique_admitted_source_files': sum(v['unique_source_union_files'] for v in repo_records.values()),
    'source_and_projection_checks': mechanical, 'fixed_suite_locks': suite_records,
    'concurrent_new26_review_provenance': new_reviews,
    'python_interfaces': {'reviewed_profile_contract': 'local-default remains exact and default; local-text-hidden is explicit and exact; wrong/extra config and non-bool values fail closed',
        'locked_run_checks_preserved': 'query ID x repetition, query/gold bytes, lock rechecks, raw binding unchanged',
        'DEV600_imports_p8_compat': False, 'DEV600_direct_helpers_unchanged': True,
        'old_compat_test_methods_ast_equal': len(old_methods), 'new_compat_test_methods': sorted(new_methods.keys() - old_methods.keys()),
        'actual_control_receipts': interface_tests, 'unique_methods_successfully_executed': len(unique_passed),
        'original_failed_invocation_retained': True,
        'invocation_failure_disposition': 'First dotted-module invocation ran 72 methods: 71 passed and one candidate method could not import test_p8_release_evidence. The unchanged candidate module was run under original CI discover import context: all 8 passed (7 repeated, 1 resolved). No test or source edits; no sum of repeated methods.',
        'actual_retrieval_calls': 0, 'builds': 0},
    'inputs': inputs, 'upstream_sources': upstream_inputs,
    'limitations': [
        'The 600 and 327 registries remain distinct frozen entry points. Their 626-ID union is an audit count, not a newly frozen union suite.',
        'All 301 shared historical questions are counted once. Promotion annotation changes and changed derived query locks are not described as identical file bytes.',
        'Exact text deduplication does not prove absence of semantically related questions. Cross-packet gold overlaps are retained and no statistical independence is claimed.',
        'Current-thread Serde150/Vite149 peer reviews remain distinct from PR153 Serde14/Vite12 reviewers, even where role labels are the same.',
        'All inspected query bodies were explicitly registered public DEV. No fresh or reserved holdout question, suite, raw body or archive was accessed.',
        'This audit does not execute retrieval, authorize ranking comparisons across source/config differences, or certify full V19/G8, quality or full Rust regression.',
        'Completed 174 source-integrity tests and CLI results belong only to old S d5dfebd4f9add3694ec678830e6195c8d38b96f3; final merged-head CI is separate.'
    ], 'protected_or_fresh_holdout_body_reads': 0, 'repository_edits': 0,
}
target = OUT / 'corpus-coexistence-independent.json'
with target.open('x') as stream:
    json.dump(report, stream, ensure_ascii=False, indent=2, sort_keys=True)
    stream.write('\n')
print(json.dumps({'path': str(target), 'sha256': sha(target.read_bytes()), 'status': report['status'],
                  'native_union': len(union), 'compat_union': comparisons['compat']['unique_union_ids'],
                  'declared_components': len(groups), 'source_union': report['total_unique_admitted_source_files'],
                  'cross_packet_overlap_pairs': len(overlaps), 'inputs': len(inputs),
                  'unique_control_methods': len(unique_passed)}))
