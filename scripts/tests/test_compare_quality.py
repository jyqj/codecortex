import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('compare_quality', Path(__file__).parents[1] / 'compare_quality.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def report(score=0.5):
    return {'schema_version': 1, 'manifest_git_blob': 'same', 'dataset_id': 'fixture',
            'purpose': 'regression', 'implementation_commit': 'sha', 'passed': True,
            'effective_config': {}, 'observations': [
                {'repo': 'r', 'task_id': 'a', 'no_answer': False,
                 'metrics': {metric: score for metric in module.METRICS}}]}


class ComparisonTests(unittest.TestCase):
    def test_different_manifest_fails(self):
        a, b = report(), report()
        b['manifest_git_blob'] = 'different'
        with self.assertRaises(ValueError): module.compare(a, b)

    def test_missing_query_fails_instead_of_using_intersection(self):
        a, b = report(), report()
        b['observations'] = []
        with self.assertRaises(ValueError): module.compare(a, b)

    def test_fixture_has_no_inferential_ci(self):
        result = module.compare(report(0), report(1))
        self.assertEqual(result['metrics']['recall_at_5']['repo_macro_delta'], 1)
        self.assertIsNone(result['metrics']['recall_at_5']['repo_cluster_bootstrap_95pct'])

    def test_more_iterations_do_not_create_more_repositories(self):
        a, b = report(0), report(1)
        a['purpose'] = b['purpose'] = 'held_out'
        a['observations'] *= 100
        b['observations'] *= 100
        self.assertIsNone(module.compare(a, b)['metrics']['recall_at_5']['repo_cluster_bootstrap_95pct'])

    def test_failure_zeros_remain_in_denominator(self):
        a, b = report(0), report(1)
        b['observations'].append(copy.deepcopy(a['observations'][0]))
        self.assertEqual(module.compare(a, b)['metrics']['recall_at_5']['repo_macro_delta'], 0.5)

    def test_nonfinite_metrics_fail(self):
        with self.assertRaises(ValueError): module.compare(report(), report(float('nan')))

    def test_metric_policy_mismatch_fails(self):
        a, b = report(), report()
        b['ndcg_policy'] = 'different'
        with self.assertRaises(ValueError): module.compare(a, b)


if __name__ == '__main__':
    unittest.main()
