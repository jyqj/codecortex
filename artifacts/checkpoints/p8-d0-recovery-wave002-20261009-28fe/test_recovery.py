"""Fixed real predecessor proof plus explicit synthetic current-wave API controls."""
import base64
import copy
import json
import os
from pathlib import Path
import tempfile
import types
import unittest
from unittest import mock

HERE = Path(__file__).resolve().parent
PKG = HERE if (HERE / 'recovery.py').is_file() else HERE / 'candidate/artifacts/checkpoints/p8-d0-recovery-wave002-20261009-28fe'
r = types.ModuleType('wave002_under_test'); r.__file__ = str(PKG / 'recovery.py')
exec(compile((PKG / 'recovery.py').read_bytes(), r.__file__, 'exec'), r.__dict__)

class FixedCapacityControls(unittest.TestCase):
    def setUp(self):
        self.reg = r.strict_json((PKG / 'registration.json').read_bytes())
        self.policy = self.reg['predecessor_admission']
        self.initial = r.strict_json((PKG / 'initial-identities.json').read_bytes())['cells']
        self.prior = r.strict_json((PKG / 'prior-receiver-receipt.json').read_bytes())
        self.row = copy.deepcopy(next(row for row in self.prior['attempts'] if row['job_id'] == r.WAVE001_JOB))
        self.job = r.strict_json((PKG / 'prior-wave001-jobs.json').read_bytes())['jobs'][0]
        self.log = (PKG / 'prior-wave001-job.log').read_bytes()
        self.artifacts = r.strict_json((PKG / 'prior-wave001-artifacts.json').read_bytes())['artifacts']
    def proof(self):
        return r.verify_capacity_failure(self.policy, self.row, self.job, self.log, self.artifacts)
    def ledger(self):
        rows = copy.deepcopy(self.prior['attempts'])
        rows[-1] = self.proof()
        assert rows[-1]['job_id'] == r.WAVE001_JOB
        return rows
    def test_real_original_capacity_proof_preserves_invalid_and_every_error(self):
        result = self.proof()
        self.assertEqual(result['state'], 'invalid')
        self.assertEqual(result['known_errors'], self.row['known_errors'])
        self.assertIsNone(result['native_outcome'])
        self.assertFalse(result['native_measurement_started'])
        self.assertFalse(result['selected_for_coverage'])
        self.assertTrue(result['control_failure_currently_verified'])
        self.assertNotIn('control_failure', self.row)
    def test_151_prior_attempts_admit_only_the_registered_final_ordinal(self):
        result = r.check_ledger(self.initial, self.ledger(), r.FIXED_WAVE, self.policy)
        self.assertEqual((result['observed_attempt_records'], result['supplemental_records'], result['next_ordinal']), (151, 1, 2))
        self.assertFalse(result['coverage_accepted'])
    def test_known_native_error_is_never_excused_by_capacity_classification(self):
        self.row['known_errors'].append('known native parity failure')
        with self.assertRaises(ValueError): self.proof()
    def test_preserved_error_cannot_be_deleted_or_rewritten_to_clean_unknown(self):
        for change in ({'known_errors': []}, {'state': 'runner_interrupted_unobserved'}, {'native_outcome': 'passed'}):
            with self.subTest(change=change):
                original=copy.deepcopy(self.row); self.row.update(change)
                with self.assertRaises(ValueError): self.proof()
                self.row=original
    def test_late_log_evidence_revokes_even_with_unchanged_archive(self):
        self.log += b'\n##[error]native parity failure\n'
        with self.assertRaisesRegex(ValueError, 'late wave001 log'): self.proof()
    def test_execute_step_or_status_change_is_rejected(self):
        self.job['steps'][8]['conclusion']='success'
        with self.assertRaisesRegex(ValueError, 'official job'): self.proof()
    def test_wrong_job_controller_or_attempt_cannot_substitute(self):
        for key, value in (('id', 1), ('run_attempt', 2), ('head_sha', 'f'*40), ('conclusion', 'success')):
            with self.subTest(key=key):
                old=self.job[key];self.job[key]=value
                with self.assertRaises(ValueError): self.proof()
                self.job[key]=old
    def test_new_or_missing_artifact_revokes_qualification(self):
        for items in ([], self.artifacts+[copy.deepcopy(self.artifacts[0])]):
            with self.subTest(count=len(items)), self.assertRaises(ValueError):
                r.verify_capacity_failure(self.policy,self.row,self.job,self.log,items)
    def test_current_artifact_digest_or_availability_cannot_drift(self):
        for key, value in (('digest','sha256:'+'0'*64), ('expired',True), ('id',999)):
            with self.subTest(key=key):
                old=self.artifacts[0][key];self.artifacts[0][key]=value
                with self.assertRaises(ValueError): self.proof()
                self.artifacts[0][key]=old
    def test_embedded_original_zip_byte_change_is_rejected_before_parsing(self):
        with tempfile.TemporaryDirectory() as td:
            raw=bytearray(base64.b64decode((PKG/'prior-wave001.zip.base64').read_bytes()))
            raw[30]^=1
            (Path(td)/'prior-wave001.zip.base64').write_bytes(base64.b64encode(raw))
            with mock.patch.object(r,'HERE',Path(td)),self.assertRaisesRegex(ValueError,'ZIP bytes'):
                self.proof()
    def test_policy_cannot_discount_space_change_source_or_expand_ordinal(self):
        for key, value in (('required_free_bytes',1),('capacity_exit_code',143),('next_ordinal',3)):
            with self.subTest(key=key):
                policy=copy.deepcopy(self.policy);policy[key]=value
                with self.assertRaises(ValueError):r.verify_capacity_failure(policy,self.row,self.job,self.log,self.artifacts)
    def test_prior_late_revocation_is_not_reset(self):
        self.row['late_evidence_revoked']=True
        with self.assertRaises(ValueError):self.proof()
    def test_missing_initial_queued_job_cannot_shrink_denominator(self):
        with self.assertRaises(ValueError):r.check_ledger(self.initial[1:],self.ledger(),r.FIXED_WAVE,self.policy)
    def test_original_success_pending_or_known_failure_never_gets_replaced(self):
        for change in ({'state':'valid_complete'},{'state':'running'},{'known_errors':['known original parity failure']},{'late_evidence_revoked':True}):
            with self.subTest(change=change):
                rows=self.ledger();next(x for x in rows if x['job_id']==113580044384).update(change)
                with self.assertRaises(ValueError):r.check_ledger(self.initial,rows,r.FIXED_WAVE,self.policy)
    def test_old_ordinal1_requires_its_original143_predecessor(self):
        rows=self.ledger();next(x for x in rows if x['job_id']==113580044384)['state']='terminal_unclassified'
        with self.assertRaisesRegex(ValueError,'known failed or completed predecessor'):r.check_ledger(self.initial,rows,r.FIXED_WAVE,self.policy)
    def test_no_third_attempt_renaming_or_duplicate_global_job(self):
        for change in ({'ordinal':3},{'ordinal':2},{'job_id':113580044384},{'run_attempt':2}):
            with self.subTest(change=change):
                rows=self.ledger();rows[-1].update(change)
                with self.assertRaises(ValueError):r.check_ledger(self.initial,rows,r.FIXED_WAVE,self.policy)
    def test_current_proof_is_required_not_a_historical_annotation(self):
        rows=self.ledger();rows[-1]['control_failure_currently_verified']=False
        with self.assertRaises(ValueError):r.check_ledger(self.initial,rows,r.FIXED_WAVE,self.policy)
    def test_original_shutdown_late_raw_still_revokes(self):
        oldjob=r.strict_json((PKG/'predecessor-original-job.json').read_bytes())
        oldlog=(PKG/'predecessor-original-job.log').read_bytes()
        with self.assertRaisesRegex(ValueError,'revoked'):
            r.interruption(oldjob,oldlog,[{'name':f'p8-scale-shard-100000-8-{r.ORIGINAL_RUN}'}],(100000,8))
    def test_registration_binds_prior_failed_receiver_and_unreconciled_capture(self):
        prior=r.read_prior_receiver(self.reg,self.initial)
        self.assertEqual(len(prior['attempts']),151)
        self.assertEqual(prior['partial_capture_history']['unreconciled_capture_count'],1)
        changed=copy.deepcopy(self.reg);changed['prior_capture_receipts']=[]
        with self.assertRaises(ValueError):r.read_prior_receiver(changed,self.initial)
    def test_actual_package_hashes_workflow_all_inputs_and_original_budget(self):
        with mock.patch.dict(os.environ,{'P8_RECOVERY_REGISTRATION_SHA256':r.sha((PKG/'registration.json').read_bytes())}):
            reg,initial,old=r.load_package(PKG.parents[2])
            self.assertEqual(len(initial),150)
            self.assertEqual(reg['complete_coverage_required_samples'],1500)
            self.assertEqual(old['source'],r.SOURCE)
            self.assertEqual(old['build']['artifact_id'],11582571291)
            with self.assertRaisesRegex(ValueError,'fixed checkout path'):r.load_package(PKG.parents[1])

