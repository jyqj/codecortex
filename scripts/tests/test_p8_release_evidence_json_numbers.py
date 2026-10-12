"""Finite JSON numbers and controlled failures for P8 release evidence."""

import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "p8_release_evidence.py"
SPEC = importlib.util.spec_from_file_location("p8_release_evidence_json_numbers", SCRIPT)
P8 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(P8)


class ReleaseEvidenceJsonNumberTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory(prefix="p8-json-number-tests-")
        self.addCleanup(temporary.cleanup)
        # The evidence reader deliberately rejects symlink path components.
        self.root = Path(temporary.name).resolve()

    def read(self, raw):
        source = self.root / "record.json"
        source.write_text(raw)
        return P8.read_json(source)

    def test_rejects_positive_and_negative_exponent_overflow(self):
        for token in ("1e999", "-1e999", "1.7976931348623159e308"):
            with self.subTest(token=token):
                with self.assertRaises(P8.Invalid):
                    self.read('{"measurement":' + token + '}')

    def test_rejects_overflow_in_nested_objects_and_arrays(self):
        for raw in ('{"record":{"value":1e999}}',
                    '{"record":[0,{"value":-1e999}]}'):
            with self.subTest(raw=raw):
                with self.assertRaises(P8.Invalid):
                    self.read(raw)

    def test_preserves_finite_float_values(self):
        for token in ("0.0", "-0.0", "1.5", "1e308", "-1e308", "1e-308"):
            with self.subTest(token=token):
                value = self.read('{"measurement":' + token + '}')["measurement"]
                self.assertIs(type(value), float)
                self.assertTrue(math.isfinite(value))
                self.assertEqual(value, float(token))
                self.assertEqual(math.copysign(1, value), math.copysign(1, float(token)))

    def test_preserves_integer_boolean_and_string_types(self):
        record = self.read('{"count":42,"enabled":true,"label":"1e999"}')
        self.assertIs(type(record["count"]), int)
        self.assertIs(type(record["enabled"]), bool)
        self.assertEqual(record["label"], "1e999")

    def test_still_rejects_nonstandard_constants(self):
        for token in ("NaN", "Infinity", "-Infinity"):
            with self.subTest(token=token):
                with self.assertRaises(P8.Invalid):
                    self.read('{"measurement":' + token + '}')

    def test_still_rejects_duplicate_keys(self):
        with self.assertRaises(P8.Invalid):
            self.read('{"measurement":1,"measurement":2}')

    def test_verify_cli_reports_invalid_input_without_traceback_or_mutation(self):
        for kind in ("candidate", "archive"):
            with self.subTest(kind=kind):
                root = self.root / kind
                root.mkdir()
                manifest = root / (kind + ".json")
                raw = ('{"schema_version":1,"kind":"p8_local_' + kind + '",'
                       '"scope":"local_engineering_only","release_certified":false,'
                       '"' + kind + '_sha256":"' + '0' * 64 + '",'
                       '"measurement":1e999,"diagnostic":"do-not-log-input"}\n')
                manifest.write_text(raw)
                before = manifest.read_bytes()
                result = subprocess.run(
                    [sys.executable, "-B", str(SCRIPT), "verify", "--" + kind, str(root)],
                    capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stdout, "")
                self.assertNotIn("Traceback", result.stderr)
                self.assertNotIn("do-not-log-input", result.stderr)
                receipt = json.loads(result.stderr)
                self.assertEqual(receipt["status"], "invalid_input")
                self.assertTrue(receipt["partial_artifacts_retained"])
                self.assertEqual(manifest.read_bytes(), before)
                self.assertEqual(set(root.iterdir()), {manifest})


if __name__ == "__main__":
    unittest.main()
