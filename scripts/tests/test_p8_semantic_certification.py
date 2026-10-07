"""Offline evidence contract controls; fixture declarations are never live proof."""
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_semantic_certification as semantic
import p8_judge_evidence as common
import p8_release_evidence as support


def paper_fixture(root):
    """Complete synthetic metadata only, deliberately unable to certify a release."""
    selected_model = {"provider": "contract-fixture", "model": "semantic-contract",
                      "revision": "fixture-20261008-01"}
    case_ids = ["case-0001", "case-0002"]
    bodies = {"binary": b"synthetic fixture bytes, never executed\n",
              "config": b'{"profile":"semantic-contract-fixture"}\n',
              "scoring": b"synthetic scorer bytes, never executed\n"}
    filenames = {"binary": "candidate.bin", "config": "config.json", "scoring": "scoring.txt"}
    for role, raw in bodies.items():
        (root / filenames[role]).write_bytes(raw)
    pinned = {"source_commit": "a" * 40, "corpus_sha256": "b" * 64,
              "query_sha256": "c" * 64, "gold_sha256": "d" * 64,
              "case_set_sha256": support.digest(sorted(case_ids)),
              **{role + "_sha256": common.sha(raw) for role, raw in bodies.items()}}
    lock = {"schema_version": 1, "kind": "p8_semantic_input_lock", "binding": copy.deepcopy(pinned),
            "model": selected_model, "files": {role: {"path": filenames[role],
            "sha256": common.sha(raw)} for role, raw in bodies.items()}}
    lock_raw = support.canonical(lock)
    (root / "input_lock.json").write_bytes(lock_raw)
    pinned["input_lock_sha256"] = common.sha(lock_raw)
    header = {"schema_version": 1, "binding": pinned}
    records = {
        "input_lock": lock,
        "custody": {**header, "source_kind": "independent_custody_export", "split": "heldout",
                    "status": "clean", "development_overlap": 0, "case_ids": case_ids,
                    "audit_sha256": "e" * 64, "custodian": "synthetic contract control"},
        "execution": {**header, "model": selected_model, "run_id": "paper-control",
            "source_kind": "live_provider_export", "endpoint": "https://api.provider-contract.com/v1",
            "started_at": "2026-10-08T10:00:00Z", "ended_at": "2026-10-08T10:01:00Z",
            "requests": [{"request_id": "rq-" + str(i), "stage": "query",
                          "model_revision": selected_model["revision"], "request_sha256": "1" * 64,
                          "response_sha256": "2" * 64} for i in range(2)]},
        "attempts": {**header, "run_id": "paper-control", "rows": [
            {"attempt_id": profile + "-" + str(i), "case_id": cid, "profile": profile,
             "status": "success", "elapsed_us": 100, "provider_request_ids": [] if profile == "baseline"
             else ["rq-" + str(i)]} for profile in ["baseline", "semantic"] for i, cid in enumerate(case_ids)]},
        "costs": {**header, "currency": "USD", "items": [
            {"request_id": "rq-" + str(i), "basis": "reported", "amount": "0.10",
             "receipt_sha256": "3" * 64} for i in range(2)]},
        "budget": {**header, "run_id": "paper-control", "model": selected_model, "approved": True,
            "approval_id": "synthetic-approval", "approval_record_sha256": "4" * 64,
            "currency": "USD", "max_cost": "1.00", "max_requests": 2,
            "approved_at": "2026-10-08T09:00:00Z", "expires_at": "2026-10-08T11:00:00Z"},
        "quality": {**header, "method": "deterministic_gold_scores",
            "scoring_sha256": pinned["scoring_sha256"], "rows": [
            {"case_id": cid, "baseline": 0.5, "semantic": 0.75} for cid in case_ids]}}
    manifest = {"schema_version": 1, "run_id": "paper-control", "binding": pinned,
        "model": selected_model, "thresholds": {"min_cases": 2, "max_error_rate": 0, "min_mean_gain": 0.1},
        "records": {}}
    # Break mutable aliases so each record really has an independent declaration.
    records = {role: copy.deepcopy(value) for role, value in records.items()}
    manifest = copy.deepcopy(manifest)
    write_fixture(root, manifest, records)
    return manifest, records


