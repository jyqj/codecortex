import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import analyze
import baseline


class BaselineContracts(unittest.TestCase):
    def test_nonadmitted_entry_is_rejected_before_git_read(self):
        with patch.object(baseline.subprocess,'check_output') as read:
            with self.assertRaisesRegex(ValueError,'INPUT_NOT_ADMITTED'):baseline.frozen_blob('forbidden:body.jsonl',{})
            read.assert_not_called()

    def test_semantic_binary_feature_is_rejected(self):
        with tempfile.TemporaryDirectory() as td:
            binary=Path(td)/'binary';binary.write_bytes(b'fixture')
            receipt={'source_sha':baseline.SOURCE,'build_exit_code':0,'artifacts':{
                n:{'copied_binary':str(binary),'binary_sha256':baseline.sha(binary.read_bytes()),'features':['semantic']}
                for n in ['cc-eval','codecortex']}}
            path=Path(td)/'receipt.json';path.write_text(json.dumps(receipt))
            with self.assertRaisesRegex(ValueError,'NETWORK_FEATURE_ENABLED'):baseline.verify_build(path)

    def test_partial_empty_is_not_a_successful_no_answer(self):
        self.assertFalse(analyze.strict_no_answer({'status':'partial','hits':[]}))
        self.assertFalse(analyze.strict_no_answer({'status':'no_match','hits':[{}]}))
        self.assertTrue(analyze.strict_no_answer({'status':'no_match','hits':[]}))

    def test_uneven_repo_and_shared_component_weights_are_explicit(self):
        cases=[{'profile':'native','no_answer':False,'means':{'top1':value},'repo':repo,'category':'fixed','family':family}
               for value,repo,family in [(0,'a','a1'),(0,'a','a2'),(1,'b','b1')]]
        r=analyze.aggregate(cases,'native','top1',{'a1':'shared','a2':'shared','b1':'other'})
        self.assertAlmostEqual(r['query_micro'],1/3);self.assertEqual(r['repository_macro'],.5)
        self.assertEqual(r['family_balanced_repository_macro'],.5);self.assertEqual(r['global_components_in_applicable_cases'],2)

    def test_actual_missing_schedules_block_global_quality_aggregation(self):
        root=Path('/tmp/v19-development-full')
        if not root.exists():self.skipTest('extract retained raw archive and set up documented run path first')
        with tempfile.TemporaryDirectory() as td:
            r=analyze.analyze(baseline.HERE/'preregistration-receipt.json',root,Path(td)/'analysis')
        self.assertIsNone(r['descriptive_supported_scorer_means'])
        self.assertIn('GLOBAL_SCHEDULE_INCOMPLETE',r['errors']);self.assertEqual(r['observed_row_count'],783)
        self.assertEqual(r['complete_paired_answerable_projections'],118)
        self.assertEqual(r['native_no_answer_missing_rows'],60)


if __name__=='__main__':unittest.main()
