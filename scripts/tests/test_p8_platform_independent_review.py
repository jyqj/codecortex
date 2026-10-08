"""Independent regressions for observed collector/EOF/SQLite gaps.

Run with PYTHONPATH=<checkout>/scripts:<checkout>/scripts/tests. All compiled
products here are explicitly synthetic protocol fixtures, never measurements.
"""
import contextlib
import json
from pathlib import Path
import shutil
import sqlite3
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import p7_fault_lifecycle_stdio as p7
import p8_cold_build as cold
import test_p8_platform as platform_controls


class IndependentPortableControls(unittest.TestCase):
    def setUp(self):
        self.helper = platform_controls.PlatformControls('test_all_eight_synthetic_cells_replay_and_duplicate_is_rejected')
        self.helper.setUp()
        self.addCleanup(self.helper.doCleanups)
        self.parent = cold.new_directory(self.helper.base / 'independent-eight-cells')
        self.bundles = []
        for os_name in cold.PLATFORMS:
            for tc in cold.TOOLCHAINS:
                for package in cold.PACKAGES:
                    self.bundles.append(self.helper.bundle(self.parent,
                        dict(platform=os_name, toolchain=tc, package=package)))

    def reseal(self, mutation, artifact=False):
        home = self.bundles[0]
        record = json.loads((home / 'receipt.json').read_text())
        mutation(record)
        if artifact:
            log = home / 'cargo-build.jsonl'
            rows = [json.loads(line) for line in log.read_text().splitlines() if line.strip()]
            rows = [record['cargo_artifact'] if row.get('reason') == 'compiler-artifact'
                    and row.get('target', {}).get('name') == 'codecortex' else row for row in rows]
            log.write_text(''.join(json.dumps(row) + '\n' for row in rows))
            record['logs_sha256']['cargo-build.jsonl'] = cold.digest(log)
        cold.write_json(home / 'receipt.json', record)
        bundle = json.loads((home / 'bundle.json').read_text())
        bundle['receipt_sha256'] = cold.digest(home / 'receipt.json')
        bundle['files'] = {p.relative_to(home).as_posix(): cold.digest(p)
                           for p in home.rglob('*') if p.is_file() and p.name != 'bundle.json'}
        cold.write_json(home / 'bundle.json', bundle)

    def assert_rejected(self):
        with self.assertRaises((ValueError, KeyError)):
            cold.collect_cells(self.parent, self.helper.root, self.helper.git('rev-parse', 'HEAD'))

    def test_portable_resealed_foreign_cargo_origin_is_rejected(self):
        self.reseal(lambda r: r['cargo_artifact'].update(
            manifest_path='/foreign/crates/cc-server/Cargo.toml', executable='/foreign/codecortex',
            target={'name': 'codecortex', 'kind': ['bin'], 'src_path': '/foreign/other.rs'}), artifact=True)
        self.assert_rejected()

    def test_portable_resealed_test_artifact_is_rejected(self):
        self.reseal(lambda r: r['cargo_artifact']['profile'].update(test=True), artifact=True)
        self.assert_rejected()

    def test_portable_resealed_compiler_and_target_drift_is_rejected(self):
        self.reseal(lambda r: (r['command'].__setitem__(0, '/foreign/cargo'),
            r['command'].__setitem__(r['command'].index('--target') + 1, 'unrelated-target'),
            r.update(compiler_environment={'RUSTC': '/foreign/rustc'})))
        self.assert_rejected()

    def test_portable_resealed_missing_logs_and_config_drift_is_rejected(self):
        self.reseal(lambda r: r.update(logs_sha256={}, cargo_configs_after=[
            {'path': '/changed/config', 'sha256': '0' * 64}]))
        self.assert_rejected()

    def test_portable_resealed_observer_identity_drift_is_rejected(self):
        self.reseal(lambda r: r.update(runner_sha256='0' * 64))
        self.assert_rejected()

    def test_portable_resealed_verbose_release_and_host_drift_is_rejected(self):
        def mutate(record):
            for field in ('toolchain', 'toolchain_after'):
                record[field]['rustc']['version_verbose'] = platform_controls.synthetic_rustc_verbose(
                    '1.96.0', 'aarch64-apple-darwin')
        self.reseal(mutate)
        self.assert_rejected()

    def test_portable_resealed_missing_duplicate_or_inconsistent_verbose_is_rejected(self):
        home = self.bundles[0]
        original = json.loads((home / 'receipt.json').read_text())['toolchain']['rustc']['version_verbose']
        variants = []
        for field in ('binary', 'release', 'host'):
            line = next(line for line in original.splitlines() if line.startswith(field + ': '))
            variants.extend((original.replace(line + '\n', ''), original + line + '\n'))
        variants.extend((original.replace('rustc 1.95.0 ', 'rustc 1.96.0 '),
                         original.replace('host: ', 'host:')))
        for verbose in variants:
            with self.subTest(verbose=verbose):
                def mutate(record):
                    for field in ('toolchain', 'toolchain_after'):
                        record[field]['rustc']['version_verbose'] = verbose
                self.reseal(mutate)
                self.assert_rejected()

    def test_portable_valid_cells_do_not_require_the_old_producer_vm(self):
        receiver = self.helper.base / 'receiver-checkout'
        shutil.copytree(self.helper.root, receiver)
        shutil.rmtree(self.helper.root)
        for target in self.helper.base.glob('bundle-build-*'):
            shutil.rmtree(target)
        for tools in self.helper.base.glob('toolchain-*'):
            shutil.rmtree(tools)
        result = cold.collect_cells(self.parent, receiver,
            cold.git(receiver, 'rev-parse', 'HEAD'))
        self.assertEqual(result['counts'], dict(passed=8, failed=0, not_run=0))


