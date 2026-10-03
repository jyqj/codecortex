#!/usr/bin/env python3
"""Counts-only receipt state check. Never authorizes or certifies clean holdout."""
import argparse
from collections import Counter
from datetime import datetime
import hashlib
import json
from pathlib import Path
import re

ROOT_KEYS = {'schema_version', 'observed_utc', 'evidence_origin', 'protocol_sha', 'boundary', 'capabilities', 'shards'}
BOUNDARY_KEYS = {'status', 'route', 'audience_evidence_sha256', 'read_export_audit_sha256',
                 'independent_verifier_receipt_sha256', 'tuning_principal_excluded'}
CAP_KEYS = {'library_read_only_call', 'library_permission_inspection', 'library_agent_task_exclusion',
            'isolated_task_runtime_boundary', 'metadata_header_requests', 'mutations_performed'}
SHARD_KEYS = {'repo_id', 'author_commit', 'candidate_ids', 'pending_components', 'public_dev_native',
              'public_dev_compat', 'would_be_holdout_ids', 'known_exposed_ids_lower_bound',
              'independent_gold_reviewed', 'query_file_sha256', 'gold_file_sha256', 'relation_graph_sha256',
              'first_public_visibility_utc', 'first_shared_visibility_utc', 'could_view_principals_audit_sha256',
              'actual_readers_audit_sha256', 'tuning_read_audit_sha256', 'exposure_disclosure_sha256',
              'custody_receipt_sha256', 'unknown_actual_readers', 'status'}
COUNTS = ['candidate_ids', 'pending_components', 'public_dev_native', 'public_dev_compat',
          'would_be_holdout_ids', 'known_exposed_ids_lower_bound', 'independent_gold_reviewed']


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def timestamp(s):
    if s is None:
        return True
    try:
        return datetime.fromisoformat(s.replace('Z', '+00:00')).utcoffset() is not None
    except (ValueError, TypeError, AttributeError):
        return False


def audit(obj, raw_sha256):
    errors = Counter()
    try:
        if set(obj) != ROOT_KEYS or obj['schema_version'] != 1:
            raise ValueError
        boundary, capabilities = obj['boundary'], obj['capabilities']
        if set(boundary) != BOUNDARY_KEYS or set(capabilities) != CAP_KEYS:
            raise ValueError
        if not isinstance(obj['shards'], list) or not obj['shards']:
            raise ValueError
        if not timestamp(obj['observed_utc']) or obj['observed_utc'] is None:
            errors['observation_time'] += 1
        if not re.fullmatch('[0-9a-f]{40}', str(obj['protocol_sha'])):
            errors['protocol_commit'] += 1
        for key, value in boundary.items():
            if key.endswith('_sha256') and value is not None and not re.fullmatch('[0-9a-f]{64}', str(value)):
                errors['boundary_evidence_hash'] += 1
        seen = set()
        rows = []
        for s in obj['shards']:
            if set(s) != SHARD_KEYS:
                raise ValueError
            repo = s['repo_id']
            if repo not in ('serde','express','typescript','requests','gin','vite') or repo in seen:
                errors['shard_identity'] += 1
            seen.add(repo)
            if not re.fullmatch('[0-9a-f]{40}', str(s['author_commit'])):
                errors['author_commit'] += 1
            if any(type(s[k]) is not int or s[k] < 0 for k in COUNTS):
                raise ValueError
            if (s['known_exposed_ids_lower_bound'] > s['would_be_holdout_ids']
                    or s['would_be_holdout_ids'] > s['candidate_ids']
                    or s['pending_components'] > s['candidate_ids']
                    or s['public_dev_compat'] > s['public_dev_native']
                    or s['public_dev_native'] > s['candidate_ids']):
                errors['count_bounds'] += 1
            for key, value in s.items():
                if key.endswith('_sha256') and value is not None and not re.fullmatch('[0-9a-f]{64}', str(value)):
                    errors['shard_evidence_hash'] += 1
                if key.endswith('_utc') and not timestamp(value):
                    errors['visibility_time'] += 1
            if not s['unknown_actual_readers'] and (not s['actual_readers_audit_sha256'] or not s['tuning_read_audit_sha256']):
                errors['unproven_reader_claim'] += 1
            exposed = s['known_exposed_ids_lower_bound']
            proof_complete = (boundary['status'] == 'claimed_verified'
                              and boundary['route'] in ('isolated_task_runtime', 'separate_existing_account')
                              and boundary['tuning_principal_excluded'] is True
                              and all(boundary[k] for k in BOUNDARY_KEYS if k.endswith('_sha256'))
                              and all(s[k] for k in SHARD_KEYS if k.endswith('_sha256'))
                              and not s['unknown_actual_readers'])
            state = ('confirmed_exposure_blocks_clean_holdout' if exposed else
                     'requires_independent_custody_verification' if proof_complete else
                     'custody_blocked_no_verified_boundary_or_reader_history')
            rows.append({'repo_id':repo, 'author_commit':s['author_commit'], 'candidate_ids':s['candidate_ids'],
                         'would_be_holdout_ids':s['would_be_holdout_ids'], 'known_exposed_ids_lower_bound':exposed,
                         'remaining_without_confirmed_exposure_ids':s['would_be_holdout_ids'] - exposed,
                         'clean_holdout_ids_certified_by_checker':0, 'actual_reader_history_unknown':s['unknown_actual_readers'],
                         'first_visibility_verified_by_checker':False, 'status':state})
    except (ValueError, TypeError, KeyError):
        return {'status':'invalid_metadata_receipt', 'errors':{'metadata_schema':1}, 'input_sha256':raw_sha256,
                'clean_holdout_ids_certified_by_checker':0}
    return {'schema_version':1, 'status':'invalid_metadata_receipt' if errors else 'custody_blocked',
            'input_sha256':raw_sha256, 'errors':dict(errors), 'shards':rows,
            'would_be_holdout_ids':sum(r['would_be_holdout_ids'] for r in rows),
            'known_exposed_ids_lower_bound':sum(r['known_exposed_ids_lower_bound'] for r in rows),
            'clean_holdout_ids_certified_by_checker':0, 'affected_global_components':'unknown_relation_graph_required',
            'other_shards_custody':'unknown_not_assumed_clean',
            'not_verified':['ACL_or_principal_identity','actual_read_events','first_public_or_shared_visibility',
                            'body_exposure_hash_authenticity','independent_source_gold_review','semantic_family_relations']}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--snapshot', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    args = p.parse_args()
    try:
        raw = args.snapshot.read_bytes()
        if len(raw) > 1_000_000:
            raise ValueError
        result = audit(json.loads(raw), sha(raw))
    except (OSError, ValueError):
        result = {'status':'invalid_metadata_receipt','errors':{'metadata_input':1},'clean_holdout_ids_certified_by_checker':0}
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    print(json.dumps({'status':result['status'], 'clean_holdout_ids':0, 'receipt_sha256':sha(args.output.read_bytes())}))
    return 1  # The checker never certifies real access control or clean holdout.


if __name__ == '__main__':
    raise SystemExit(main())
