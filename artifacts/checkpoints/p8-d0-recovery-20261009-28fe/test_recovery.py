"""Controller controls: frozen official metadata/logs plus explicit synthetic ledger mutations."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
PKG = HERE if (HERE / 'recovery.py').is_file() else HERE.parent / 'candidate/artifacts/checkpoints/p8-d0-recovery-20261009-28fe'
spec = importlib.util.spec_from_file_location('recovery', PKG / 'recovery.py')
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)

class RecoveryControls(unittest.TestCase):
    def setUp(self):
        self.initial = json.loads((PKG / 'initial-identities.json').read_bytes())['cells']
        self.job = json.loads((PKG / 'predecessor-original-job.json').read_bytes())
        self.raw = (PKG / 'predecessor-original-job.log').read_bytes()
        self.wave = [{'scale':100000,'repetition':8,'ordinal':1,'predecessor_run_id':r.ORIGINAL_RUN,
                      'predecessor_run_attempt':1,'predecessor_job_id':113580044384}]
        self.ledger = [{'scale':x['scale'],'repetition':x['repetition'],'ordinal':0,
            'run_id':x['run_id'],'run_attempt':1,'job_id':x['job_id'],'controller_source':r.ORIGINAL_CONTROLLER,
            'state':'not_started','known_errors':[]} for x in self.initial]
        self.target = next(x for x in self.ledger if (x['scale'],x['repetition']) == (100000,8))
        self.target.update(r.interruption(self.job,self.raw,[],(100000,8)))
    def check(self):
        return r.check_ledger(self.initial,self.ledger,self.wave)
    def bad(self, pattern=None):
        if pattern:
            with self.assertRaisesRegex(ValueError,pattern): self.check()
        else:
            with self.assertRaises(ValueError): self.check()
    def test_real_original_shutdown_is_unknown_not_native_pass(self):
        result = self.check()
        self.assertEqual(result['initial_cells'],150)
        self.assertEqual(result['observed_attempt_records'],150)
        self.assertFalse(result['coverage_accepted'])
        self.assertIsNone(self.target['native_outcome'])
        self.assertEqual(self.target['root_cause'],'unknown')
    def test_missing_queued_initial_is_rejected(self):
        self.initial.pop(0); self.bad('150')
    def test_duplicate_initial_job_is_rejected(self):
        self.initial[1]['job_id']=self.initial[0]['job_id']; self.bad('duplicate')
    def test_initial_run_attempt_cannot_be_relabelled(self):
        self.target['run_attempt']=2; self.bad('original attempt')
    def test_initial_cell_pending_cannot_be_supplemented(self):
        self.target['state']='running'; self.bad('active')
    def test_successful_original_cannot_be_remeasured(self):
        self.target['state']='valid_complete'; self.bad('successful')
    def test_late_native_failure_revokes_previously_eligible_cell(self):
        self.assertFalse(self.check()['coverage_accepted'])
        self.target['known_errors']=['parity false in subsequently recovered raw']; self.bad('late error')
    def test_unclassified_original_failure_is_not_an_infra_shortcut(self):
        self.target['state']='terminal_unclassified'; self.bad('unclassified')
    def add_supplement(self,state='runner_interrupted_unobserved'):
        row={**self.target,'ordinal':1,'run_id':9001,'job_id':9002,'controller_source':'1'*40,'state':state}
        self.ledger.append(row)
        return row
    def wave2(self):
        self.wave[0].update(ordinal=2,predecessor_run_id=9001,predecessor_job_id=9002)
    def test_second_supplement_is_bounded_and_requires_prior_interruption(self):
        self.add_supplement();self.wave2()
        result=self.check();self.assertEqual(result['supplemental_records'],1)
        self.assertEqual(result['next_ordinal'],2)
        self.assertFalse(result['coverage_accepted'])
    def test_duplicate_wave_cannot_allocate_ordinal1_again(self):
        self.add_supplement(); self.bad('repeats')
    def test_success_then_second_supplement_is_rejected(self):
        self.add_supplement('valid_complete');self.wave2();self.bad('successful')
    def test_concurrent_supplement_cannot_start_a_second(self):
        self.add_supplement('running');self.wave2();self.bad('active')
    def test_renaming_controller_does_not_allow_ordinal_gap_or_reset(self):
        row=self.add_supplement();row['ordinal']=2;row['controller_source']='2'*40
        self.wave[0]['ordinal']=1;self.bad('gap')
    def test_third_supplement_exceeds_prospective_choice_not_original_todo_gate(self):
        row=self.add_supplement();row['ordinal']=3;self.bad('bound')
    def test_known_native_failure_then_shutdown_cannot_be_excused(self):
        raw=self.raw.replace(b'##[error]Process completed', b'{"status": "failed"}\n##[error]Process completed',1)
        with self.assertRaisesRegex(ValueError,'native/budget'): r.interruption(self.job,raw,[],(100000,8))
    def test_other_error_before_shutdown_rejects(self):
        with self.assertRaisesRegex(ValueError,'additional'):
            r.interruption(self.job,b'##[error]parity mismatch\n'+self.raw,[],(100000,8))
    def test_shutdown_without_explicit143_does_not_qualify(self):
        with self.assertRaises(ValueError): r.interruption(self.job,self.raw.replace(b'exit code 143.',b'exit code 3.'),[],(100000,8))
    def test_late_original_artifact_revokes_admission_even_if_name_claims_success(self):
        with self.assertRaisesRegex(ValueError,'revoked'):
            r.interruption(self.job,self.raw,[{'name':f'p8-scale-shard-100000-8-{r.ORIGINAL_RUN}'}],(100000,8))
    def test_cancelled_or_running_job_is_not_the_observed_original_failure(self):
        for status,conclusion in [('in_progress',None),('completed','cancelled'),('completed','success')]:
            with self.subTest(status=status,conclusion=conclusion):
                job=copy.deepcopy(self.job);job.update(status=status,conclusion=conclusion)
                with self.assertRaises(ValueError):r.interruption(job,self.raw,[],(100000,8))
    def test_failed_upload_cannot_be_silently_treated_as_unobserved_shutdown(self):
        job=copy.deepcopy(self.job)
        next(s for s in job['steps'] if s['name']==r.UPLOAD)['conclusion']='failure'
        with self.assertRaises(ValueError):r.interruption(job,self.raw,[],(100000,8))
    def test_original_run_all_identity_fields_are_checked(self):
        original=json.loads((PKG/'37854240827-run.json').read_bytes())
        r.validate_original_run(original)
        for field,value in [('id',1),('head_sha','0'*40),('run_attempt',2),('event','workflow_dispatch'),('head_branch','other'),('path','.github/workflows/other.yml')]:
            with self.subTest(field=field):
                changed=copy.deepcopy(original);changed[field]=value
                with self.assertRaises(ValueError):r.validate_original_run(changed)
        original['repository']['full_name']='other/repo'
        with self.assertRaises(ValueError):r.validate_original_run(original)
    def test_globally_same_job_id_cannot_be_laundered_under_another_run(self):
        row=self.add_supplement();row['job_id']=self.target['job_id'];self.wave2();self.bad('globally')
    def test_unknown_extra_original_job_rejects(self):
        pages=[json.loads((PKG/f'37854240827-jobs{n}.json').read_bytes()) for n in (1,2)]
        jobs=[j for p in pages for j in p['jobs']]
        jobs.append({**jobs[0],'id':999999,'name':'unregistered-extra'})
        with self.assertRaisesRegex(ValueError,'unknown'):r.original_job_map(jobs,r.initial_map(self.initial))
    def test_unknown_artifact_or_wrong_origin_rejects(self):
        def artifact(name,i):return {'id':i,'name':name,'workflow_run':{'id':r.ORIGINAL_RUN,'head_sha':r.ORIGINAL_CONTROLLER},'expired':False,'size_in_bytes':1,'digest':'sha256:'+'0'*64}
        artifacts=[artifact(f'p8-scale-build-{r.ORIGINAL_RUN}',1),artifact(f'p8-d0-study-admission-{r.ORIGINAL_RUN}',2)]
        self.assertEqual(len(r.validate_artifacts(artifacts)),2)
        for extra in [artifact('unknown-extra',3),artifact(f'p8-scale-shard-100000-99-{r.ORIGINAL_RUN}',3)]:
            with self.assertRaises(ValueError):r.validate_artifacts(artifacts+[extra])
        artifacts[0]['workflow_run']['id']=1
        with self.assertRaisesRegex(ValueError,'source/run'):r.validate_artifacts(artifacts)
    def test_nonfinite_json_numbers_reject(self):
        for number in ['NaN','Infinity','-Infinity','1e999']:
            with self.subTest(number=number),self.assertRaises(ValueError):r.strict_json('{"n":'+number+'}')
    def test_duplicate_json_keys_reject(self):
        with self.assertRaises(ValueError):r.strict_json('{"ordinal":1,"ordinal":2}')
    def test_official_page_missing_or_changed_identity_rejects(self):
        pages=[json.loads((PKG/f'37854240827-jobs{n}.json').read_bytes()) for n in (1,2)]
        jobs=[j for p in pages for j in p['jobs']]
        self.assertEqual(len(r.original_job_map(jobs,r.initial_map(self.initial))),150)
        target=next(j for j in jobs if j['id']==113580044384);target['run_attempt']=2
        with self.assertRaises(ValueError):r.original_job_map(jobs,r.initial_map(self.initial))
    def test_wrong_predecessor_prevents_cross_cell_substitution(self):
        self.wave[0]['predecessor_job_id']=1;self.bad('identity')
    def test_multi_cell_wave_does_not_expand_first_wave_to_whole_matrix(self):
        self.wave.append(dict(self.wave[0]));self.bad('one explicitly')
    def test_token_cannot_be_sent_to_another_host_or_repository(self):
        with tempfile.TemporaryDirectory() as d:
            api=r.API('not-a-real-token',Path(d))
            with mock.patch.object(api.opener,'open') as opened:
                for url in ['https://evil.test/repos/jyqj/codecortex/actions/runs/1','https://api.github.com/repos/other/repo/actions/runs/1']:
                    with self.assertRaises(ValueError):api._get(url,True,100)
                opened.assert_not_called()
    def test_signed_log_redirect_gets_no_auth_header(self):
        with tempfile.TemporaryDirectory() as d:
            api=r.API('not-a-real-token',Path(d))
            response=mock.MagicMock();response.__enter__.return_value.read.return_value=b'log'
            with mock.patch.object(api.opener,'open',return_value=response) as opened:
                self.assertEqual(api._get('https://logs.blob.core.windows.net/path?sig=example',False,100),b'log')
                request=opened.call_args.args[0]
                self.assertIsNone(request.get_header('Authorization'))
    def test_oversized_or_duplicate_api_pages_cannot_be_admitted(self):
        with tempfile.TemporaryDirectory() as d:
            api=r.API('not-a-real-token',Path(d))
            with mock.patch.object(api,'get',return_value={'total_count':2,'jobs':[{'id':1},{'id':1}]}):
                with self.assertRaises(ValueError):api.pages('/actions/runs/1/jobs','jobs')
    def test_changed_total_between_api_pages_is_not_partial_success(self):
        with tempfile.TemporaryDirectory() as d:
            api=r.API('not-a-real-token',Path(d))
            pages=[{'total_count':101,'jobs':[{'id':i}for i in range(100)]},{'total_count':102,'jobs':[{'id':100}]}]
            with mock.patch.object(api,'get',side_effect=pages):
                with self.assertRaisesRegex(ValueError,'changed'):api.pages('/actions/runs/1/jobs','jobs')

if __name__=='__main__':unittest.main()
