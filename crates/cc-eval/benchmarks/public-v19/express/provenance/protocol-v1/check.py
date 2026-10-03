#!/usr/bin/env python3
"""Read explicit V19 shards; emit hashes/counts only. No ranking or gold writes."""
import argparse
from collections import Counter, defaultdict
import hashlib
import json
from pathlib import Path
import re
import subprocess
import unicodedata

HERE = Path(__file__).resolve().parent
CATEGORIES = {'file_exact_match', 'configuration_lookup', 'component_location', 'api_usage',
              'semantic_feature', 'error_handling', 'architecture_understanding',
              'cross_language', 'symbol_location', 'call_chain'}
PREFIX = b'codecortex-public-v19-split-v1\n'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def split(component):
    return 'holdout' if int.from_bytes(hashlib.sha256(PREFIX + component.encode()).digest()[:8], 'big') < 2**62 else 'dev'


def relative(path):
    return (isinstance(path, str) and path and not any(c in path for c in '\\:*?[')
            and all(p not in ('', '.', '..') for p in path.split('/'))
            and all(ord(c) >= 32 for c in path))


def audit(shards, relations=None, custodian=False, evaluator=None, suites=()):
    import jsonschema  # Installed local dependency; never install/download automatically.
    schema = json.loads((HERE / 'evaluator-query.schema.json').read_text())
    validator = jsonschema.Draft202012Validator(schema)
    locked = json.loads((HERE / 'source-locks.json').read_text())['candidates']
    source_shas = {r['repository'].split('/')[-1].lower(): r['source_sha'] for r in locked}
    errors = Counter()
    counts = defaultdict(Counter)
    hashes = []
    ids = {}
    family_splits = {}
    components = defaultdict(set)
    text_ids = defaultdict(set)
    native_ids, compat_ids = set(), set()
    native_rows, compat_rows = {}, {}
    global_members = {}
    if relations:
        try:
            raw = Path(relations).read_bytes()
            obj = json.loads(raw)
            hashes.append({'kind': 'relations', 'sha256': sha(raw)})
            for c in obj['components']:
                members = c['members']
                canonical = c['global_family']
                if not members or len(set(members)) != len(members) or canonical != min(members):
                    errors['invalid_relation_component'] += 1
                for m in members:
                    if m in global_members:
                        errors['duplicate_relation_membership'] += 1
                    global_members[m] = canonical
        except (OSError, ValueError, KeyError, TypeError):
            errors['invalid_relation_file'] += 1
    for repo, profile, path in shards:
        if repo not in source_shas or profile not in ('native', 'compat'):
            errors['invalid_shard_identity'] += 1
            continue
        try:
            raw = Path(path).read_bytes()
            lines = raw.decode('utf-8-sig').splitlines()
        except (OSError, UnicodeError):
            errors['unreadable_shard'] += 1
            continue
        hashes.append({'repo': repo, 'profile': profile, 'sha256': sha(raw), 'bytes': len(raw)})
        found = 0
        for line in lines:
            if not line.strip():
                continue
            found += 1
            counts[repo][profile + '_rows'] += 1
            try:
                q = json.loads(line)
            except ValueError:
                errors['invalid_json'] += 1
                continue
            if list(validator.iter_errors(q)):
                errors['evaluator_schema'] += 1
                continue
            f, ident = q['query_family'], q['id']
            if not re.fullmatch(r'v19\.' + repo + r'\.f(?!0000)\d{4}', f):
                errors['family_id'] += 1
            if not re.fullmatch(re.escape(f) + r'\.[a-z]{2}(?!00)\d{2}', ident):
                errors['query_id'] += 1
            key = (profile, ident)
            if key in ids:
                errors['duplicate_global_query_id'] += 1
            ids[key] = q
            try:
                a = q['annotations']['v19']
                needed = ['protocol_version', 'repo_id', 'source_sha', 'global_family', 'intent',
                          'query_language', 'hard_scope', 'facets', 'graph_constraints',
                          'mutation_profile', 'review_status']
                if not isinstance(a, dict) or any(k not in a for k in needed):
                    raise ValueError
                if a['protocol_version'] != 1 or a['repo_id'] != repo or a['source_sha'] != source_shas[repo]:
                    errors['source_protocol_lock'] += 1
                component = a['global_family']
                if not isinstance(component, str) or component != global_members.get(f, f):
                    errors['unregistered_global_component'] += 1
                    component = f
                if not isinstance(a['intent'], str) or not a['intent'].strip():
                    errors['empty_intent'] += 1
                if a['query_language'] != ident.rsplit('.', 1)[-1][:2]:
                    errors['variant_language'] += 1
                if not isinstance(a['facets'], list) or not isinstance(a['graph_constraints'], list):
                    raise ValueError
                if a['hard_scope'] != {'path_prefix': q['path_prefix']}:
                    errors['scope_annotation_disagreement'] += 1
                if a['review_status'] not in ('pending', 'accepted', 'quarantine', 'blocked_source_access', 'holdout_custody_blocked'):
                    errors['review_status'] += 1
                counts[repo][profile + '_' + a['review_status']] += 1
                if a['review_status'] == 'accepted' and (not a.get('reviewer_id') or not a.get('author_id')
                                                       or a['reviewer_id'] == a['author_id']
                                                       or not re.fullmatch('[0-9a-f]{64}', str(a.get('review_receipt_sha256', '')))):
                    errors['independent_review_receipt'] += 1
                if q['split'] not in ('dev', 'holdout', 'quarantine'):
                    errors['invalid_split'] += 1
                if q['split'] != 'quarantine' and q['split'] != split(component):
                    errors['deterministic_split'] += 1
                if q['split'] == 'quarantine' and a['review_status'] != 'quarantine':
                    errors['quarantine_status'] += 1
                old = family_splits.setdefault(component, q['split'])
                if old != q['split']:
                    errors['component_crosses_split'] += 1
                components[component].add(f)
            except (KeyError, TypeError, ValueError):
                errors['protocol_annotations'] += 1
                continue
            counts[repo][profile + '_' + q['split']] += 1
            if q['split'] == 'holdout' and not custodian:
                errors['holdout_body_in_public_intake'] += 1
            if q['difficulty'] not in (1, 2, 3) or q['category'] not in CATEGORIES:
                errors['category_difficulty'] += 1
            if not q['query'].strip() or len(q['query'].encode()) > 4096 or not q['language']:
                errors['query_bounds'] += 1
            text = ' '.join(unicodedata.normalize('NFKC', q['query']).casefold().split())
            text_ids[sha(text.encode())].add(ident)
            if q['path_prefix'] is not None and not relative(q['path_prefix'].rstrip('/')):
                errors['path_prefix'] += 1
            if profile == 'compat':
                compat_ids.add(ident)
                compat_rows[ident] = q
                if q['no_answer'] or q['answers'] or not q['expected_files']:
                    errors['compat_gold'] += 1
                if len(set(q['expected_files'])) != len(q['expected_files']) or not all(relative(p) for p in q['expected_files']):
                    errors['compat_literal_paths'] += 1
                continue
            native_ids.add(ident)
            native_rows[ident] = q
            if q['expected_files']:
                errors['native_expected_files'] += 1
            if q['no_answer']:
                counts[repo]['native_no_answer'] += 1
                if q['answers'] or a['facets'] or a['graph_constraints']:
                    errors['no_answer_gold'] += 1
                continue
            if not q['answers'] or not any(g['primary'] for g in q['answers']):
                errors['native_primary'] += 1
            group_ids = [g['id'] for g in q['answers']]
            if len(set(group_ids)) != len(group_ids) or any(not x for x in group_ids):
                errors['native_group_id'] += 1
            for g in q['answers']:
                if g['grade'] not in (1, 2, 3) or not g['alternatives']:
                    errors['native_group_grade'] += 1
                if g['primary'] and g['grade'] != 3:
                    errors['protocol_primary_grade'] += 1
                for alt in g['alternatives']:
                    if not relative(alt['path']):
                        errors['native_path'] += 1
                    if alt.get('span') and alt['span']['start'] >= alt['span']['end']:
                        errors['native_span'] += 1
                    if alt.get('symbol') and not alt['symbol']['name']:
                        errors['native_symbol'] += 1
            facet_ids = set()
            for facet in a['facets']:
                if not isinstance(facet, dict) or not isinstance(facet.get('id'), str) or facet.get('required') is not True or facet.get('group_id') not in group_ids:
                    errors['facet_group'] += 1
                elif facet['id'] in facet_ids:
                    errors['duplicate_facet'] += 1
                else:
                    facet_ids.add(facet['id'])
        if not found:
            errors['empty_shard'] += 1
    errors['duplicate_normalized_query'] += sum(len(v) > 1 for v in text_ids.values())
    for ident in native_ids & compat_ids:
        n, c = native_rows[ident], compat_rows[ident]
        if any(n[k] != c[k] for k in ('query', 'query_family', 'split', 'category', 'language', 'difficulty', 'path_prefix')):
            errors['projection_identity_drift'] += 1
        paths = {a['path'] for g in n['answers'] for a in g['alternatives']}
        if set(c['expected_files']) != paths:
            errors['projection_gold_paths'] += 1
        ordered = list(dict.fromkeys(a['path'] for g in sorted(n['answers'], key=lambda g: not g['primary']) for a in g['alternatives']))
        if c['expected_files'] != ordered:
            errors['projection_gold_order'] += 1
    if compat_ids - native_ids:
        errors['compat_without_native_intake'] += len(compat_ids - native_ids)
    evaluator_checks = []
    if evaluator:
        for suite in suites:
            # CLI diagnostics may contain case/body data: hash only, never echo.
            try:
                proc = subprocess.run([str(evaluator), 'validate', '--suite', str(suite)],
                                      capture_output=True, timeout=120)
                evaluator_checks.append({'suite_sha256': sha(Path(suite).read_bytes()),
                                         'exit_code': proc.returncode,
                                         'diagnostics_sha256': sha(proc.stdout + proc.stderr)})
                if proc.returncode:
                    errors['actual_evaluator_validation'] += 1
            except (OSError, subprocess.TimeoutExpired):
                errors['actual_evaluator_unavailable'] += 1
    errors = {k: v for k, v in sorted(errors.items()) if v}
    return {'schema': 1, 'status': 'rejected' if errors else 'format_checks_passed_not_source_gold_review',
            'query_files': hashes, 'counts': {k: dict(sorted(v.items())) for k, v in sorted(counts.items())},
            'native_unique_query_ids': len(native_ids), 'compat_unique_query_ids': len(compat_ids),
            'global_family_components': len(components), 'native_family_ids': len({q['query_family'] for q in native_rows.values()}),
            'split_counts_global_components': dict(Counter(family_splits.values())), 'errors': errors,
            'actual_evaluator_checks': evaluator_checks,
            'custody': 'location_access_boundary_not_verified_by_checker' if custodian else 'public_intake_holdout_bodies_forbidden',
            'not_verified': ['semantic_family_independence', 'source_gold_correctness', 'independent_reviewer_identity',
                             'source_checkout_license_byte_locks', 'holdout_access_control', 'ranking_quality']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--shard', action='append', required=True, metavar='REPO:PROFILE=FILE')
    parser.add_argument('--relations', type=Path)
    parser.add_argument('--custodian', action='store_true', help='only at separately authorized restricted custody; not an access-control mechanism')
    parser.add_argument('--evaluator', type=Path)
    parser.add_argument('--suite', action='append', type=Path, default=[])
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        shards = []
        for item in args.shard:
            head, path = item.split('=', 1)
            repo, profile = head.split(':', 1)
            shards.append((repo, profile, Path(path)))
        result = audit(shards, args.relations, args.custodian, args.evaluator, args.suite)
    except (ValueError, ImportError, OSError, TypeError, KeyError):
        result = {'status': 'checker_input_or_dependency_blocked', 'errors': {'input_or_dependency': 1}}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'status': result['status'], 'error_count': sum(result['errors'].values()),
                      'receipt_sha256': sha(args.output.read_bytes())}))
    return 1 if result['errors'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