def write_fixture(root, manifest, records):
    for role, value in records.items():
        raw = support.canonical(value)
        (root / (role + ".json")).write_bytes(raw)
        manifest["records"][role] = {"path": role + ".json", "sha256": common.sha(raw)}
    (root / "manifest.json").write_bytes(support.canonical(manifest))


class SemanticCertificationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.serial = 0
        self.manifest, self.records = paper_fixture(self.root)

    def output(self):
        self.serial += 1
        return self.root / ("result-" + str(self.serial))

    def evaluate(self, manifest=None, records=None):
        write_fixture(self.root, manifest if manifest is not None else self.manifest,
                      records if records is not None else self.records)
        out = self.output()
        return semantic.evaluate(self.root / "manifest.json", out), out

    def failed_check(self, result, name):
        self.assertNotEqual(result["exit_code"], 0)
        self.assertFalse(next(c for c in result["checks"] if c["name"] == name)["passed"])
        self.assertFalse(result["release_certified"])

    def test_complete_paper_metadata_passes_structure_but_cannot_certify(self):
        result, out = self.evaluate()
        self.assertEqual(result["structural_status"], "passed")
        self.assertEqual(result["status"], "blocked")
        self.assertEqual(result["exit_code"], 1)
        self.assertEqual(result["M4_semantic"], "blocked")
        self.assertEqual(result["blockers"], result["external_blockers"])
        self.assertEqual(len(result["external_blockers"]), 4)
        self.assertEqual(result["model_calls"], 0)
        self.assertEqual(result["network_calls"], 0)
        self.assertEqual(result["local_profile"], {"status": "not_assessed", "modified": False})
        self.assertFalse(result["quality_observation"]["model_benefit_certified"])
        self.assertFalse(result["quality_observation"]["scorer_execution_authenticated"])
        self.assertEqual(result["quality_observation"]["mean_delta"], 0.25)
        self.assertEqual(result["cost_summary"]["reported_total"], "0.20")
        self.assertEqual((out / "metadata/execution.json").read_bytes(), (self.root / "execution.json").read_bytes())
        if os.name == "posix":
            self.assertEqual((out / "metadata/execution.json").stat().st_mode & 0o077, 0)

    def test_missing_evidence_is_blocked_including_every_required_role(self):
        for role in sorted(semantic.ROLES):
            manifest = copy.deepcopy(self.manifest)
            manifest["records"].pop(role)
            (self.root / "manifest.json").write_bytes(support.canonical(manifest))
            result = semantic.evaluate(self.root / "manifest.json", self.output())
            self.assertEqual(result["structural_status"], "incomplete")
            self.failed_check(result, "present_" + role)

    def test_old_local_candidate_cannot_be_promoted(self):
        self.records["input_lock"]["kind"] = "p8_local_candidate"
        result, _ = self.evaluate()
        self.assertEqual(result["structural_status"], "blocked_local_candidate")
        self.assertFalse(result["local_profile"]["modified"])
        self.assertFalse(result["release_certified"])

    def test_candidate_actual_bytes_are_rechecked(self):
        for filename, check in [("candidate.bin", "actual_binary_bytes"),
                                ("config.json", "actual_config_bytes"), ("scoring.txt", "actual_scoring_bytes")]:
            original = (self.root / filename).read_bytes()
            (self.root / filename).write_bytes(original + b" ")
            self.failed_check(self.evaluate()[0], check)
            (self.root / filename).write_bytes(original)

    def test_corpus_query_gold_source_and_case_bindings_cannot_drift(self):
        for key in sorted(semantic.BINDING):
            records = copy.deepcopy(self.records)
            records["custody"]["binding"][key] = "0" * (40 if key == "source_commit" else 64)
            self.failed_check(self.evaluate(records=records)[0], "binding_custody")

    def test_lock_requires_exact_actual_hash_and_semantic_kind(self):
        records = copy.deepcopy(self.records)
        records["input_lock"]["kind"] = "fixture"
        result, _ = self.evaluate(records=records)
        self.failed_check(result, "semantic_input_lock_kind")
        self.failed_check(result, "input_lock_digest")

    def test_clean_heldout_metadata_is_cross_checked(self):
        for key, value in [("source_kind", "self_claim"), ("split", "dev"),
                           ("status", "dirty"), ("development_overlap", 1)]:
            records = copy.deepcopy(self.records)
            records["custody"][key] = value
            self.failed_check(self.evaluate(records=records)[0], "clean_heldout_declaration")

    def test_custody_inventory_duplicates_digests_and_minimums(self):
        records = copy.deepcopy(self.records)
        records["custody"]["case_ids"].append("case-0001")
        self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)
        manifest = copy.deepcopy(self.manifest)
        manifest["thresholds"]["min_cases"] = 3
        self.failed_check(self.evaluate(manifest=manifest)[0], "minimum_cases")
        records = copy.deepcopy(self.records)
        records["custody"]["case_ids"] = ["case-0002", "case-0001"]
        self.assertEqual(self.evaluate(records=records)[0]["structural_status"], "passed")

    def test_fake_local_http_and_loopback_provider_origins_block(self):
        for source, endpoint in [("fake", "https://api.provider-contract.com/v1"),
            ("local", "https://api.provider-contract.com/v1"), ("live_provider_export", "http://api.provider-contract.com"),
            ("live_provider_export", "https://localhost/v1"), ("live_provider_export", "https://127.0.0.1/v1"),
            ("live_provider_export", "https://192.168.1.3/v1"), ("live_provider_export", "https://[::1]/v1"),
            ("live_provider_export", "https://api.provider-contract.com/?key=secret")]:
            records = copy.deepcopy(self.records)
            records["execution"].update(source_kind=source, endpoint=endpoint)
            self.failed_check(self.evaluate(records=records)[0], "live_provider_origin_declaration")

    def test_record_model_run_and_observed_revision_must_match(self):
        records = copy.deepcopy(self.records)
        records["execution"]["requests"][0]["model_revision"] = "different-version"
        self.failed_check(self.evaluate(records=records)[0], "observed_revision_rq-0")
        records = copy.deepcopy(self.records)
        records["execution"]["model"]["revision"] = "different-version"
        self.failed_check(self.evaluate(records=records)[0], "execution_run_and_model")
        records = copy.deepcopy(self.records)
        records["execution"]["run_id"] = "different-run"
        self.failed_check(self.evaluate(records=records)[0], "execution_run_and_model")
        manifest = copy.deepcopy(self.manifest)
        manifest["model"]["revision"] = "latest"
        self.assertEqual(self.evaluate(manifest=manifest)[0]["exit_code"], 2)

    def test_errors_remain_in_all_attempt_denominator_and_missing_timing_is_not_zero(self):
        for status in sorted(semantic.STATUSES - semantic.SUCCESS):
            records = copy.deepcopy(self.records)
            records["attempts"]["rows"][2].update(status=status, elapsed_us=None)
            result, _ = self.evaluate(records=records)
            self.failed_check(result, "error_rate_semantic")
            self.failed_check(result, "measured_attempt_semantic-0")
            self.assertEqual(result["attempt_distributions"]["semantic"]["error_rate"], 0.5)
            self.assertEqual(result["attempt_distributions"]["semantic"]["all_attempts"], 2)
            self.assertEqual(result["attempt_distributions"]["semantic"]["status_counts"][status], 1)

    def test_no_match_is_terminal_success_but_not_quality_gain(self):
        self.records["attempts"]["rows"][2]["status"] = "no_match"
        self.records["quality"]["rows"][0]["semantic"] = 0.0
        result, _ = self.evaluate()
        self.assertEqual(result["attempt_distributions"]["semantic"]["error_rate"], 0)
        self.failed_check(result, "declared_mean_gain_threshold")

    def test_complete_pairs_unique_attempts_and_known_status_required(self):
        records = copy.deepcopy(self.records)
        records["attempts"]["rows"].pop()
        self.failed_check(self.evaluate(records=records)[0], "complete_paired_attempt_inventory")
        for edit in [lambda r: r.append(copy.deepcopy(r[0])), lambda r: r[0].update(status="ok"),
                     lambda r: r[0].update(elapsed_us=True), lambda r: r[0].update(elapsed_us=-1),
                     lambda r: r[0].update(case_id="unknown"), lambda r: r[0].update(profile=[]),
                     lambda r: r[0].update(provider_request_ids=["rq-0"])]:
            records = copy.deepcopy(self.records)
            edit(records["attempts"]["rows"])
            self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)

    def test_request_inventory_duplicates_and_success_response_are_checked(self):
        records = copy.deepcopy(self.records)
        records["attempts"]["rows"][2]["provider_request_ids"] = []
        self.failed_check(self.evaluate(records=records)[0], "query_request_inventory")
        records = copy.deepcopy(self.records)
        records["execution"]["requests"][0]["response_sha256"] = None
        self.failed_check(self.evaluate(records=records)[0], "successful_provider_response_semantic-0")
        records = copy.deepcopy(self.records)
        records["execution"]["requests"].append(copy.deepcopy(records["execution"]["requests"][0]))
        self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)

    def test_index_and_retry_costs_are_included_in_budget(self):
        for stage in ["index", "retry"]:
            records = copy.deepcopy(self.records)
            records["execution"]["requests"].append({"request_id": "extra-rq", "stage": stage,
                "model_revision": self.manifest["model"]["revision"], "request_sha256": "5" * 64,
                "response_sha256": None})
            if stage == "retry":
                records["attempts"]["rows"][2]["provider_request_ids"].append("extra-rq")
            result, _ = self.evaluate(records=records)
            self.failed_check(result, "all_request_costs_accounted")
            self.failed_check(result, "approved_request_limit")
            self.assertEqual(result["cost_summary"]["missing_requests"], 1)

    def test_estimated_unknown_and_missing_costs_do_not_become_reported(self):
        for basis, amount in [("estimated", "0.10"), ("unknown", None)]:
            records = copy.deepcopy(self.records)
            records["costs"]["items"][0].update(basis=basis, amount=amount, receipt_sha256=None)
            result, _ = self.evaluate(records=records)
            self.failed_check(result, "actual_reported_costs_required")
            self.failed_check(result, "approved_cost_limit")
            self.assertEqual(result["cost_summary"]["reported_total"], "0.10")
            if basis == "estimated":
                self.assertEqual(result["cost_summary"]["estimated_total"], "0.10")
            else:
                self.assertEqual(result["cost_summary"]["unknown_requests"], 1)
        records = copy.deepcopy(self.records)
        records["costs"]["items"].pop()
        self.failed_check(self.evaluate(records=records)[0], "all_request_costs_accounted")

    def test_cost_provenance_and_decimal_amounts_are_strict(self):
        for edit in [lambda i: i.update(basis="estimated"), lambda i: i.update(basis="unknown"),
                     lambda i: i.update(amount=0.1), lambda i: i.update(amount="-0.1"),
                     lambda i: i.update(amount="NaN"), lambda i: i.update(amount="1e-3"),
                     lambda i: i.update(receipt_sha256=None), lambda i: i.update(basis=[])]:
            records = copy.deepcopy(self.records)
            edit(records["costs"]["items"][0])
            self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)

    def test_approval_scope_window_and_caps_are_required(self):
        for key, value, check in [("approved", False, "approved_budget_scope"),
            ("run_id", "other-run", "approved_budget_scope"), ("currency", "EUR", "approved_budget_scope"),
            ("max_cost", "0.19", "approved_cost_limit"), ("max_requests", 1, "approved_request_limit"),
            ("approved_at", "2026-10-08T10:00:01Z", "approved_execution_window"),
            ("expires_at", "2026-10-08T10:00:59Z", "approved_execution_window")]:
            records = copy.deepcopy(self.records)
            records["budget"][key] = value
            self.failed_check(self.evaluate(records=records)[0], check)
        records = copy.deepcopy(self.records)
        records["budget"]["approved"] = 1
        self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)

    def test_quality_is_recomputed_requires_pairs_and_never_authenticates_scorer(self):
        records = copy.deepcopy(self.records)
        records["quality"]["rows"].pop()
        self.failed_check(self.evaluate(records=records)[0], "paired_quality_inventory")
        records = copy.deepcopy(self.records)
        records["quality"]["scoring_sha256"] = "0" * 64
        self.failed_check(self.evaluate(records=records)[0], "deterministic_gold_score_declaration")
        for edit in [lambda q: q.update(mean_delta=1.0), lambda q: q["rows"][0].update(semantic=True),
                     lambda q: q["rows"][0].update(semantic=2),
                     lambda q: q["rows"].append(copy.deepcopy(q["rows"][0]))]:
            records = copy.deepcopy(self.records)
            edit(records["quality"])
            self.assertEqual(self.evaluate(records=records)[0]["exit_code"], 2)

    def test_self_asserted_trust_override_is_rejected(self):
        for key in ["external_verified", "release_certified", "trusted_provider", "allow_fixture"]:
            manifest = copy.deepcopy(self.manifest)
            manifest[key] = True
            result, _ = self.evaluate(manifest=manifest)
            self.assertEqual(result["exit_code"], 2)
            self.assertFalse(result["release_certified"])

    def test_duplicate_nonfinite_oversize_and_malformed_json_are_retained_as_invalid(self):
        for raw in ['{"schema_version":1,"schema_version":1}', '{"score":NaN}',
                    '{"score":1e999}', "{" + " " * common.MAX_INPUT + "}"]:
            (self.root / "manifest.json").write_text(raw)
            out = self.output()
            result = semantic.evaluate(self.root / "manifest.json", out)
            self.assertEqual(result["exit_code"], 2)
            self.assertEqual(result, json.loads((out / "receipt.json").read_bytes()))

    def test_missing_drifted_symlink_and_traversal_inputs_fail_closed(self):
        for path in ["absent.json", "../custody.json", "link.json"]:
            manifest = copy.deepcopy(self.manifest)
            manifest["records"]["custody"]["path"] = path
            if path == "link.json":
                (self.root / path).symlink_to(self.root / "custody.json")
            (self.root / "manifest.json").write_bytes(support.canonical(manifest))
            self.assertEqual(semantic.evaluate(self.root / "manifest.json", self.output())["exit_code"], 2)
        write_fixture(self.root, self.manifest, self.records)
        (self.root / "costs.json").write_bytes((self.root / "costs.json").read_bytes() + b" ")
        self.assertEqual(semantic.evaluate(self.root / "manifest.json", self.output())["exit_code"], 2)

    def test_existing_output_is_never_overwritten(self):
        _, out = self.evaluate()
        before = (out / "receipt.json").read_bytes()
        with self.assertRaises(FileExistsError):
            semantic.evaluate(self.root / "manifest.json", out)
        self.assertEqual((out / "receipt.json").read_bytes(), before)

    def test_real_cli_blocks_absent_evidence_without_modifying_local_profile(self):
        marker = self.root / "local-profile.json"
        marker.write_text('{"release_certified":true,"scope":"local_fixture"}\n')
        before = marker.read_bytes()
        self.manifest["records"] = {}
        (self.root / "manifest.json").write_bytes(support.canonical(self.manifest))
        out = self.output()
        process = subprocess.run([sys.executable, str(Path(semantic.__file__)), "--manifest",
            str(self.root / "manifest.json"), "--output", str(out)], text=True, capture_output=True,
            env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}, timeout=10)
        self.assertEqual(process.returncode, 1)
        result = json.loads(process.stdout)
        self.assertEqual(result["status"], "blocked")
        self.assertEqual(result["M4_semantic"], "blocked")
        self.assertEqual(result, json.loads((out / "receipt.json").read_bytes()))
        self.assertEqual(marker.read_bytes(), before)


if __name__ == "__main__":
    unittest.main()
