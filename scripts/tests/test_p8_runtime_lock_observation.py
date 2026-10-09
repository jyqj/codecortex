"""Counter and feature-binding controls; synthetic bytes are not native observations."""
import json
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_runtime_build as build
import p8_runtime_lock_observation as observation
from test_p8_runtime_evidence import ReceiptFixture, fixture_modules


def snapshot():
    metric = dict(attempts=0, acquired=0, poisoned=0, failed=0, would_block=0,
                  in_flight=0, elapsed_ns_total=0, elapsed_ns_max=0)
    return dict(schema_version=1, process_id=41, db_instance_id=1, created_instances=1,
                scope="process_lifetime_all_index_db_handles", sqlite_busy_wait_observed=False,
                coherent=True, overflowed=False, update_sequence=2,
                **{role: {name: dict(metric) for name in observation.METRICS}
                   for role in observation.ROLES})


def measured_pair():
    before, after = snapshot(), snapshot()
    after.update(db_instance_id=3, created_instances=3, update_sequence=100)
    for name in observation.METRICS:
        after["workload"][name].update(attempts=4, acquired=1, poisoned=1, failed=1,
                                       would_block=1, elapsed_ns_total=1999, elapsed_ns_max=999)
        after["observer"][name].update(attempts=2, acquired=2,
                                       elapsed_ns_total=701, elapsed_ns_max=601)
    return before, after


class LockObservationControls(unittest.TestCase):
    def test_retired_instances_outcomes_and_submicrosecond_values_are_retained(self):
        before, after = measured_pair()
        result = observation.summarize(before, after)
        self.assertEqual(result["created_instances_at_boundaries"], [1, 3])
        self.assertEqual(result["counters"]["workload"]["writer_mutex_acquire"]["elapsed_ns_total"], 1999)
        self.assertEqual(result["counters"]["observer"]["writer_mutex_acquire"]["elapsed_ns_total"], 701)
        self.assertEqual(result["counters"]["workload"]["writer_mutex_acquire"]["attempts"], 4)
        self.assertIsNone(result["counters"]["workload"]["writer_mutex_acquire"]["interval_max_ns"])

    def test_reset_overflow_incoherence_and_undrained_boundaries_are_rejected(self):
        for change in (lambda x: x.update(process_id=42),
                       lambda x: x.update(coherent=False),
                       lambda x: x.update(overflowed=True),
                       lambda x: x.update(update_sequence=1),
                       lambda x: x["workload"]["writer_mutex_acquire"].update(attempts=5, in_flight=1),
                       lambda x: x["workload"]["writer_mutex_acquire"].update(attempts=5),
                       lambda x: x["observer"]["read_pool_lock_acquire"].update(acquired=True)):
            before, after = measured_pair()
            change(after)
            with self.assertRaises(ValueError):
                observation.summarize(before, after)

    def test_lifetime_maximum_is_not_subtracted_as_interval_maximum(self):
        before, after = measured_pair()
        for value in (before, after):
            for role in observation.ROLES:
                for metric in value[role].values():
                    metric["attempts"] += 1
                    metric["acquired"] += 1
                    metric["elapsed_ns_total"] += 5000
                    metric["elapsed_ns_max"] = 5000
        result = observation.summarize(before, after)
        metric = result["counters"]["workload"]["writer_mutex_acquire"]
        self.assertEqual(metric["elapsed_ns_total"], 1999)
        self.assertEqual(metric["process_lifetime_max_ns_at_end"], 5000)
        self.assertIsNone(metric["interval_max_ns"])

    def test_failed_snapshot_response_is_kept_before_rejection(self):
        class Product:
            process = SimpleNamespace(pid=41)
            coherent = False
            def tool(self, name, args, timeout):
                self.call = (name, args, timeout)
                value = snapshot()
                value["coherent"] = self.coherent
                return value
        class Raw:
            def emit(self, kind, **value):
                self.row = dict(kind=kind, **value)
        product, raw = Product(), Raw()
        with self.assertRaises(ValueError):
            observation.capture(product, raw, "before_work", lambda: 3)
        self.assertFalse(raw.row["response"]["coherent"])
        self.assertEqual(product.call, ("status", {"aspect": "lock_observation"}, 30))
        product.coherent = True
        product.process = SimpleNamespace(pid=42)
        with self.assertRaisesRegex(ValueError, "another process"):
            observation.capture(product, raw, "before_work", lambda: 3)
        self.assertEqual(raw.row["owned_product_pid"], 42)
        self.assertEqual(raw.row["response"]["process_id"], 41)

    def test_retained_replay_rejects_duplicate_boundaries_or_changed_summary(self):
        before, after = measured_pair()
        expected = observation.summarize(before, after)
        rows = [dict(kind="db_lock_observation", phase="before_work", started_ns=1,
                     finished_ns=2, response=before, owned_product_pid=41),
                dict(kind="db_lock_observation", phase="after_work_and_sampler_drain",
                     started_ns=100, finished_ns=101, response=after, owned_product_pid=41)]
        rows.extend(dict(kind="operation", id=number, offered_ns=3, finished_ns=99)
                    for number in range(900))
        plan = dict(diagnostic_profile=build.LOCK_OBSERVATION_PROFILE, profile="mixed",
                    operations=900, files=1000, offer_interval_ms=500, concurrency=4)
        report = dict(diagnostic_profile=build.LOCK_OBSERVATION_PROFILE,
                      status="passed_observation", exit_code=0, db_lock_observation=expected,
                      owned_cleanup=dict(unfinished_work=0, sampler_stopped=True, product_stopped=True),
                      artifact_seal_status="sealed")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "build-evidence").mkdir()
            (root / "build-evidence/build-receipt.json").write_text(json.dumps({
                "diagnostic_profile": build.LOCK_OBSERVATION_PROFILE}))
            (root / "product").mkdir()
            (root / "product/process.json").write_text(json.dumps({"pid": 41}))
            (root / "db-lock-observation.json").write_text(json.dumps(expected))
            raw = root / "raw.jsonl"
            raw.write_text("".join(json.dumps(row) + "\n" for row in rows))
            self.assertEqual(observation.verify_retained(root, plan, report)["status"],
                             "original_counter_boundaries_verified")
            raw.write_text("".join(json.dumps(row) + "\n" for row in rows + [rows[1]]))
            with self.assertRaisesRegex(ValueError, "boundaries"):
                observation.verify_retained(root, plan, report)
            raw.write_text("".join(json.dumps(row) + "\n" for row in rows))
            (root / "product/process.json").write_text(json.dumps({"pid": 42}))
            with self.assertRaisesRegex(ValueError, "owned product PID"):
                observation.verify_retained(root, plan, report)
            (root / "product/process.json").write_text(json.dumps({"pid": 41}))
            (root / "db-lock-observation.json").write_text("{}")
            with self.assertRaisesRegex(ValueError, "summary differs"):
                observation.verify_retained(root, plan, report)

    def test_failed_archive_is_preserved_without_counter_success_or_native_replay(self):
        plan = dict(diagnostic_profile=build.LOCK_OBSERVATION_PROFILE, profile="mixed",
                    operations=900, files=1000, offer_interval_ms=500, concurrency=1)
        report = dict(diagnostic_profile=build.LOCK_OBSERVATION_PROFILE, status="failed", exit_code=2)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "build-evidence").mkdir()
            (root / "build-evidence/build-receipt.json").write_text(json.dumps({
                "diagnostic_profile": build.LOCK_OBSERVATION_PROFILE}))
            self.assertEqual(observation.verify_retained(root, plan, report)["status"],
                             "retained_failed_observation")
            report["exit_code"] = 0
            with self.assertRaisesRegex(ValueError, "success exit"):
                observation.verify_retained(root, plan, report)


