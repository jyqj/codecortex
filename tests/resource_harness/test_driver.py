import ast
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
from resource_harness import driver, protocol
from resource_harness.identity import SOURCES, precheck
from resource_harness.identity import checkout_identity
from resource_harness.measurement import Gate
from resource_harness.measurement import measure
from resource_harness.build import build_one
from resource_harness.product import Product
from resource_harness.runtime import Journal, Observer, RpcFailure

MODULE = Path(driver.__file__).parent
FAKE = Path(__file__).with_name('fake_product.py')


class DriverTests(unittest.TestCase):
    def group(self, directory):
        group = directory / 'group'
        group.mkdir()
        for name, value in {'memory.current': '1024', 'memory.stat': 'anon 512\nfile 256\nslab 128',
                            'memory.events': 'oom 0\noom_kill 0', 'memory.max': 'max',
                            'cpu.max': 'max 100000', 'cgroup.procs': str(os.getpid())}.items():
            (group / name).write_text(value + '\n')
        return group

    def test_manifest_exact_protocol_and_preserved_helpers(self):
        manifest = json.loads((MODULE / 'protocol-manifest.json').read_text())
        self.assertEqual(manifest['sources'], SOURCES)
        self.assertEqual(manifest['workload']['count'], 100000)
        self.assertEqual([manifest['workload'][k] for k in
            ('dimensions', 'parse', 'http_global', 'http_per_project', 'claim_batch_items')],
            [128, 4, 4, 2, 16])
        self.assertEqual(manifest['timeouts'], dict(ready=300,rpc=300,eof=15,
            controlled_terminate_wait=15,overall=900,http_hold=120,http_file_wait=20,
            mock_terminate_wait=10))
        self.assertEqual(manifest['limits'], dict(memory_guard_bytes=12884901888,
                                                  disk_reserve_bytes=4294967296))
        self.assertEqual(manifest['intervals']['status_gap_seconds'], .2)
        text = Path(protocol.__file__).read_text()
        for node in ast.parse(text).body:
            if isinstance(node, ast.FunctionDef) and node.name in manifest['preserved_function_source_sha256']:
                self.assertEqual(hashlib.sha256(ast.get_source_segment(text,node).encode()).hexdigest(),
                                 manifest['preserved_function_source_sha256'][node.name], node.name)
        self.assertEqual(protocol.source(99999), 'pub fn resource_99999() -> u32 { 99999 }\n')

    def test_import_help_manifest_never_launch_or_build(self):
        with patch('subprocess.Popen', side_effect=AssertionError('unexpected launch')), \
             patch('subprocess.run', side_effect=AssertionError('unexpected command')), \
             patch('subprocess.check_output', side_effect=AssertionError('unexpected read command')), \
             patch('sys.stdout', new=io.StringIO()):
            self.assertEqual(driver.main(['manifest']), 0)
            with self.assertRaises(SystemExit) as caught:
                driver.main(['--help'])
            self.assertEqual(caught.exception.code, 0)

    def test_measurement_cannot_build_and_all_original_postready_gates_survive(self):
        # Compare assertion ASTs directly with fixed original script, read-only.
        ref = 'da8b618cbf8b67683c6b16880419089f7159c15e'
        path = 'artifacts/checkpoints/localwidth4-100k-paired-20261003/candidate/run.py'
        old = subprocess.check_output(['git','show',f'{ref}:{path}'],
                                      cwd=MODULE.parents[1], text=True)
        new = (MODULE / 'measurement.py').read_text()
        original_tail = old[old.index('    queries=['):old.index('    summary["status"]="passed')]
        original_tail = 'def fixture():\n' + original_tail
        expected = [ast.dump(n.test) for n in ast.walk(ast.parse(original_tail)) if isinstance(n,ast.Assert)]
        actual = [ast.dump(n.test) for n in ast.walk(ast.parse(new)) if isinstance(n,ast.Assert)]
        self.assertTrue(expected)
        for assertion in expected:
            self.assertIn(assertion, actual)
        for node in ast.walk(ast.parse(new)):
            if isinstance(node, ast.ImportFrom):
                self.assertNotIn(node.module, ('.build', 'build'))
        self.assertNotIn('cargo', new)
        self.assertNotIn('build_one', new)
        self.assertNotIn('prepare(', new)

    def test_missing_receipts_cannot_reach_preflight_or_measurement(self):
        with tempfile.TemporaryDirectory() as temp:
            base = Path(temp)
            measure = lambda *a, **k: self.fail('premature measurement')
            with self.assertRaises(FileNotFoundError):
                driver.run_single(base/'missing.json','candidate',base/'out',base,'unknown',measure_fn=measure)
            self.assertFalse((base/'out').exists())

    def test_build_barrier_candidate_own_smoke_and_one_formal_run(self):
        for variant in SOURCES:
            with self.subTest(variant=variant), tempfile.TemporaryDirectory() as temp:
                base=Path(temp);bundle_path=base/'ready';bundle_path.write_text('fixture')
                bundle=dict(receipts={v:dict(sha256=v) for v in SOURCES})
                receipts={v:dict(source_root=str(base/v)) for v in SOURCES}
                calls=[]
                def measure(root,out,sha,receipt,receipt_digest,group,scope,**kw):
                    calls.append((root.name,kw.get('preflight',False)))
                    return dict(status='passed_preflight_32' if kw else 'passed_declared_100k_local_scope',
                                files=32 if kw else 100000,failures=[],not_run=[])
                with patch.object(driver,'load_bundle',return_value=(bundle,receipts)) as load, \
                     patch.object(driver,'compiler_scan',return_value={'fixture':True}), \
                     patch.object(driver,'precheck',return_value={'fixture':True}), \
                     patch('resource_harness.build.build_one',side_effect=AssertionError('measurement tried build')):
                    self.assertEqual(driver.run_single(bundle_path,variant,base/'out',base,'unknown',measure_fn=measure),0)
                    self.assertEqual(load.call_count,2)
                expected=[(v,True) for v in dict.fromkeys((variant,'candidate'))]+[(variant,False)]
                self.assertEqual(calls,expected)
                session=json.loads((base/'out/session.json').read_text())
                self.assertEqual(session['formal_runs'],1)
                self.assertTrue(session['candidate_own_cache_smoke_complete'])
                self.assertFalse(session['strict_paired_causal_speedup'])
                self.assertEqual(session['group_independence'],'unknown')

    def test_failed_smoke_preserved_no_formal_no_build_fallback(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);ready=base/'ready';ready.write_text('fixture')
            bundle=dict(receipts={v:dict(sha256=v) for v in SOURCES})
            receipts={v:dict(source_root=str(base/v)) for v in SOURCES}
            with patch.object(driver,'load_bundle',return_value=(bundle,receipts)), \
                 patch.object(driver,'compiler_scan',return_value={}), \
                 patch.object(driver,'precheck',return_value={}):
                calls=[]
                def failed(*args,**kw):
                    calls.append(kw)
                    return dict(status='failed',files=32,failures=['fixture'],not_run=[])
                with self.assertRaisesRegex(RuntimeError,'no 100k'):
                    driver.run_single(ready,'candidate',base/'out',base,'unknown',measure_fn=failed)
                self.assertEqual(calls,[dict(preflight=True)])
                self.assertEqual(json.loads((base/'out/session.json').read_text())['formal_runs'],0)

    def test_unreadable_required_group_is_error_not_zero_or_product_fallback(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp)
            with self.assertRaisesRegex(RuntimeError,'unreadable'):
                precheck(base/'missing',base)

    def test_pinned_blob_bytes_reject_change_even_when_git_hides_dirty_file(self):
        # Entirely self-owned tiny Git fixture; no production checkout or build.
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);(base/'scripts').mkdir();(base/'crates/toy').mkdir(parents=True)
            preparation=base/'scripts/p7_release_resource_preparation.py'
            preparation.write_text('# inert fixture\n')
            source=base/'crates/toy/lib.rs';source.write_text('// inert toy source\n')
            def git(*args):
                return subprocess.check_output(['git',*args],cwd=base,text=True).strip()
            git('-c','init.templateDir=','init','--quiet')
            git('add','scripts','crates')
            git('-c','user.name=Fixture','-c','user.email=fixture@example.invalid',
                'commit','--quiet','-m','self-owned inert fixture')
            fixture_sha=git('rev-parse','HEAD')
            with patch.dict('resource_harness.identity.SOURCES',candidate=fixture_sha), \
                 patch('resource_harness.identity.PREPARATION_SHA256',
                       hashlib.sha256(preparation.read_bytes()).hexdigest()):
                identity=checkout_identity(base,'candidate')
                self.assertEqual(identity['source_sha'],fixture_sha)
                git('update-index','--assume-unchanged','crates/toy/lib.rs')
                source.write_text('// different inert fixture\n')
                self.assertEqual(git('status','--porcelain'),'')
                with self.assertRaisesRegex(RuntimeError,'pinned source bytes differ'):
                    checkout_identity(base,'candidate')

    def test_fake_build_phase_completes_before_receipt_and_never_launches_product(self):
        # A Python child prints a toy artifact receipt; it performs no compilation.
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);group=self.group(base);binary=base/'toy-binary'
            binary.write_text('inert fixture bytes')
            identity=dict(source_sha=SOURCES['candidate'],source_tree='fixture-tree',
                          source_root=str(base),source_files_sha256={})
            artifact=dict(reason='compiler-artifact',target=dict(name='codecortex'),
                          executable=str(binary),features=['semantic','semantic-http'],
                          profile=dict(opt_level='3',debug_assertions=False))
            native=subprocess.Popen
            def launch(argv,**kw):
                self.assertEqual(argv[1:4],['build','--release','-p'])
                return native([sys.executable,'-u','-c','print('+repr(json.dumps(artifact))+')'],**kw)
            with patch('resource_harness.build.checkout_identity',return_value=identity), \
                 patch('subprocess.check_output',return_value='rustc 1.95.0 (59807616e 2026-04-14)\n'), \
                 patch('subprocess.Popen',side_effect=launch):
                receipt=build_one(base,base/'build','candidate',base,group,'fixture')
            rows=list(map(json.loads,(base/'build/composition.jsonl').read_text().splitlines()))
            self.assertEqual([r['phase'] for r in rows if r['event']=='phase_start'],
                             ['build-before','build','build-complete'])
            self.assertEqual(receipt['build_exit_code'],0)
            self.assertEqual(receipt['binary_sha256'],hashlib.sha256(binary.read_bytes()).hexdigest())
            self.assertEqual(set(receipt['log_sha256']),
                             {'cargo-build.jsonl','build-stderr.log','composition.jsonl'})

    def test_fake_product_single_reader_initialized_unpack_phase_normal_stop(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);group=self.group(base);out=base/'out';out.mkdir()
            stream=io.StringIO();observer=Observer(Journal(stream),group,scope='fixture')
            native=subprocess.Popen
            def launch(argv,**kwargs):
                self.assertEqual(argv[1:3],['mcp','--project-path'])
                return native([sys.executable,'-u',str(FAKE)],**kwargs)
            with patch('resource_harness.product.subprocess.Popen',side_effect=launch):
                p=Product(base/'harmless-fixture',base,out,base/'own-cache',observer,os.getpid())
            try:
                p.initialize(out)
                p.phase='cold-empty-index-build'
                self.assertEqual(p.tool('wrapped',{'ok':True}),{'ok':True})
                self.assertEqual(p.tool('bare',{'bare':1}),{'bare':1})
                with self.assertRaises(RuntimeError):p.tool('error',{})
                p.phase='normal-eof-exit'
                p.p.stdin.close();self.assertEqual(p.p.wait(timeout=2),0)
                p.close_logs()
            finally:
                if p.p.poll() is None:p.p.stdin.close();p.p.wait(timeout=2);p.close_logs()
            rows=[json.loads(line) for line in (out/'rpc.jsonl').read_text().splitlines()]
            self.assertEqual(sum(r['event']=='notification' for r in rows),1)
            self.assertEqual(sum(r['event']=='stdout_wire' for r in rows),5)
            self.assertIn('stdout_eof',[r.get('kind') for r in rows])
            self.assertIn('process_exit',[r.get('kind') for r in rows])
            with self.assertRaises(RpcFailure):p.transport.pending.raise_if_terminal()
            phases=[r['phase'] for r in map(json.loads,stream.getvalue().splitlines()) if r['event']=='phase_start']
            self.assertEqual(phases,['productstartup-before','productstartup','cold-empty-index-build','normal-eof-exit'])

    def test_guard_persists_failure_before_waking_fixture_pending_and_ready_check(self):
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);group=self.group(base);out=base/'out';out.mkdir()
            observer=Observer(Journal(io.StringIO()),group,scope='fixture')
            native=subprocess.Popen
            with patch('resource_harness.product.subprocess.Popen',
                       side_effect=lambda a,**kw:native([sys.executable,'-u',str(FAKE)],**kw)):
                p=Product(base/'fixture',base,out,base/'cache',observer,os.getpid())
            summary=dict(status='running',failures=[]);gate=Gate(summary,out,observer)
            failures=[]
            def pending():
                try:p.tool('pending',{})
                except RpcFailure as exc:failures.append(exc.event['kind'])
            thread=threading.Thread(target=pending);thread.start()
            end=time.monotonic()+2
            while not p.transport.pending.pending and time.monotonic()<end:time.sleep(.001)
            self.assertTrue(p.transport.pending.pending)
            def harmless_eof():
                self.assertEqual(json.loads((out/'summary.json').read_text())['status'],'failed')
                self.assertEqual(p.transport.pending.terminal_event['kind'],'resource_guard')
                p.p.stdin.close()
            with patch.object(p.p,'terminate',side_effect=harmless_eof):
                gate.fail('memory_above_12GiB',p,None,observed_bytes=12884901889)
            thread.join(timeout=2);self.assertFalse(thread.is_alive())
            p.p.wait(timeout=2);p.close_logs()
            self.assertEqual(failures,['resource_guard'])
            with self.assertRaisesRegex(RuntimeError,'memory_above'):gate.check(p)

    def test_composed_preflight_32_only_fake_product_cache_http_and_all_phases(self):
        # Executes only this new toy fixture and owned synthetic loopback mock.
        # No build, Rust binary, production database or 100k generation.
        with tempfile.TemporaryDirectory() as temp:
            base=Path(temp);group=self.group(base)
            binary=Path(__file__).with_name('fake_cache_product.py').resolve()
            receipt=dict(source_sha=SOURCES['candidate'],source_tree='fixture-tree',
                build_exit_code=0,guard_stop=None,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),
                compiler_artifact=dict(executable=str(binary),features=['semantic','semantic-http'],
                                       profile=dict(opt_level='3',debug_assertions=False)))
            native=subprocess.Popen
            def launch(argv,**kw):
                if argv[0]==str(binary):
                    return native([sys.executable,'-u',str(binary),argv[-1]],**kw)
                if argv[0]=='git':
                    return native(argv,**kw)
                self.assertEqual(argv[0],sys.executable)
                self.assertEqual(Path(argv[1]).name,'protocol.py')
                return native(argv,**kw)
            with patch('subprocess.Popen',side_effect=launch), \
                 patch('resource_harness.build.build_one',side_effect=AssertionError('premature build')):
                summary=measure(base,base/'out',SOURCES['candidate'],receipt,'fixture-receipt',
                                group,'fixture',preflight=True)
            self.assertEqual(summary['status'],'passed_preflight_32',summary['failures'])
            self.assertFalse(summary['not_run'])
            self.assertEqual(summary['files'],32)
            hold=json.loads((base/'out/sharedgate-preflight.json').read_text())
            self.assertEqual((hold['actual_http_held'],hold['actual_returned']),(2,0))
            phases=[r['phase'] for r in map(json.loads,(base/'out/composition.jsonl').read_text().splitlines())
                    if r['event']=='phase_start']
            for phase in ('generation-before','generation-after','productstartup-before','productstartup',
                          'cold-empty-index-build','backfill-drain','serial-reference',
                          'concurrent-repeated','concurrent-distinct','normal-eof-exit',
                          'reopen-status-and-query','cleanup','cleanup-after'):
                self.assertIn(phase,phases)
            self.assertEqual(phases.count('productstartup'),2)
            self.assertTrue((base/'out/live/n32/cache/complete').exists())


if __name__ == '__main__':
    unittest.main()
