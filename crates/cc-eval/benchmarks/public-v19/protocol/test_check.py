"""Synthetic protocol counterexamples; never public-corpus or holdout questions."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('protocol_check', HERE / 'check.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class ProtocolChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.native = json.loads((HERE / 'examples/queries.native.jsonl').read_text())
        self.compat = json.loads((HERE / 'examples/queries.compat.jsonl').read_text())

    def run_rows(self, rows, profile='native', custodian=False, other=(), relations=None):
        p = self.root / ('queries-' + profile + '.jsonl')
        p.write_text(''.join(json.dumps(r) + '\n' for r in rows))
        return check.audit([('serde', profile, p), *other], relations, custodian)

    def test_native_and_compat_actual_minimum_schema(self):
        c = self.root / 'compat.jsonl'
        c.write_text(json.dumps(self.compat) + '\n')
        r = self.run_rows([self.native], other=[('serde', 'compat', c)])
        self.assertEqual(r['errors'], {})
        self.assertEqual(r['global_family_components'], 1)
        self.assertEqual(r['native_unique_query_ids'], 1)

    def test_unknown_top_level_metadata_rejected(self):
        self.native['repo_id'] = 'serde'
        self.assertIn('evaluator_schema', self.run_rows([self.native])['errors'])

    def test_locked_source_cannot_drift(self):
        self.native['annotations']['v19']['source_sha'] = '0' * 40
        self.assertIn('source_protocol_lock', self.run_rows([self.native])['errors'])

    def test_family_ids_and_split_are_global(self):
        self.native['split'] = 'holdout'
        r = self.run_rows([self.native])
        self.assertIn('deterministic_split', r['errors'])
        self.assertIn('holdout_body_in_public_intake', r['errors'])

    def test_real_holdout_shape_rejected_public_and_counts_only_in_custody(self):
        q = copy.deepcopy(self.native)
        q['id'] = 'v19.serde.f0002.en01'
        q['query_family'] = 'v19.serde.f0002'
        q['annotations']['v19']['global_family'] = q['query_family']
        q['split'] = check.split(q['query_family'])
        self.assertEqual(q['split'], 'holdout')
        q['query'] = 'UNIQUE_SYNTHETIC_CUSTODY_SENTINEL'
        r = self.run_rows([q])
        self.assertIn('holdout_body_in_public_intake', r['errors'])
        r = self.run_rows([q], custodian=True)
        self.assertEqual(r['errors'], {})
        receipt = json.dumps(r)
        for sensitive in [q['query'], q['annotations']['v19']['intent'], 'demo.rs', q['id']]:
            self.assertNotIn(sensitive, receipt)

    def test_duplicate_variants_cannot_inflate_count(self):
        q = copy.deepcopy(self.native)
        q['id'] = 'v19.serde.f0001.en02'
        q['query'] = q['query'].upper()
        r = self.run_rows([self.native, q])
        self.assertIn('duplicate_normalized_query', r['errors'])
        self.assertEqual(r['global_family_components'], 1)

    def test_author_cannot_sign_own_review(self):
        a = self.native['annotations']['v19']
        a.update(review_status='accepted', author_id='author', reviewer_id='author', review_receipt_sha256='a'*64)
        self.assertIn('independent_review_receipt', self.run_rows([self.native])['errors'])
        a['reviewer_id'] = 'different-reviewer'
        self.assertEqual(self.run_rows([self.native])['errors'], {})
        # Identity authenticity/source review still explicitly not verified by this tool.

    def test_unregistered_relation_and_cross_split_components_rejected(self):
        self.native['annotations']['v19']['global_family'] = 'v19.express.f0001'
        self.assertIn('unregistered_global_component', self.run_rows([self.native])['errors'])
        q = copy.deepcopy(self.native)
        q['id'] = 'v19.serde.f0001.en02'
        q['split'] = 'quarantine'
        q['annotations']['v19']['review_status'] = 'quarantine'
        q['query'] = 'Another synthetic variant'
        self.assertIn('component_crosses_split', self.run_rows([self.native, q])['errors'])

    def test_relations_share_split_and_remain_one_independent_cluster(self):
        q = copy.deepcopy(self.native)
        q['id'] = 'v19.serde.f0003.en01'
        q['query_family'] = 'v19.serde.f0003'
        q['query'] = 'Distinct wording of related synthetic task'
        q['annotations']['v19']['global_family'] = self.native['query_family']
        p = self.root / 'relations.json'
        p.write_text(json.dumps({'components':[{'global_family':self.native['query_family'],
                                              'members':[self.native['query_family'],q['query_family']]}]}))
        r = self.run_rows([self.native, q], relations=p)
        self.assertEqual(r['errors'], {})
        self.assertEqual(r['global_family_components'], 1)
        self.assertEqual(r['native_family_ids'], 2)

    def test_native_no_answer_and_compat_incompatibility(self):
        self.native.update(no_answer=True, answers=[])
        self.native['annotations']['v19']['facets'] = []
        self.assertEqual(self.run_rows([self.native])['errors'], {})
        self.assertIn('compat_gold', self.run_rows([self.native], profile='compat')['errors'])

    def test_bad_spans_facets_paths_and_grades_rejected(self):
        a = self.native['answers'][0]['alternatives'][0]
        a['path'] = '../outside.rs'
        a['span'] = {'start':4, 'end':4}
        self.native['answers'][0]['grade'] = 1
        self.native['annotations']['v19']['facets'][0]['group_id'] = 'absent'
        errors = self.run_rows([self.native])['errors']
        for code in ['native_path','native_span','protocol_primary_grade','facet_group']:
            self.assertIn(code, errors)

    def test_projection_and_global_id_collisions_rejected(self):
        c = self.root / 'compat.jsonl'
        self.compat['query'] = 'Drifted synthetic query'
        c.write_text(json.dumps(self.compat) + '\n')
        self.assertIn('projection_identity_drift', self.run_rows([self.native], other=[('serde','compat',c)])['errors'])
        self.assertIn('duplicate_global_query_id', self.run_rows([self.native,self.native])['errors'])

    def test_fixed_split_vectors_do_not_depend_on_process_randomness(self):
        self.assertEqual([check.split('v19.serde.f%04d'%n) for n in range(1,5)], ['dev','holdout','dev','dev'])


if __name__ == '__main__':
    unittest.main()