class DiagnosticBuildBindingControls(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.fixture = ReceiptFixture(Path(temporary.name))

    def enable_diagnostic(self):
        fixture = self.fixture
        fixture.receipt["diagnostic_profile"] = build.LOCK_OBSERVATION_PROFILE
        fixture.receipt["build_command"] = build.command_for(fixture.target, True)
        for message in fixture.messages:
            if message.get("reason") == "compiler-artifact":
                message["features"] = [build.LOCK_OBSERVATION_FEATURE]
        fixture.resign_control()

    def verify_diagnostic(self):
        fixture = self.fixture
        with fixture_modules(fixture.root):
            return build.verify_receipt(fixture.root, fixture.out / "build-receipt.json",
                                        fixture.binaries, True)

    def test_default_and_feature_builds_cannot_cross_claim_the_other_profile(self):
        self.fixture.verify()
        with self.assertRaisesRegex(ValueError, "profiles must not be mixed"):
            self.verify_diagnostic()
        self.enable_diagnostic()
        self.verify_diagnostic()
        with self.assertRaisesRegex(ValueError, "profiles must not be mixed"):
            self.fixture.verify()

    def test_missing_feature_on_one_actual_artifact_is_rejected_even_when_resealed(self):
        self.enable_diagnostic()
        for message in self.fixture.messages:
            if message.get("target", {}).get("name") == "codecortex":
                message["features"] = []
        self.fixture.resign_control()
        with self.assertRaisesRegex(ValueError, "actual release"):
            self.verify_diagnostic()


if __name__ == "__main__":
    unittest.main()
