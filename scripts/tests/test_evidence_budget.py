import copy
import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
spec=importlib.util.spec_from_file_location('budget',Path(__file__).parents[1]/'evidence_budget.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
SOURCE={'x.py':'first\nsecond\nthird\n'}
def task():
    return {'id':'q','repo':'r','result_pointer':'','labels':[{'id':'a','file_path':'x.py','start_line':1,'end_line':2}], 'required_groups':[['a']]}
def hit(start,end,text):
    return {'file_path':'x.py','start_line':start,'end_line':end,'text':text}
def sample(hits):
    return {'output':hits,'error':None,'mode':'warm_cache','iteration':0}
class EvidenceBudgetTests(unittest.TestCase):
    def test_split_evidence_union_equals_one_complete_chunk(self):
        t=task()
        one=m.covered_labels(t,SOURCE,[hit(1,2,'first\nsecond')])
        split=m.covered_labels(t,SOURCE,[hit(1,1,'first'),hit(2,2,'second')])
        self.assertEqual(one,{'a'});self.assertEqual(one,split)
    def test_source_text_not_claimed_span_is_authoritative(self):
        for h in [hit(1,2,'first'),hit(1,2,'second\nfirst'),hit(1,2,'invented first second')]:
            self.assertEqual(m.covered_labels(task(),SOURCE,[h]),set())
    def test_duplicate_cost_is_charged_without_extra_coverage(self):
        h=hit(1,2,'first\nsecond')
        a=m.score(task(),SOURCE,sample([h]),len,[10000])
        b=m.score(task(),SOURCE,sample([h,h]),len,[10000])
        self.assertEqual(a['region_coverage'],b['region_coverage'])
        self.assertGreater(b['full_handler_tokens'],a['full_handler_tokens'])
        self.assertGreater(b['budgets']['10000']['adapter_prefix']['tokens'],a['budgets']['10000']['adapter_prefix']['tokens'])
    def test_and_of_or_groups(self):
        t=task();t['labels'].append({'id':'b','file_path':'x.py','start_line':3,'end_line':3})
        t['required_groups']=[['a','b']]
        self.assertTrue(m.evidence_metrics(t,{'b'})['sufficient'])
        t['required_groups']=[['a'],['b']]
        self.assertFalse(m.evidence_metrics(t,{'b'})['sufficient'])
    def test_budget_stops_prefix_and_does_not_skip_bad_items(self):
        h=hit(1,2,'first\nsecond');bad={'text':'noise'*1000}
        row=m.score(task(),SOURCE,sample([bad,h]),len,[200])
        self.assertFalse(row['budgets']['200']['adapter_prefix']['sufficient'])
    def test_tiny_budget_and_tool_errors_have_zero_coverage(self):
        row=m.score(task(),SOURCE,dict(sample([]),output=None,error='failed'),len,[1])
        self.assertTrue(row['error']);self.assertFalse(row['budgets']['1']['adapter_prefix']['frame_fits'])
        self.assertEqual(row['region_coverage'],0)
    def test_degraded_empty_search_not_correct_abstention(self):
        t=task();t.update(no_answer=True,labels=[],required_groups=[],result_pointer='/hits')
        output={'hits':[],'evidence_summary':{'retrieval':{'lanes':{'grep':{'work_limited':True}}}}}
        self.assertFalse(m.score(t,SOURCE,sample(output),len,[10000])['correct_empty_response'])
    def test_stage_locators_not_confused_with_visible_evidence(self):
        t=task();t['result_pointer']='/hits'
        output={'hits':[],'evidence_summary':{'retrieval':{'stages':{'candidate_union':[hit(1,2,'')]}}}}
        row=m.score(t,SOURCE,sample(output),len,[10000])
        self.assertEqual(row['stage_locator_recall']['candidate_union'],1)
        self.assertEqual(row['region_coverage'],0)
    def test_locator_tasks_do_not_enter_source_budget_denominator(self):
        t=task();t['evidence_mode']='locator'
        manifest={'schema_version':1,'purpose':'regression','dataset_id':'fixture','repositories':[{'id':'r','revision':'authored','files':SOURCE}],'tasks':[t]}
        header={'manifest':manifest,'manifest_git_blob':'fixture','implementation_commit':'fixture','effective_config':{},'provenance':{}}
        row=dict(sample([{'file_path':'x.py','start_line':1,'end_line':2}]),task_id='q')
        report=m.report(header,[row],len,[1000])
        self.assertEqual(report['source_evidence_observations'],0)
        self.assertEqual(report['locator_positive_observations'],1)
        self.assertIsNone(report['summaries']['1000']['adapter_sufficiency_repo_macro'])
        t['evidence_mode']='source'
        report=m.report(header,[row],len,[1000])
        self.assertEqual(report['source_evidence_observations'],1)
        self.assertEqual(report['summaries']['1000']['adapter_sufficiency_repo_macro'],0)

    def test_bad_grid_rejected(self):
        t=task();t['no_answer']=False
        manifest={'schema_version':1,'purpose':'regression','repositories':[{'id':'r','revision':'authored','files':SOURCE}],'tasks':[t]}
        header={'schema_version':1,'repetitions':1,'manifest':manifest}
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'raw';p.write_text(json.dumps({'kind':'header','data':header})+'\n')
            with self.assertRaises(ValueError):m.read_raw(p)
    def test_real_tokenizer_cache_is_verified_before_scoring(self):
        with tempfile.TemporaryDirectory() as d:
            # Imported lazily in production; test works without optional package.
            try:
                m.tokenizer(Path(d))
            except (ValueError,importlib.metadata.PackageNotFoundError): pass
            else: self.fail('missing cache must fail closed')
if __name__=='__main__':unittest.main()
