import copy
import importlib.util
import json
from pathlib import Path
import unittest
import unittest.mock

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('experiment', HERE / 'experiment.py')
e = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e)


class Controls(unittest.TestCase):
    def setUp(self):
        self.rows = [json.loads(line) for line in (HERE / 'original-gold.jsonl').read_text().splitlines()]
        e.split(self.rows)

    def test_family_leak_rejected(self):
        self.rows[0]['split'] = 'holdout' if self.rows[0]['split'] == 'dev' else 'dev'
        with self.assertRaisesRegex(ValueError, 'family'):
            e.validate_partition(self.rows)

    def test_gold_module_leak_rejected(self):
        dev = next(r for r in self.rows if r['split'] == 'dev')
        held = next(r for r in self.rows if r['split'] == 'holdout')
        held['answers'] = copy.deepcopy(dev['answers'])
        with self.assertRaisesRegex(ValueError, 'module'):
            e.validate_partition(self.rows)

    def test_quota_cannot_drop_failed_case(self):
        with self.assertRaisesRegex(ValueError, 'quota'):
            e.validate_partition(self.rows[:-1])

    def test_native_gold_preserved(self):
        original = [json.loads(l) for l in (HERE / 'original-gold.jsonl').read_text().splitlines()]
        authored = [json.loads(l) for l in (HERE / 'queries/native.jsonl').read_text().splitlines()]
        for a, b in zip(original, authored):
            a['split'] = b['split']
            self.assertEqual(a, b)

    def test_single_factor_controls_and_same_admitted_corpus(self):
        baseline = json.loads((HERE / 'suites/native-baseline.json').read_text())
        for name, section, knob in [('graph_vote_off', 'search', 'graph_weight'),
                                    ('graph_rerank_off', 'ranking', 'graph_rerank_weight')]:
            arm = json.loads((HERE / f'suites/native-{name}.json').read_text())
            self.assertEqual(arm['source'], baseline['source'])
            self.assertEqual(arm['queries_digest'], baseline['queries_digest'])
            config = copy.deepcopy(arm['engine_config'])
            self.assertEqual(config.pop(section), {knob: 0.0})
            self.assertEqual(config, baseline['engine_config'])

    def test_bootstrap_does_not_count_repetitions_as_queries(self):
        values = [0.1, 0.4, 0.9]
        a = e.interval(values)
        self.assertEqual(a, e.interval(values))
        self.assertEqual(a['family_n'], 3)
        self.assertFalse(a['conclusive'])

    def test_lock_drift_rejected(self):
        e.verify()
        with unittest.mock.patch.object(e, 'digest', return_value='corrupt'):
            with self.assertRaisesRegex(ValueError, 'drift'):
                e.verify()


if __name__ == '__main__':
    unittest.main()