class IndependentActiveObserverControls(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='p8-independent-active-observer-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_shared_active_sqlite_probe_closes_on_success_and_error(self):
        real_connect = sqlite3.connect
        for invalid in (False, True):
            with self.subTest(invalid=invalid):
                project = self.root / str(invalid)
                path = project / '.codecortex/index.sqlite3'
                path.parent.mkdir(parents=True)
                with contextlib.closing(real_connect(path)) as connection:
                    if not invalid:
                        connection.execute('CREATE TABLE semantic_outbox(task_id TEXT)')
                        connection.execute('CREATE TABLE semantic_manifest(doc_key TEXT)')
                opened = []

                def connect(*args, **kwargs):
                    connection = real_connect(*args, **kwargs)
                    opened.append(connection)
                    return connection

                with mock.patch.object(p7.sqlite3, 'connect', side_effect=connect):
                    if invalid:
                        with self.assertRaises(sqlite3.OperationalError):
                            p7.database_snapshot(project, SimpleNamespace(event=lambda *a, **kw: None), 223, 'control')
                    else:
                        p7.database_snapshot(project, SimpleNamespace(event=lambda *a, **kw: None), 223, 'control')
                self.assertEqual(len(opened), 1)
                try:
                    with self.assertRaisesRegex(sqlite3.ProgrammingError, 'closed database'):
                        opened[0].execute('SELECT 1')
                finally:
                    opened[0].close()

    def product_with_tail(self, tail):
        project = self.root / 'project'
        project.mkdir()
        fake = self.root / 'contract_fake_product.py'
        fake.write_text('#!' + sys.executable + '\n' +
            'import json,sys\n' + 'tools=' + repr(sorted(p7.TOOLS)) + '\n' +
            'for line in sys.stdin:\n'
            ' request=json.loads(line)\n'
            ' if "id" not in request: continue\n'
            ' result={"tools":[{"name":name} for name in tools]} if request["method"]=="tools/list" else {}\n'
            ' print(json.dumps({"jsonrpc":"2.0","id":request["id"],"result":result}),flush=True)\n'
            ' if request["method"]=="tools/list":\n' +
            ('  print(' + repr(tail) + ',flush=True)\n' if tail else '') +
            '  break\n')
        fake.chmod(0o755)
        return p7.Product(fake, project, self.root / 'cache', self.root / 'home',
                          'contract-tail', p7.Evidence(self.root))

    def test_normal_zero_exit_does_not_hide_a_malformed_stdout_tail(self):
        product = self.product_with_tail('SYNTHETIC INVALID JSON AFTER LAST RESPONSE')
        with self.assertRaises(Exception):
            product.finish()
        self.assertEqual(product.process.poll(), 0)
        self.assertFalse(product.reader.is_alive())
        self.assertTrue(product.stdout_log.closed)
        self.assertIn(b'SYNTHETIC INVALID JSON', product.stdout_path.read_bytes())

    def test_clean_stdio_eof_still_passes_the_original_exit_contract(self):
        product = self.product_with_tail('')
        result = product.finish()
        self.assertEqual(result['exit_code'], 0)
        self.assertFalse(result['requested_sigkill'])
        self.assertFalse(result['cleanup'])

    def test_duplicate_terminal_response_is_preserved_and_rejected(self):
        product = self.product_with_tail(json.dumps({'jsonrpc': '2.0', 'id': 2, 'result': {}}))
        with self.assertRaisesRegex(AssertionError, 'stdout tail'):
            product.finish()
        self.assertFalse(product.reader.is_alive())
        self.assertTrue(product.exit_receipt['stdout_errors'])

    def test_legitimate_terminal_notification_is_preserved_and_accepted(self):
        notification = json.dumps({'jsonrpc': '2.0', 'method': 'notifications/message', 'params': {'message': 'synthetic control'}})
        product = self.product_with_tail(notification)
        record = product.finish()
        self.assertEqual(record['exit_code'], 0)
        self.assertEqual(record['terminal_notifications'], 1)
        self.assertEqual(record['stdout_errors'], [])
        self.assertIn(notification.encode(), product.stdout_path.read_bytes())


if __name__ == '__main__':
    unittest.main()

