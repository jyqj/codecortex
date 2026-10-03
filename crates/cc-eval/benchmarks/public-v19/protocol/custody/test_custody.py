import copy
import importlib.util
import json
from pathlib import Path
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('custody_check', HERE / 'check_custody.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class Custody(unittest.TestCase):
    def setUp(self):
        self.snapshot = json.loads((HERE / 'current-snapshot.json').read_text())

    def run_snapshot(self):
        return check.audit(self.snapshot, 'f'*64)

    def test_current_counts_do_not_certify_clean(self):
        r = self.run_snapshot()
        self.assertEqual(r['errors'], {})
        self.assertEqual(r['would_be_holdout_ids'], 117)
        self.assertEqual(r['known_exposed_ids_lower_bound'], 72)
        self.assertEqual(r['clean_holdout_ids_certified_by_checker'], 0)
        self.assertEqual(r['shards'][1]['remaining_without_confirmed_exposure_ids'], 23)

    def test_unseen_ranking_and_reader_claim_do_not_restore_exposed(self):
        s = self.snapshot['shards'][0]
        s.update(unknown_actual_readers=False, actual_readers_audit_sha256='a'*64, tuning_read_audit_sha256='b'*64)
        self.assertEqual(self.run_snapshot()['shards'][0]['status'], 'confirmed_exposure_blocks_clean_holdout')

    def test_even_complete_claim_is_not_verified_acl(self):
        self.snapshot['shards'] = [self.snapshot['shards'][1]]
        s = self.snapshot['shards'][0]
        s['known_exposed_ids_lower_bound'] = 0
        s['unknown_actual_readers'] = False
        for k in s:
            if k.endswith('_sha256'):s[k] = 'a'*64
        b = self.snapshot['boundary']
        b.update(status='claimed_verified',route='separate_existing_account',tuning_principal_excluded=True)
        for k in b:
            if k.endswith('_sha256'):b[k] = 'b'*64
        r = self.run_snapshot()
        self.assertEqual(r['shards'][0]['status'], 'requires_independent_custody_verification')
        self.assertEqual(r['clean_holdout_ids_certified_by_checker'], 0)

    def test_unknown_counts_stay_unknown_without_inference(self):
        s = self.snapshot['shards'][3]
        self.assertIsNone(s['pending_components'])
        self.assertIsNone(s['public_dev_compat'])
        r = self.run_snapshot()
        self.assertEqual(r['errors'], {})
        self.assertEqual(r['shards'][3]['remaining_without_confirmed_exposure_ids'], 22)
        self.assertEqual(r['clean_holdout_ids_certified_by_checker'], 0)

    def test_unknown_body_metadata_field_rejected_not_echoed(self):
        self.snapshot['shards'][0]['query'] = 'SECRET_SYNTHETIC_SENTINEL'
        r = self.run_snapshot()
        self.assertEqual(r['status'], 'invalid_metadata_receipt')
        self.assertNotIn('SECRET_SYNTHETIC_SENTINEL', json.dumps(r))

    def test_inconsistent_counts_and_hashes_rejected(self):
        s = self.snapshot['shards'][0]
        s['known_exposed_ids_lower_bound'] = 99
        s['gold_file_sha256'] = 'not-a-hash'
        r = self.run_snapshot()
        self.assertIn('count_bounds', r['errors'])
        self.assertIn('shard_evidence_hash', r['errors'])

    def test_claim_no_read_without_audit_is_unproven(self):
        self.snapshot['shards'][1]['unknown_actual_readers'] = False
        self.assertIn('unproven_reader_claim', self.run_snapshot()['errors'])

    def test_observation_timestamp_is_not_visibility_time(self):
        r = self.run_snapshot()
        self.assertTrue(all(not s['first_visibility_verified_by_checker'] for s in r['shards']))
        self.snapshot['shards'][0]['first_public_visibility_utc'] = 'not-a-time'
        self.assertIn('visibility_time', self.run_snapshot()['errors'])


if __name__ == '__main__':unittest.main()
