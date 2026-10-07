"""Independent corruption and domain controls for the public DEV audit."""

import copy
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_corpus_audit as audit


def example_rows():
    row = {"id": "fixture.f1.en", "query_family": "fixture.f1", "category": "api_usage",
           "difficulty": 2, "language": "rust", "split": "dev",
           "query": "Where does this fictional sample expose its entry point?", "path_prefix": None,
           "no_answer": False, "expected_files": [],
           "answers": [{"id": "support", "primary": False, "grade": 2,
                        "alternatives": [{"path": "src/support.rs", "span": {"start": 0, "end": 4}}]},
                       {"id": "primary", "primary": True, "grade": 3,
                        "alternatives": [{"path": "src/entry.rs", "span": {"start": 0, "end": 4}}]}],
           "annotations": {"v19": {"source_sha": "a" * 40}}}
    projected = copy.deepcopy(row)
    projected["answers"] = []
    projected["expected_files"] = ["src/entry.rs", "src/support.rs"]
    no_answer = copy.deepcopy(row)
    no_answer.update(id="fixture.f2.en", query_family="fixture.f2", answers=[], no_answer=True)
    return [row, no_answer], [projected]


class CorpusRulesTests(unittest.TestCase):
    def test_valid_projection_keeps_no_answer_out_of_compat_denominator(self):
        native, compat = example_rows()
        self.assertEqual(audit.check_projection(native, compat), ({}, 1))

    def test_duplicate_id_is_not_hidden_by_dictionary_overwrite(self):
        native, compat = example_rows()
        native.append(copy.deepcopy(native[0]))
        self.assertEqual(audit.check_projection(native, compat)[0]["native_duplicate_id"], 1)

    def test_compat_text_and_primary_order_drift_are_separate_errors(self):
        native, compat = example_rows()
        compat[0]["query"] += " changed"
        compat[0]["expected_files"].reverse()
        errors, _ = audit.check_projection(native, compat)
        self.assertEqual(errors, {"projection_identity_drift": 1, "projection_gold_drift": 1})

    def test_omitted_compat_answer_is_not_silently_accepted(self):
        self.assertIn("compat_denominator_drift", audit.check_projection(example_rows()[0], [])[0])

    def test_query_parser_rejects_nondev_before_echoing_a_body(self):
        row = example_rows()[0][0]
        row["split"] = "holdout"
        with self.assertRaisesRegex(audit.Invalid, "non-DEV") as raised:
            audit.query_rows(json.dumps(row).encode())
        self.assertNotIn(row["query"], str(raised.exception))

    def test_duplicate_json_key_and_unknown_schema_are_rejected(self):
        with self.assertRaises(audit.Invalid):
            audit.parse_json(b'{"split":"dev","split":"dev"}')
        row = example_rows()[0][0]
        row["unexpected"] = True
        with self.assertRaises(audit.Invalid):
            audit.query_rows(json.dumps(row).encode())

    def test_source_span_and_utf8_boundaries_are_checked(self):
        row = example_rows()[0][0]
        self.assertEqual(audit.source_check([row], {"src/entry.rs": b"code", "src/support.rs": b"more"}, "a" * 40), ({}, 2))
        row["answers"][0]["alternatives"][0]["span"] = {"start": 1, "end": 4}
        errors, spans = audit.source_check([row], {"src/entry.rs": b"code", "src/support.rs": "éab".encode()}, "a" * 40)
        self.assertEqual(errors["span_utf8_boundary"], 1)
        self.assertEqual(spans, 1)

    def test_source_evidence_hash_detects_equal_length_corruption(self):
        row = example_rows()[0][0]
        row["annotations"]["v19"]["gold_evidence"] = [{"path": "src/entry.rs", "source_sha": "a" * 40,
            "start_byte": 0, "end_byte": 4, "file_sha256": audit.sha(b"code"), "span_sha256": audit.sha(b"code")}]
        errors, _ = audit.source_check([row], {"src/entry.rs": b"evil", "src/support.rs": b"more"}, "a" * 40)
        self.assertEqual(errors, {"annotation_source_evidence_drift": 1})

    def test_source_outside_domain_cannot_be_counted_as_valid_span(self):
        errors, spans = audit.source_check([example_rows()[0][0]], {}, "a" * 40)
        self.assertEqual(errors["gold_outside_source"], 2)
        self.assertEqual(spans, 0)

    def test_unsafe_and_protected_paths_rejected(self):
        for name in ("../queries.dev.jsonl", "foo//bar", "gold/held-out.json", "data/HOLDOUT.json", "x\\y"):
            with self.subTest(name=name), self.assertRaises(audit.Invalid):
                audit.relative(name)


class PinnedGitTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="p8-public-audit-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.path = audit.PREFIX + "fixture/queries.native.dev.jsonl"
        target = self.root / self.path
        target.parent.mkdir(parents=True)
        self.raw = b'{"synthetic":"public dev unit-test input"}\n'
        target.write_bytes(self.raw)
        self.git("init", "-q")
        self.git("add", ".")
        self.git("-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid", "commit", "-qm", "fixture")
        self.ref = self.git("rev-parse", "HEAD").decode().strip()
        self.entry = self.ref + ":" + self.path
        self.store = audit.PinnedBlobs(self.root)
        self.store.add_admission_pins({self.entry: audit.sha(self.raw)})

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args], stderr=subprocess.DEVNULL)

    def test_read_is_exact_historical_blob_not_current_mutable_file(self):
        (self.root / self.path).write_bytes(b"changed working tree")
        self.assertEqual(self.store.read(self.entry), self.raw)
        self.assertEqual(self.store.total_bytes, len(self.raw))
        self.assertEqual(self.store.read(self.entry), self.raw)
        self.assertEqual(self.store.total_bytes, len(self.raw))

    def test_wrong_digest_is_rejected_without_adding_verified_cache(self):
        self.store.pins[self.entry] = "0" * 64
        with self.assertRaisesRegex(audit.Invalid, "hash drift"):
            self.store.read(self.entry)
        self.assertEqual(self.store.cache, {})

    def test_unlisted_and_protected_entries_never_call_git(self):
        for path in (audit.PREFIX + "fixture/unlisted.json", audit.PREFIX + "fixture/holdout.jsonl"):
            with self.subTest(path=path), mock.patch.object(audit, "git") as git:
                with self.assertRaises(audit.Invalid):
                    self.store.read(self.ref + ":" + path)
                git.assert_not_called()

    def test_symlink_git_mode_rejected_even_with_matching_content_hash(self):
        target = self.root / self.path
        target.unlink()
        target.symlink_to("not-followed")
        self.git("add", ".")
        self.git("-c", "user.name=P8 fixture", "-c", "user.email=p8@example.invalid", "commit", "-qm", "symlink")
        entry = self.git("rev-parse", "HEAD").decode().strip() + ":" + self.path
        self.store.add_admission_pins({entry: audit.sha(b"not-followed")})
        with self.assertRaisesRegex(audit.Invalid, "non-regular"):
            self.store.read(entry)

    def test_byte_limit_rejects_before_git_show(self):
        mode = b"100644 blob " + b"a" * 40 + b"\t" + self.path.encode() + b"\0"
        with mock.patch.object(audit, "git", side_effect=[mode, str(audit.MAX_BLOB + 1).encode()]) as git:
            with self.assertRaisesRegex(audit.Invalid, "byte budget"):
                self.store.read(self.entry)
            self.assertEqual(git.call_count, 2)


if __name__ == "__main__":
    unittest.main()
