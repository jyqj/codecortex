"""Body-free split counterexamples and production contamination controls."""

from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_corpus_audit as corpus
import p8_leakage_audit as audit


def metadata(ident, family, split="dev", text_fingerprint="synthetic fingerprint"):
    return {"id_sha256": corpus.sha(ident.encode()), "family": family, "split": split,
            "query_sha256": corpus.sha(text_fingerprint.encode())}


class FamilyBoundaryTests(unittest.TestCase):
    def test_independent_metadata_components_can_remain_separated(self):
        rows = [metadata("x", "f1"), metadata("y", "f2", "holdout", "different synthetic fingerprint")]
        self.assertEqual(audit.audit_family_metadata(rows, [{"members": ["f1"]}, {"members": ["f2"]}]), {})

    def test_translation_family_split_is_quarantined_even_with_distinct_hashes(self):
        rows = [metadata("en", "f1"), metadata("zh", "f1", "holdout", "different synthetic fingerprint")]
        errors = audit.audit_family_metadata(rows, [{"members": ["f1"]}])
        self.assertEqual(errors, {"component_cross_split": 1, "family_cross_split": 1})

    def test_transitive_component_does_not_hide_cross_split_relation(self):
        rows = [metadata("1", "f1"), metadata("2", "f2", "holdout", "two"), metadata("3", "f3", "dev", "three")]
        self.assertEqual(audit.audit_family_metadata(rows, [{"members": ["f1", "f2", "f3"]}]), {"component_cross_split": 1})

    def test_exact_fingerprint_leak_detected_without_body_access(self):
        rows = [metadata("x", "f1"), metadata("y", "f2", "holdout")]
        self.assertEqual(audit.audit_family_metadata(rows, [{"members": ["f1"]}, {"members": ["f2"]}]),
                         {"normalized_query_cross_split": 1})

    def test_missing_and_duplicate_membership_are_errors(self):
        rows = [metadata("x", "f1"), metadata("y", "f2")]
        errors = audit.audit_family_metadata(rows, [{"members": ["f1"]}, {"members": ["f1"]}])
        self.assertEqual(errors, {"duplicate_component_membership": 1, "unregistered_family": 1})

    def test_body_fields_are_not_accepted_as_metadata(self):
        row = metadata("x", "f1")
        row["query"] = "synthetic forbidden field"
        with self.assertRaisesRegex(corpus.Invalid, "body-free"):
            audit.audit_family_metadata([row], [{"members": ["f1"]}])


class ProductionBoundaryTests(unittest.TestCase):
    def setUp(self):
        self.query = "Where does this fictional public DEV query find its answer?"
        self.fingerprints = {corpus.sha(corpus.normalize(self.query).encode())}

    def scan(self, source):
        return audit.scan_text("crates/fiction/src/lib.rs", source.encode(), self.fingerprints, {"src/fiction/example.rs"})

    def test_generic_source_is_not_marked_as_a_corpus_hit(self):
        self.assertEqual(self.scan('pub fn rank(name: &str) -> usize { name.len() }'), [])

    def test_literal_query_dictionary_injection_detected(self):
        findings = self.scan('const QUERIES: &[&str] = &["' + self.query + '"];')
        self.assertEqual(findings[0]["rule"], "public_dev_query_literal")
        self.assertNotIn(self.query, str(findings))

    def test_raw_and_escaped_literal_queries_detected(self):
        variants = ['const Q: &str = r##"' + self.query + '"##;',
                    'const Q: &str = "' + self.query.replace("DEV", "D\\u{45}V").replace(" public", "\\npublic") + '";']
        for source in variants:
            with self.subTest(source=source):
                self.assertEqual(self.scan(source)[0]["rule"], "public_dev_query_literal")

    def test_gold_path_in_inline_test_is_preserved_for_review(self):
        rows = self.scan('#[cfg(test)] mod tests { const P: &str = "src/fiction/example.rs"; }')
        self.assertEqual(rows[0]["rule"], "public_dev_gold_path_literal_review")

    def test_benchmark_include_is_flagged(self):
        rows = self.scan('const Q: &str = include_str!("../benchmarks/queries.jsonl");')
        self.assertIn("benchmark_import_review", {r["rule"] for r in rows})

    def test_renamed_eval_dependency_is_not_hidden_by_alias(self):
        findings = audit.dependency_findings("Cargo.toml", b'[dependencies]\nhelper = { package = "cc-eval", path = "../cc-eval" }\n')
        self.assertEqual(findings[0]["rule"], "production_eval_dependency")

    def test_platform_build_dependency_is_checked(self):
        findings = audit.dependency_findings("Cargo.toml", b'[target.\'cfg(unix)\'.build-dependencies]\nhelper = { package = "cc-eval", version = "1" }\n')
        self.assertEqual(len(findings), 1)

    def test_dev_dependency_is_reported_outside_production_scope(self):
        self.assertEqual(audit.dependency_findings("Cargo.toml", b'[dev-dependencies]\ncc-eval = { path = "../cc-eval" }\n'), [])

    def test_inherited_workspace_alias_is_resolved_before_checking(self):
        findings = audit.dependency_findings("Cargo.toml", b'[dependencies]\nhelper = { workspace = true }\n',
                                            {"helper": {"package": "cc-eval", "path": "crates/cc-eval"}})
        self.assertEqual(findings[0]["rule"], "production_eval_dependency")

    def test_missing_workspace_alias_is_input_failure(self):
        with self.assertRaisesRegex(corpus.Invalid, "unresolved workspace"):
            audit.dependency_findings("Cargo.toml", b'[dependencies]\nhelper = { workspace = true }\n', {})


if __name__ == "__main__":
    unittest.main()