class SyntheticCurrentAPIControls(unittest.TestCase):
    """Only this adapter fabricates the as-yet unlaunched wave002 run/job metadata."""
    setUp = FixedCapacityControls.setUp
    def fixture(self):
        old = r.strict_json((PKG/'original-registration.json').read_bytes())
        wave1 = r.strict_json((PKG/'prior-wave001-run.json').read_bytes())
        current = copy.deepcopy(wave1)
        current.update(id=990000001,head_sha='a'*40,head_branch=r.BRANCH,status='in_progress',conclusion=None)
        ownjob=copy.deepcopy(self.job)
        ownjob.update(id=990000002,run_id=current['id'],head_sha=current['head_sha'],name='D0 100000 repetition 8 supplemental ordinal 2',status='in_progress',conclusion=None)
        original_run=r.strict_json((PKG/'37854240827-run.json').read_bytes())
        original_jobs=[j for n in (1,2) for j in r.strict_json((PKG/f'current-original-jobs{n}.json').read_bytes())['jobs']]
        original_artifacts=r.strict_json((PKG/'current-original-artifacts.json').read_bytes())['artifacts']
        build_inventory=r.strict_json((PKG/'fixed-build-artifacts.json').read_bytes())['artifacts']
        mapping={
            '/actions/runs?branch='+r.urllib.parse.quote(r.WAVE001_BRANCH,safe=''):{'workflow_runs':[wave1]},
            '/actions/runs?branch='+r.urllib.parse.quote(r.BRANCH,safe=''):{'workflow_runs':[current]},
            f'/actions/runs/{current["id"]}':current,
            f'/actions/runs/{current["id"]}/attempts/1/jobs':{'jobs':[ownjob]},
            f'/actions/runs/{r.ORIGINAL_RUN}':original_run,
            f'/actions/runs/{r.ORIGINAL_RUN}/attempts/1/jobs':{'jobs':original_jobs},
            f'/actions/runs/{r.ORIGINAL_RUN}/artifacts':{'artifacts':original_artifacts},
            '/actions/jobs/113580044384/logs':(PKG/'predecessor-original-job.log').read_bytes(),
            f'/actions/runs/{r.WAVE001_RUN}':wave1,
            f'/actions/runs/{r.WAVE001_RUN}/attempts/1/jobs':{'jobs':[self.job]},
            f'/actions/runs/{r.WAVE001_RUN}/artifacts':{'artifacts':self.artifacts},
            f'/actions/jobs/{r.WAVE001_JOB}/logs':self.log,
            f'/actions/runs/{r.BUILD_RUN}':{'id':r.BUILD_RUN,'head_sha':r.SOURCE,'run_attempt':1,'status':'completed','conclusion':'success'},
            f'/actions/runs/{r.BUILD_RUN}/artifacts':{'artifacts':build_inventory}}
        class Adapter:
            pages=r.API.pages
            def __init__(self):self.requests=[]
            def get(self,suffix,maximum=r.MAX_JSON,log=False):
                self.requests.append(suffix)
                if suffix in mapping:return copy.deepcopy(mapping[suffix])
                split='&per_page=100&page=' if '&per_page=100&page=' in suffix else '?per_page=100&page='
                root,page=suffix.rsplit(split,1);value=copy.deepcopy(mapping[root]);field=next(iter(value));rows=value[field]
                return {'total_count':len(rows),field:rows[(int(page)-1)*100:int(page)*100]}
        return old,mapping,Adapter(),current
    def observe(self, change=None):
        old,mapping,api,current=self.fixture()
        if change:change(mapping,current)
        env={'GITHUB_REPOSITORY':r.REPO,'GITHUB_REF_NAME':r.BRANCH,'GITHUB_EVENT_NAME':'push',
             'GITHUB_RUN_ATTEMPT':'1','GITHUB_RUN_ID':str(current['id']),'GITHUB_SHA':current['head_sha']}
        with mock.patch.dict(os.environ,env):value=r.observe(self.reg,self.initial,old,api)
        return value,api
    def test_complete_prior_API_and_synthetic_current_wave_keep_150_151_152_separate(self):
        value,api=self.observe()
        self.assertEqual(len(value['all_initial_attempts']),150)
        self.assertEqual(len(value['all_prior_attempts']),151)
        self.assertEqual(value['observed_attempt_records_including_current_wave'],152)
        self.assertEqual(value['predecessor']['state'],'invalid')
        self.assertEqual(value['predecessor']['known_errors'],self.row['known_errors'])
        self.assertIsNone(value['current_wave_attempt']['native_measurement_started'])
        self.assertEqual(value['original_predecessor']['observed_job_exit'],143)
        self.assertFalse(value['partial_reconciliation_performed'])
        self.assertTrue(any('page=2' in suffix for suffix in api.requests))
    def test_duplicate_wave_run_and_old_rerun_block_before_execution(self):
        def extra(mapping,current):
            key='/actions/runs?branch='+r.urllib.parse.quote(r.BRANCH,safe='')
            mapping[key]['workflow_runs'].append(dict(current,id=990000003))
        with self.assertRaisesRegex(ValueError,'extra or missing registered workflow run'):self.observe(extra)
        def rerun(mapping,current):mapping[f'/actions/runs/{r.WAVE001_RUN}']['run_attempt']=2
        with self.assertRaisesRegex(ValueError,'run identity/rerun'):self.observe(rerun)
    def test_current_job_duplicate_or_wrong_source_rejects(self):
        def change(mapping,current):mapping[f'/actions/runs/{current["id"]}/attempts/1/jobs']['jobs'][0]['head_sha']='b'*40
        with self.assertRaisesRegex(ValueError,'current wave job identity'):self.observe(change)
        def duplicate(mapping,current):mapping[f'/actions/runs/{current["id"]}/attempts/1/jobs']['jobs'][0]['id']=r.WAVE001_JOB
        with self.assertRaisesRegex(ValueError,'current wave job identity'):self.observe(duplicate)
    def test_prior_original_artifact_is_not_silently_forgotten(self):
        def change(mapping,current):
            rows=mapping[f'/actions/runs/{r.ORIGINAL_RUN}/artifacts']['artifacts']
            retained={x['id'] for x in self.prior['artifact_identities'] if x['run_id']==r.ORIGINAL_RUN}
            remove=next(x for x in rows if x['id'] in retained and 'capacity' in x['name'])
            rows.remove(remove)
        with self.assertRaisesRegex(ValueError,'previously observed original artifact'):self.observe(change)
    def test_previously_running_original_cannot_regress_to_queued(self):
        def change(mapping,current):
            running=next(x['job_id'] for x in self.prior['attempts'] if x['ordinal']==0 and x['official_status']=='in_progress')
            next(x for x in mapping[f'/actions/runs/{r.ORIGINAL_RUN}/attempts/1/jobs']['jobs'] if x['id']==running)['status']='queued'
        with self.assertRaisesRegex(ValueError,'previously running original job'):self.observe(change)
    def test_latest_original_log_and_new_raw_revoke_before_launch(self):
        def log_change(mapping,current):mapping['/actions/jobs/113580044384/logs']+=b'\nnew failure'
        with self.assertRaisesRegex(ValueError,'late original rep8 log'):self.observe(log_change)
        def raw_change(mapping,current):
            rows=mapping[f'/actions/runs/{r.ORIGINAL_RUN}/artifacts']['artifacts'];added=copy.deepcopy(rows[0])
            added.update(id=990000004,name=f'p8-scale-shard-100000-8-{r.ORIGINAL_RUN}');rows.append(added)
        with self.assertRaisesRegex(ValueError,'supplementation revoked'):self.observe(raw_change)

if __name__=='__main__':unittest.main()
