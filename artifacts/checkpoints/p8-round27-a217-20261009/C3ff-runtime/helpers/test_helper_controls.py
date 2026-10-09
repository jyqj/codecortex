"""Explicit synthetic auditor controls only; no native acceptance or hour run."""
import copy, hashlib, json, pathlib, sys, tempfile, unittest, zipfile
from unittest import mock
HERE=pathlib.Path(__file__).parent
sys.dont_write_bytecode=True
sys.path[:0]=[str(HERE),'/dev/shm/a217aaae3bde/codecortex-combined-guard-view/scripts/tests','/dev/shm/a217aaae3bde/codecortex-combined-guard-view/scripts']
import p8_runtime as runtime
from test_p8_runtime_cache import ProtocolProduct, ACTIONS
import e_raw_cache_audit as audit
import preflight_e_runtime_zip as preflight

class AuditorControls(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory(prefix='combined-E-helper-synthetic-',dir=HERE)
        cls.m=pathlib.Path(cls.temp.name);(cls.m/'project').mkdir();(cls.m/'product').mkdir()
        (cls.m/'project/.p8-owned').write_text(runtime.MARKER)
        (cls.m/'project/stable.py').write_text(f'def {runtime.QUERY}():\n    return 7\n')
        product=ProtocolProduct(cls.m/'project');tool=product.tool
        journal=[];qid=0
        def rpc(name,args,value):
            nonlocal qid
            qid+=1
            journal.append({'event':'request','payload':{'id':qid,'method':'tools/call','params':{'name':name,'arguments':copy.deepcopy(args)}}})
            journal.append({'event':'stdout_wire','text':json.dumps({'id':qid,'result':{'structuredContent':{'result':copy.deepcopy(value)}}})})
        def wrapped(name,args,timeout=45):
            v=tool(name,args,timeout);rpc(name,args,v);return v
        product.tool=wrapped
        rpc('index',{'path':'/synthetic-owned','full':True},{'resolution_freshness':{'index_epoch':5}})
        rows=[];previous=None;mutation=None
        for number in range(3601):
            at=number*1_000_000_000
            if number%3==0:
                ordinal=number//3;action=ACTIONS[ordinal%6]
                if action!='restore_api':product.epoch+=1
                mutation=dict(action=action,ordinal=ordinal,operation_id=number,index_epoch=product.epoch)
                row=dict(kind='operation',id=number,operation='build',status='success',offered_ns=at,call_started_ns=at+1,finished_ns=at+100,mutation_ordinal=ordinal,mutation=dict(action=action),response=dict(resolution_freshness=dict(index_epoch=product.epoch)))
                rpc('index',{'path':'/synthetic-owned','full':False},row['response'])
            else:
                row=dict(kind='operation',id=number,operation='read',status='error',offered_ns=at,call_started_ns=at+1)
                clock=iter(range(at+2,at+200)).__next__
                previous=runtime.soak_read(product,cls.m/'project',row,previous,mutation,clock)
                row.update(status='success',finished_ns=clock())
            rows.append(row)
        product.tool('status',{'aspect':'index'})
        endpoint=product.tool('search',{'query':runtime.QUERY,'mode':'symbol','top_k':5})
        cls.report=dict(status='passed_observation',exit_code=0,observed_work_ns=3600_000_000_000,actual_concurrency=dict(maximum=1,read_build_overlap=False),resource_time_coverage=dict(work_start_ns=0,work_end_ns=3600_000_100_000))
        cls.report['cache_reuse']=runtime.soak_cache_summary(rows,3601,0,3600_000_100_000,cls.m/'project')
        assert cls.report['cache_reuse']['passed']
        rows.append(dict(kind='endpoint_public',incremental=endpoint,full=endpoint))
        with (cls.m/'raw.jsonl').open('w') as f:
            for row in rows:f.write(json.dumps(row,sort_keys=True)+'\n')
        cls.journal=journal
        cls.write_journal(journal)
        cls.raw,cls.offsets,_=audit.load_runtime_rows(cls.m/'raw.jsonl')
        cls.plan=dict(profile='soak',operations=3601,request_timeout_seconds=60,queue_capacity=128,resource_interval_seconds=1,read_protocol=dict(name=runtime.SOAK_READ_PROTOCOL,offered_read_operations=2400,planned_request_counts=dict.fromkeys(runtime.SOAK_READ_ROLES,2400),request_roles=list(runtime.SOAK_READ_ROLES),strategy='local',rpc_timeout_seconds=dict(before_status=30,symbol=60,hybrid=60,after_status=30),rpc_timeout_sum_seconds=180))
    @classmethod
    def write_journal(cls,events):
        with (cls.m/'product/rpc.jsonl').open('w') as f:
            for event in events:f.write(json.dumps(event,sort_keys=True)+'\n')
    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()
    def tearDown(self):self.write_journal(self.journal)
    def run_audit(self,report=None):return audit.audit_cache(runtime,self.m,self.plan,report or self.report,self.raw,self.offsets)
    def test_complete_3601_synthetic_denominator_and_wire_match(self):
        result=self.run_audit();self.assertEqual(result['validated_reads'],2400);self.assertEqual(result['protocol_rpc_count'],9600)
        self.assertTrue(result['original_search_wire_order_and_full_responses_match'])
    def test_changed_original_wire_response_cannot_borrow_probe_receipt(self):
        events=copy.deepcopy(self.journal)
        for i,event in enumerate(events):
            if event['event']=='request' and event['payload']['params']['name']=='search':
                response=json.loads(events[i+1]['text']);response['result']['structuredContent']['result']={'changed':'synthetic negative control'};events[i+1]['text']=json.dumps(response);break
        self.write_journal(events)
        with self.assertRaises(AssertionError):self.run_audit()
    def test_missing_original_wire_response_rejected(self):
        self.write_journal(self.journal[:-1])
        with self.assertRaises(AssertionError):self.run_audit()
    def test_changed_report_quarters_rejected(self):
        r=copy.deepcopy(self.report);r['cache_reuse']['time_quarters'][0]['hits']+=1
        with self.assertRaises(AssertionError):self.run_audit(r)
    def test_changed_report_denominator_rejected(self):
        r=copy.deepcopy(self.report);r['cache_reuse']['request_counts']['hybrid']['success']-=1
        with self.assertRaises(AssertionError):self.run_audit(r)

class CleanupAndZipControls(unittest.TestCase):
    def test_actual_e95_cleanup_shape_and_each_unknown_writer_rejected(self):
        report=dict(artifact_seal_status='sealed',artifact_seal='seal.json',owned_cleanup=dict(unfinished_work=0,sampler_stopped=True,product_stopped=True,comparison_product_stopped=True,product_construction_pending=False,comparison_product_construction_pending=False))
        audit.confirmed_cleanup(report)
        for key in report['owned_cleanup']:
            changed=copy.deepcopy(report);changed['owned_cleanup'][key]=1 if key=='unfinished_work' else not changed['owned_cleanup'][key]
            with self.subTest(key=key),self.assertRaises(AssertionError):audit.confirmed_cleanup(changed)
        for key in ['finalization_error','failed_artifact_seal','pre_seal_report']:
            changed=copy.deepcopy(report);changed[key]={}
            with self.subTest(key=key),self.assertRaises(AssertionError):audit.confirmed_cleanup(changed)
    def fixture(self,name='safe/member.txt',symlink=False):
        temp=tempfile.TemporaryDirectory(prefix='combined-E-zip-synthetic-',dir=HERE);self.addCleanup(temp.cleanup);p=pathlib.Path(temp.name);z=p/'fixture.zip'
        with zipfile.ZipFile(z,'w') as f:
            info=zipfile.ZipInfo(name)
            if symlink:info.external_attr=(0o120777<<16)
            f.writestr(info,b'synthetic archive storage control\n')
        meta=p/'artifact-metadata.json';meta.write_text(json.dumps(dict(id=1,workflow_run=dict(head_sha=audit.HEAD,id=audit.RUN),size_in_bytes=z.stat().st_size,digest='sha256:'+audit.file_hash(z))))
        return p,z,meta
    def test_zip_original_bytes_extract_readonly_and_hash(self):
        p,z,m=self.fixture();r=preflight.inventory(z,m,p/'extracted');v=preflight.extract_originals(r)
        self.assertTrue(v['zip_unchanged']);self.assertEqual((p/'extracted/safe/member.txt').read_bytes(),b'synthetic archive storage control\n')
    def test_zip_traversal_and_symlink_rejected(self):
        for name,symlink in [('../outside',False),('/absolute',False),('safe/link',True)]:
            p,z,m=self.fixture(name,symlink)
            with self.subTest(name=name),self.assertRaises(AssertionError):preflight.inventory(z,m,p/'extracted')
    def test_zip_normalized_path_collision_rejected(self):
        p,z,m=self.fixture('safe//member.txt')
        with zipfile.ZipFile(z,'a') as archive:archive.writestr('safe/member.txt',b'conflicting synthetic path')
        v=json.loads(m.read_text());v.update(size_in_bytes=z.stat().st_size,digest='sha256:'+audit.file_hash(z));m.write_text(json.dumps(v))
        with self.assertRaises(AssertionError):preflight.inventory(z,m,p/'extracted')
    def test_wrong_head_and_insufficient_space_do_not_extract(self):
        p,z,m=self.fixture();v=json.loads(m.read_text());v['workflow_run']['head_sha']='0'*40;m.write_text(json.dumps(v))
        with self.assertRaises(AssertionError):preflight.inventory(z,m,p/'extracted')
        v['workflow_run']['head_sha']=audit.HEAD;m.write_text(json.dumps(v))
        with mock.patch.object(preflight.shutil,'disk_usage',return_value=type('Usage',(),{'free':0})()):r=preflight.inventory(z,m,p/'extracted')
        with self.assertRaises(AssertionError):preflight.extract_originals(r)
        self.assertFalse((p/'extracted').exists())

if __name__=='__main__':unittest.main(verbosity=2)
