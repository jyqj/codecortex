"""Harmless temporary inputs; current validator is required, no product runs."""
import os
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / 'scripts'))
import p0_historical_corpus as corpus


class HistoricalCorpusTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.validator = Path(os.environ.get('CC_EVAL_VALIDATOR', corpus.ROOT / 'target/debug/cc-eval'))
        if not cls.validator.is_file():
            raise RuntimeError('build the current cc-eval validator before these tests')

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='p0-historical-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.manifest = corpus.materialize(self.root)

    def rejected(self, reason):
        result = subprocess.run([str(self.validator.resolve()), 'validate', '--suite', str(self.manifest)],
                                capture_output=True, text=True, timeout=60)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertIn(reason, result.stderr)

    def test_exact_historical_inputs(self):
        corpus.validate(self.validator, self.manifest, include_other=False)
        self.assertEqual({str(p.relative_to(self.root)) for p in self.root.rglob('*') if p.is_file()}, set(corpus.PINS))
        for path in (corpus.MANIFEST, corpus.QUERY):
            self.assertEqual((self.root / path).read_bytes(), (corpus.ROOT / path).read_bytes())

    def test_changed_source(self):
        path = self.root / 'crates/cc-search/src/plan.rs'
        path.write_bytes(path.read_bytes() + b'// harmless negative\n')
        self.rejected('source or query content lock drift')

    def test_missing_source(self):
        (self.root / 'crates/cc-search/src/plan.rs').unlink()
        self.rejected('No such file or directory')

    def test_changed_query(self):
        path = self.root / corpus.QUERY
        path.write_bytes(path.read_bytes() + b'\n')
        self.rejected('source or query content lock drift')

    def test_missing_query(self):
        (self.root / corpus.QUERY).unlink()
        self.rejected('No such file or directory')

    def test_path_escape(self):
        for path in ('../escape', '/tmp/escape', 'crates/../escape', 'crates\\escape'):
            with self.subTest(path=path), self.assertRaisesRegex(ValueError, 'allowlist'):
                corpus.destination(self.root, path)

    def test_validator_source_path_escape(self):
        manifest = json.loads(self.manifest.read_bytes())
        manifest['source']['files'][0] = '../escape.rs'
        self.manifest.write_text(json.dumps(manifest))
        self.rejected('non-canonical relative path')

    def test_changed_live_controls(self):
        original = Path.read_bytes
        for path in (corpus.MANIFEST, corpus.QUERY):
            def changed(p):
                raw = original(p)
                return raw + b'\n' if p == corpus.ROOT / path else raw
            with self.subTest(path=path), tempfile.TemporaryDirectory() as fresh:
                with mock.patch.object(Path, 'read_bytes', changed):
                    with self.assertRaisesRegex(ValueError, 'historical bytes differ'):
                        corpus.materialize(Path(fresh))
                self.assertEqual(list(Path(fresh).iterdir()), [])

    def test_symlink_escape(self):
        with tempfile.TemporaryDirectory() as outside:
            fresh = self.root / 'fresh'
            fresh.mkdir()
            (fresh / 'crates').symlink_to(outside, target_is_directory=True)
            with self.assertRaisesRegex(ValueError, 'escape'):
                corpus.destination(fresh, corpus.MANIFEST)

    def test_nonfresh_tree(self):
        with self.assertRaisesRegex(ValueError, 'fresh empty'):
            corpus.materialize(self.root)

    def test_stale_anchor(self):
        with mock.patch.object(corpus, 'git', return_value=b'0' * 40):
            with self.assertRaisesRegex(ValueError, 'stale historical anchor'):
                corpus.ensure_anchor()

    def test_missing_anchor_fetch_fails_closed(self):
        with mock.patch.object(corpus.subprocess, 'run', side_effect=[
                subprocess.CompletedProcess([], 1), subprocess.CalledProcessError(1, 'git fetch')]):
            with self.assertRaises(subprocess.CalledProcessError):
                corpus.ensure_anchor()

    def test_wrong_historical_blob(self):
        original = corpus.git
        def changed(*args):
            if args[0] == 'show' and args[1].endswith(':crates/cc-search/src/plan.rs'):
                return b'// nearby source is not the recorded source\n'
            return original(*args)
        with tempfile.TemporaryDirectory() as fresh, mock.patch.object(corpus, 'git', side_effect=changed):
            with self.assertRaisesRegex(ValueError, 'historical bytes differ'):
                corpus.materialize(Path(fresh))
            self.assertEqual(list(Path(fresh).iterdir()), [])

    def test_missing_historical_blob(self):
        original = corpus.git
        def missing(*args):
            if args[0] == 'ls-tree' and args[-1] == 'crates/cc-search/src/plan.rs':
                return b''
            return original(*args)
        with tempfile.TemporaryDirectory() as fresh, mock.patch.object(corpus, 'git', side_effect=missing):
            with self.assertRaisesRegex(ValueError, 'missing or non-file'):
                corpus.materialize(Path(fresh))


if __name__ == '__main__':
    unittest.main()
