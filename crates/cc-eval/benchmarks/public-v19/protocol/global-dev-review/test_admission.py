import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

HERE=Path(__file__).resolve().parent
spec=importlib.util.spec_from_file_location('admission',HERE/'check_admission.py');m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
EVALUATOR=Path('/tmp/p7-017-build/debug/cc-eval')


class Admission(unittest.TestCase):
    def run_case(self,mutator=None,include_gin=False):
        original=m.load_blob
        def altered(commit,path,proofs):
            raw=original(commit,path,proofs)
            return mutator(commit,path,raw) if mutator else raw
        with tempfile.TemporaryDirectory() as td,patch.object(m,'load_blob',altered):
            return m.audit(EVALUATOR,Path(td),include_gin=include_gin)

    def test_gin_extension_six_validations_and_public_scope(self):
        r=self.run_case(include_gin=True);self.assertEqual(r['errors'],{})
        self.assertEqual(r['development_admitted_native_rows'],228)
        self.assertEqual(r['repo_results']['gin']['local_components'],66)
        self.assertEqual(r['repo_results']['gin']['current_content_hash_accept'],67)
        self.assertEqual(r['global_review']['conservative_global_correlation_components'],208)
        self.assertEqual(r['global_review']['cross_pairs_automatically_checked'],17157)
        self.assertEqual(r['clean_holdout'],0);self.assertEqual(r['protected_body_reads'],0)
        self.assertEqual(sum(len(x['actual_evaluator_validations']) for x in r['repo_results'].values()),6)

    def test_gin_upstream_proof_tamper_blocks(self):
        def mutate(c,p,b):
            if p.endswith('reviews/gin/provenance/upstream-admission.json'):
                d=json.loads(b);d['inventory'][0]['upstream_and_snapshot_git_blob']='0'*40;return json.dumps(d).encode()
            return b
        r=self.run_case(mutate,include_gin=True)
        self.assertIn('GIN_UPSTREAM_BYTES_DRIFT',r['errors']);self.assertEqual(r['development_admitted_native_rows'],0)

    def test_gin_reviewed_row_commitment_tamper_blocks(self):
        def mutate(c,p,b):
            if p.endswith('reviews/gin/repair-v2/dev-004-repair-review.json'):
                d=json.loads(b);d['retained_prior_acceptance'][0]['native_row_sha256']='0'*64;return json.dumps(d).encode()
            return b
        r=self.run_case(mutate,include_gin=True);self.assertIn('GIN_ROW_REVIEW_BINDING',r['errors'])

    def test_gin_non_allowlisted_query_is_rejected_before_read(self):
        def mutate(c,p,b):
            if p.endswith('/gin/suite-native-dev.json'):
                d=json.loads(b);d['queries']='forbidden.jsonl';return json.dumps(d).encode()
            self.assertNotIn('forbidden',p);return b
        with self.assertRaisesRegex(ValueError,'QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST'):self.run_case(mutate,include_gin=True)

    def test_two_repo_frozen_receipt_is_preserved(self):
        r=self.run_case();self.assertEqual(r,json.loads((HERE/'evidence/admission.json').read_text()))

    def test_real_four_suite_dev_admission_and_scope(self):
        r=self.run_case();self.assertEqual(r['errors'],{});self.assertEqual(r['development_admitted_native_rows'],161)
        self.assertEqual(r['global_review']['local_components_before_cross_repo'],149)
        self.assertEqual(r['global_review']['conservative_global_correlation_components'],147)
        self.assertEqual(r['clean_holdout'],0);self.assertEqual(r['formal_600_accepted_families'],0)

    def test_source_tamper_blocks_before_development_acceptance(self):
        r=self.run_case(lambda c,p,b:b+b'\n' if p.endswith('/source/index.js') else b)
        self.assertIn('SOURCE_BYTES_DRIFT',r['errors']);self.assertIn('EVALUATOR_VALIDATION',r['errors']);self.assertEqual(r['development_admitted_native_rows'],0)

    def test_root_license_tamper_fails(self):
        r=self.run_case(lambda c,p,b:b+b'\n' if p.endswith('express/license/LICENSE') else b)
        self.assertIn('ROOT_LICENSE_DRIFT',r['errors'])

    def test_bsd_notice_tamper_fails(self):
        r=self.run_case(lambda c,p,b:b+b'\n' if p.endswith('license/werkzeug-0.6.2/LICENSE') else b)
        self.assertIn('BSD_RETAINED_EVIDENCE_DRIFT',r['errors'])

    def test_incomplete_content_review_does_not_admit(self):
        def mutate(c,p,b):
            if p.endswith('/repair-v1/dev-008-repair-review.json'):
                d=json.loads(b);d['counts']['combined_content_accepted_native_rows']=69;return json.dumps(d).encode()
            return b
        self.assertIn('CONTENT_REVIEW_INCOMPLETE',self.run_case(mutate)['errors'])

    def test_dev_split_drift_rejected_not_formal_holdout(self):
        def mutate(c,p,b):
            if p.endswith('express/intake/dev-repair-v1/queries.native.dev.jsonl'):
                rows=[json.loads(l) for l in b.splitlines()];rows[0]['split']='holdout';return b'\n'.join(json.dumps(q).encode() for q in rows)+b'\n'
            return b
        r=self.run_case(mutate);self.assertIn('NON_DEV_INTAKE',r['errors']);self.assertEqual(r['clean_holdout'],0)

    def test_unknown_suite_query_path_never_loaded(self):
        def mutate(c,p,b):
            if p.endswith('/express/intake/dev-repair-v1/suite.native.dev.json'):
                d=json.loads(b);d['queries']='forbidden.jsonl';return json.dumps(d).encode()
            self.assertNotIn('forbidden',p)
            return b
        with self.assertRaisesRegex(ValueError,'QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST'):self.run_case(mutate)

    def test_transitive_components_do_not_count_twice(self):
        rows=[{'query_family':x} for x in ['a','b','c']]
        self.assertEqual(len(m.components(rows,[['a','b'],['b','c']])),1)
        self.assertEqual(len(m.components(rows,[['a','unread']])),3)


if __name__=='__main__':unittest.main()
