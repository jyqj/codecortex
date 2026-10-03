import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import check_four_repo as m

EVALUATOR=Path('/tmp/p7-017-build/debug/cc-eval')


class FourRepo(unittest.TestCase):
    def run_case(self,mutator=None):
        original=m.base.load_blob
        def altered(commit,path,proofs):
            raw=original(commit,path,proofs)
            return mutator(commit,path,raw) if mutator else raw
        with tempfile.TemporaryDirectory() as td,patch.object(m.base,'load_blob',altered):
            return m.audit(EVALUATOR,Path(td))

    def test_current_four_repo_admission_and_conservative_pair(self):
        r=self.run_case();self.assertEqual(r['errors'],{})
        self.assertEqual(r['development_admitted_native_rows'],301)
        self.assertEqual(r['repo_results']['typescript']['local_components'],72)
        self.assertEqual(r['repo_results']['typescript']['independent_component_needschange_preserved'],1)
        self.assertEqual(r['global_review']['conservative_global_correlation_components'],280)
        self.assertEqual(r['global_review']['cross_pairs_automatically_checked'],33801)
        self.assertEqual(sum(len(v['actual_evaluator_validations']) for v in r['repo_results'].values()),16)
        self.assertEqual(r['clean_holdout'],0);self.assertEqual(r['formal_600_accepted_families'],0)

    def test_typescript_source_tamper_is_not_admitted(self):
        r=self.run_case(lambda c,p,b:b+b'\n' if p.endswith('typescript/source/packages/typescript/src/api/options.ts') else b)
        self.assertIn('TS_SOURCE_BYTES_DRIFT',r['errors']);self.assertIn('TS_OFFICIAL_BYTES_DRIFT',r['errors'])
        self.assertEqual(r['development_admitted_native_rows'],0)

    def test_unreviewed_content_error_cannot_be_resolved_by_pair(self):
        def mutate(c,p,b):
            if p.endswith('typescript/remaining/block-041-060/review-receipt.json'):
                d=json.loads(b);row=next(x for x in d['rows'] if x['decision']=='needschange')
                row['error_codes'].append('R_SOURCE_FACT_WRONG');return json.dumps(d).encode()
            return b
        r=self.run_case(mutate);self.assertIn('TS_CONTENT_REVIEW_INCOMPLETE',r['errors'])

    def test_typescript_query_allowlist_blocks_before_body_read(self):
        def mutate(c,p,b):
            if p.endswith('typescript/blocks/01-migration/suite.native.candidate.json'):
                d=json.loads(b);d['queries']='forbidden.jsonl';return json.dumps(d).encode()
            self.assertNotIn('forbidden',p);return b
        with self.assertRaisesRegex(ValueError,'QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST'):self.run_case(mutate)

    def test_pair_source_witness_drift_blocks_admission(self):
        original=Path.read_text
        def altered(path,*args,**kwargs):
            raw=original(path,*args,**kwargs)
            if path.name=='pair-adjudication.json':
                d=json.loads(raw);d['shared_obligation_span_sha256']=['0'*64];return json.dumps(d)
            return raw
        with patch.object(Path,'read_text',altered):r=self.run_case()
        self.assertIn('TS_PAIR_SOURCE_DRIFT',r['errors']);self.assertEqual(r['development_admitted_native_rows'],0)


if __name__=='__main__':unittest.main()
