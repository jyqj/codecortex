"""Synthetic input-integrity controls; these fixtures execute no product."""

import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SCRIPT = Path(__file__).resolve().parents[2] / 'scripts/lock_benchmark_inputs.py'
MODULE_SPEC = importlib.util.spec_from_file_location('benchmark_input_lock', SCRIPT)
locker = importlib.util.module_from_spec(MODULE_SPEC)
MODULE_SPEC.loader.exec_module(locker)


class InputLockTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / 'inputs'
        self.root.mkdir()
        self.output = self.base / 'lock.json'
        files = {
            'product': b'synthetic bytes; never executed\n',
            'config.json': b'{"retrieval_strategy":"auto"}\n',
            'corpus/one.rs': b'pub fn one() {}\n',
            'queries.jsonl': b'{"id":"q1","query":"one"}\n',
            'scoring.json': b'{"version":"codecortex-native-v1"}\n',
            'model.json': b'{"provider":"fake","revision":"fake-v1"}\n',
            'environment.json': b'{"os":"synthetic","cpu":null,"ram_bytes":null}\n',
            'source/Cargo.toml': b'[workspace]\n',
            'source/src/lib.rs': b'pub fn source() {}\n',
            'report/metrics.json': b'{"profile":"synthetic_no_execution","value":null}\n',
        }
        for name, data in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        source = {name.removeprefix('source/'): hashlib.sha256(data).hexdigest() for name, data in files.items() if name.startswith('source/')}
        binary_sha = hashlib.sha256(files['product']).hexdigest()
        self.write_json('source-receipt.json', source)
        self.write_json('build.json', {'binary_sha256': binary_sha, 'source_files': source, 'source_commit': 'a' * 40, 'features': ['semantic-http'], 'exit_code': 0})
        self.write_json('execution.json', {'binary_sha256': binary_sha, 'source_commit': 'a' * 40, 'features': ['semantic-http'], 'exit_code': 0, 'fixture_only': True})
        paths = [
            ('product', 'product', 'file', 'binary'),
            ('config', 'config.json', 'file', 'config'),
            ('corpus', 'corpus', 'tree', 'corpus'),
            ('queries', 'queries.jsonl', 'file', 'queries'),
            ('scoring', 'scoring.json', 'file', 'scoring'),
            ('model', 'model.json', 'file', 'model'),
            ('environment', 'environment.json', 'file', 'environment'),
            ('source', 'source', 'tree', 'source'),
            ('source_receipt', 'source-receipt.json', 'file', 'source_receipt'),
            ('build', 'build.json', 'file', 'build_receipt'),
            ('execution', 'execution.json', 'file', 'execution_receipt'),
            ('report', 'report', 'tree', 'report'),
        ]
        select = lambda item, field: {'input': item, 'pointer': field}
        self.spec = {
            'schema_version': 1, 'scope': 'preparation_only',
            'inputs': [{'id': item, 'path': path, 'kind': kind, 'roles': [role]} for item, path, kind, role in paths],
            'bindings': [
                {'kind': 'sha256', 'receipt': select('build', '/binary_sha256'), 'artifact': 'product'},
                {'kind': 'sha256', 'receipt': select('execution', '/binary_sha256'), 'artifact': 'product'},
                {'kind': 'tree_sha256', 'receipt': select('source_receipt', ''), 'artifact': 'source'},
                {'kind': 'equal', 'left': select('build', '/source_files'), 'right': select('source_receipt', '')},
                {'kind': 'equal', 'left': select('build', '/source_commit'), 'right': select('execution', '/source_commit')},
                {'kind': 'equal', 'left': select('build', '/features'), 'right': select('execution', '/features')},
                {'kind': 'value', 'at': select('build', '/features'), 'expected': ['semantic-http']},
                {'kind': 'value', 'at': select('build', '/exit_code'), 'expected': 0},
                {'kind': 'value', 'at': select('execution', '/exit_code'), 'expected': 0},
            ],
            'unresolved': ['Synthetic fixture; no product executed', 'P7-020 is incomplete', 'CPU and RAM are unknown'],
        }

    def write_json(self, name, value):
        (self.root / name).write_bytes(locker.json_bytes(value))

    def prepare(self):
        self.pin = locker.prepare(self.root, self.spec, self.output)['lock_sha256']

    def verify(self):
        return locker.verify(self.root, self.output, self.pin)

    def test_roundtrip_is_read_only_and_does_not_collect_environment_or_execute(self):
        before = {str(path.relative_to(self.root)): path.read_bytes() for path in self.root.rglob('*') if path.is_file()}
        with mock.patch.dict(os.environ, {'UNRELATED_PRIVATE_VALUE': 'synthetic-secret-sentinel'}), mock.patch('subprocess.Popen', side_effect=AssertionError('product invocation is forbidden')):
            self.prepare()
            report = self.verify()
        after = {str(path.relative_to(self.root)): path.read_bytes() for path in self.root.rglob('*') if path.is_file()}
        self.assertEqual(before, after)
        self.assertEqual(report['unresolved'], self.spec['unresolved'])
        self.assertFalse(report['release_candidate'])
        self.assertFalse(report['execution_attestation'])
        self.assertNotIn(b'synthetic-secret-sentinel', self.output.read_bytes())
        self.assertIsNone(json.loads((self.root / 'environment.json').read_text())['cpu'])

    def test_each_frozen_dimension_and_report_drift_rejects_old_lock(self):
        self.prepare()
        for name in ('product', 'config.json', 'corpus/one.rs', 'queries.jsonl', 'scoring.json', 'model.json', 'environment.json', 'source/src/lib.rs', 'report/metrics.json'):
            with self.subTest(path=name):
                path = self.root / name
                old = path.read_bytes()
                path.write_bytes(old + b' ')
                with self.assertRaises(locker.LockError):
                    self.verify()
                path.write_bytes(old)
        self.assertEqual(self.verify()['status'], 'matching_preparation_inputs')

    def test_complete_directory_membership_includes_add_delete_rename_and_empty_directories(self):
        self.prepare()
        member = self.root / 'report/extra.json'
        member.write_text('{}')
        with self.assertRaises(locker.LockError):
            self.verify()
        member.unlink()
        empty = self.root / 'corpus/empty'
        empty.mkdir()
        with self.assertRaises(locker.LockError):
            self.verify()
        empty.rmdir()
        original = self.root / 'report/metrics.json'
        content = original.read_bytes()
        renamed = self.root / 'report/renamed.json'
        original.rename(renamed)
        with self.assertRaises(locker.LockError):
            self.verify()
        renamed.unlink()
        with self.assertRaises(locker.LockError):
            self.verify()
        original.write_bytes(content)
        self.verify()

    def test_mode_changes_are_input_drift(self):
        self.prepare()
        path = self.root / 'product'
        path.chmod(path.stat().st_mode ^ 0o100)
        with self.assertRaises(locker.LockError):
            self.verify()

    def test_bad_build_or_execution_binary_cannot_be_prepared(self):
        for name in ('build.json', 'execution.json'):
            with self.subTest(receipt=name):
                original = json.loads((self.root / name).read_text())
                changed = copy.deepcopy(original)
                changed['binary_sha256'] = '0' * 64
                self.write_json(name, changed)
                with self.assertRaisesRegex(locker.LockError, 'binding mismatch'):
                    locker.prepare(self.root, self.spec, self.output)
                self.assertFalse(self.output.exists())
                self.write_json(name, original)

    def test_source_map_and_profile_must_match_actual_selected_inputs(self):
        original = json.loads((self.root / 'build.json').read_text())
        for change in ({'features': []}, {'source_files': {}}, {'exit_code': 1}, {'source_commit': 'b' * 40}):
            with self.subTest(change=change):
                self.write_json('build.json', {**original, **change})
                with self.assertRaisesRegex(locker.LockError, 'binding mismatch'):
                    locker.prepare(self.root, self.spec, self.output)
        self.write_json('build.json', original)

    def test_missing_receipt_role_or_binary_binding_cannot_be_omitted(self):
        for mutate in ('role', 'binding'):
            spec = copy.deepcopy(self.spec)
            if mutate == 'role':
                spec['inputs'] = [item for item in spec['inputs'] if item['id'] != 'execution']
            else:
                spec['bindings'] = [item for item in spec['bindings'] if item.get('receipt', {}).get('input') != 'execution']
            with self.subTest(missing=mutate), self.assertRaises(locker.LockError):
                locker.prepare(self.root, spec, self.output)
        self.prepare()
        (self.root / 'execution.json').unlink()
        with self.assertRaises((locker.LockError, OSError)):
            self.verify()

    def test_one_receipt_cannot_pose_as_both_build_and_execution(self):
        spec = copy.deepcopy(self.spec)
        spec['inputs'][9]['roles'].append('execution_receipt')
        spec['bindings'][1]['receipt']['input'] = 'build'
        with self.assertRaisesRegex(locker.LockError, 'distinct'):
            locker.prepare(self.root, spec, self.output)

    def test_lock_rehash_or_promoted_claim_does_not_reuse_old_pin(self):
        self.prepare()
        lock = json.loads(self.output.read_text())
        lock['specification']['unresolved'] = []
        self.output.write_bytes(locker.json_bytes(lock))
        with self.assertRaisesRegex(locker.LockError, 'external pin'):
            self.verify()
        lock['release_candidate'] = True
        self.output.write_bytes(locker.json_bytes(lock))
        forged_pin = hashlib.sha256(self.output.read_bytes()).hexdigest()
        with self.assertRaisesRegex(locker.LockError, 'promoted'):
            locker.verify(self.root, self.output, forged_pin)

    def test_lock_cannot_overwrite_or_be_written_inside_an_input_tree(self):
        self.prepare()
        original = self.output.read_bytes()
        with self.assertRaises(FileExistsError):
            locker.prepare(self.root, self.spec, self.output)
        self.assertEqual(self.output.read_bytes(), original)
        for output in (self.root / 'report/lock.json', self.root / 'product'):
            with self.subTest(output=output), self.assertRaisesRegex(locker.LockError, 'outside'):
                locker.prepare(self.root, self.spec, output)

    def test_root_escape_symlink_and_special_file_rejected(self):
        spec = copy.deepcopy(self.spec)
        spec['inputs'][0]['path'] = '../outside'
        with self.assertRaises(locker.LockError):
            locker.prepare(self.root, spec, self.output)
        link = self.root / 'corpus/link'
        link.symlink_to(self.root / 'product')
        with self.assertRaisesRegex(locker.LockError, 'symlink'):
            locker.prepare(self.root, self.spec, self.output)
        link.unlink()
        fifo = self.root / 'report/pipe'
        os.mkfifo(fifo)
        with self.assertRaisesRegex(locker.LockError, 'regular'):
            locker.prepare(self.root, self.spec, self.output)

    def test_json_duplicate_keys_and_boolean_integer_equivalence_are_rejected(self):
        with self.assertRaises(locker.LockError):
            locker.parse_json(b'{"scope":1,"scope":1}')
        original = json.loads((self.root / 'build.json').read_text())
        original['exit_code'] = False
        self.write_json('build.json', original)
        with self.assertRaisesRegex(locker.LockError, 'binding mismatch'):
            locker.prepare(self.root, self.spec, self.output)

    def test_change_during_preparation_leaves_no_new_lock(self):
        original_snapshot = locker.snapshot
        calls = 0

        def snapshot_then_change(*args):
            nonlocal calls
            result = original_snapshot(*args)
            calls += 1
            if calls == 1:
                (self.root / 'report/extra').write_text('arrived between snapshots')
            return result

        with mock.patch.object(locker, 'snapshot', side_effect=snapshot_then_change):
            with self.assertRaisesRegex(locker.LockError, 'changed during'):
                locker.prepare(self.root, self.spec, self.output)
        self.assertFalse(self.output.exists())

    def test_cli_requires_external_pin_and_reports_drift_without_changing_report(self):
        spec = self.base / 'spec.json'
        spec.write_bytes(locker.json_bytes(self.spec))
        create = subprocess.run([sys.executable, str(SCRIPT), 'prepare', '--root', str(self.root), '--spec', str(spec), '--out', str(self.output)], capture_output=True, text=True, check=True)
        pin = json.loads(create.stdout)['lock_sha256']
        command = [sys.executable, str(SCRIPT), 'verify', '--root', str(self.root), '--lock', str(self.output)]
        missing = subprocess.run(command, capture_output=True, text=True)
        self.assertEqual(missing.returncode, 2)
        report = self.root / 'report/metrics.json'
        report.write_text('{"changed":true}')
        failed = subprocess.run(command + ['--expected-lock-sha256', pin], capture_output=True, text=True)
        self.assertEqual(failed.returncode, 2)
        self.assertIn('input lock rejected', failed.stderr)
        self.assertEqual(report.read_text(), '{"changed":true}')

    def test_cli_nonobject_lock_has_structured_rejection(self):
        self.output.write_text('[]\n')
        pin = hashlib.sha256(self.output.read_bytes()).hexdigest()
        result = subprocess.run([
            sys.executable, str(SCRIPT), 'verify', '--root', str(self.root),
            '--lock', str(self.output), '--expected-lock-sha256', pin,
        ], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('input lock rejected', result.stderr)
        self.assertNotIn('Traceback', result.stderr)


if __name__ == '__main__':
    unittest.main()
