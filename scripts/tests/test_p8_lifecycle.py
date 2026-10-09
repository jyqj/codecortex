"""Lifecycle evidence controls; fixture numbers are not performance observations."""
import copy
from contextlib import closing
import json
from pathlib import Path
import sqlite3
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_lifecycle as lifecycle


def status():
    return {"indexed_files": 2, "resolution_freshness": {"complete": True, "status": "ready", "index_epoch": 5},
            "diagnostics": {"retrieval": {"generation": {"index_epoch": 5, "incarnation": [1] * 16}},
                            "search_cache": {key: 0 for key in lifecycle.COUNTERS}}}


class LifecycleTests(unittest.TestCase):
    def test_replay_binary_must_match_actual_original_cargo_and_source_witness(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            binary = root / "replay"
            binary.write_bytes(b"synthetic non-executable receipt negative control only")
            source_executable = root / "cargo-target/release/p8-measurements"
            source_executable.parent.mkdir(parents=True)
            source_executable.write_bytes(binary.read_bytes())
            checkout = Path(lifecycle.__file__).resolve().parents[1]
            source = {"source_commit": "a" * 40, "manifest_sha256": "b" * 64}
            artifact = {"reason": "compiler-artifact", "manifest_path": str(checkout / "crates/cc-eval/Cargo.toml"),
                        "target": {"name": "p8-measurements", "kind": ["bin"],
                                   "src_path": str(checkout / "crates/cc-eval/src/bin/p8-measurements.rs")},
                        "executable": str(source_executable),
                        "features": [], "profile": {"test": False, "debug_assertions": False, "opt_level": "3"}}
            stdout, stderr, path = root / "cargo.jsonl", root / "cargo.stderr", root / "receipt.json"
            stdout.write_text(json.dumps(artifact) + "\n" + json.dumps({"reason": "build-finished", "success": True}) + "\n")
            stderr.write_text("")
            receipt = {"schema_version": 1, "build_exit_code": 0, "source_before": source, "source_after": source,
                       "binary_sha256": lifecycle.digest(binary), "artifact": artifact,
                       "copy_source": {"path": str(source_executable), "sha256": lifecycle.digest(source_executable)},
                       "build_command": ["cargo", "build", "--locked", "--release", "-p", "cc-eval", "--bin", "p8-measurements",
                                         "--target-dir", str(root / "cargo-target")],
                       "build_stdout": {"name": stdout.name, "sha256": lifecycle.digest(stdout)},
                       "build_stderr": {"name": stderr.name, "sha256": lifecycle.digest(stderr)}}
            path.write_text(json.dumps(receipt))
            product = {"source": source}
            verified = lifecycle.evaluator_identity(binary, path, product, release=True)
            self.assertEqual(verified["status"], "original_cargo_artifact_and_source_verified")
            for change in ({"build_exit_code": 1}, {"source_after": {}}, {"binary_sha256": "0" * 64},
                           {"artifact": {}}, {"build_command": ["cargo", "build", "--locked"]}):
                path.write_text(json.dumps({**receipt, **change}))
                with self.assertRaises(ValueError):
                    lifecycle.evaluator_identity(binary, path, product, release=True)
            path.write_text(json.dumps(receipt))
            stdout.write_text(stdout.read_text() + "\n")
            with self.assertRaisesRegex(ValueError, "Cargo log changed"):
                lifecycle.evaluator_identity(binary, path, product, release=True)
            for change in ({"manifest_path": "/another/checkout/crates/cc-eval/Cargo.toml"},
                           {"target": {**artifact["target"], "src_path": "/another/checkout/crates/cc-eval/src/bin/p8-measurements.rs"}},
                           {"executable": None}, {"executable": str(root / "wrong-executable")}):
                wrong = {**artifact, **change}
                stdout.write_text(json.dumps(wrong) + "\n" + json.dumps({"reason": "build-finished", "success": True}) + "\n")
                altered = {**receipt, "artifact": wrong,
                           "build_stdout": {"name": stdout.name, "sha256": lifecycle.digest(stdout)}}
                path.write_text(json.dumps(altered))
                with self.assertRaises(ValueError):
                    lifecycle.evaluator_identity(binary, path, product, release=True)
            stdout.write_text(json.dumps(artifact) + "\n" + json.dumps({"reason": "build-finished", "success": True}) + "\n")
            path.write_text(json.dumps(receipt))
            source_executable.write_bytes(b"actual Cargo copy source changed")
            with self.assertRaisesRegex(ValueError, "copy source"):
                lifecycle.evaluator_identity(binary, path, product, release=True)

    def test_full_raw_inventory_refuses_tampering_added_inputs_and_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            raw = root / "events.jsonl"
            raw.write_text('{"observed_cache":"miss"}\n')
            (root / "sessions").mkdir()
            (root / "sessions/rpc.jsonl").write_text('{"actual_response":true}\n')
            receipt = {"artifact_inventory": lifecycle.artifact_inventory(root)}
            lifecycle.verify_artifact_inventory(root, receipt)
            before = raw.read_bytes()
            raw.write_text('{"observed_cache":"hit"}\n')
            with self.assertRaisesRegex(ValueError, "inventory changed"):
                lifecycle.verify_artifact_inventory(root, receipt)
            raw.write_bytes(before)
            extra = root / "unrecorded.json"
            extra.write_text("{}")
            with self.assertRaises(ValueError):
                lifecycle.verify_artifact_inventory(root, receipt)
            extra.unlink()
            raw.unlink()
            raw.symlink_to(root / "sessions/rpc.jsonl")
            with self.assertRaisesRegex(ValueError, "nonregular"):
                lifecycle.verify_artifact_inventory(root, receipt)

    def test_only_direct_current_query_cache_deltas_classify(self):
        before = status()
        after = copy.deepcopy(before)
        after["diagnostics"]["search_cache"]["graph_misses"] = 1
        self.assertEqual(lifecycle.cache_observation(before, after)[0], "miss")
        after["diagnostics"]["search_cache"]["result_hits"] = 1
        self.assertEqual(lifecycle.cache_observation(before, after)[0], "miss")
        after = copy.deepcopy(before)
        after["diagnostics"]["search_cache"]["graph_hits"] = 1
        self.assertEqual(lifecycle.cache_observation(before, after)[0], "hit")
        for old, new in ((before, before), (after, before)):
            with self.assertRaises(ValueError):
                lifecycle.cache_observation(old, new)
        for changes in ({"graph_hits": 2}, {"graph_hits": 1, "graph_misses": 1},
                        {"graph_hits": 1, "result_misses": 1}, {"graph_misses": True}):
            invalid = copy.deepcopy(before)
            invalid["diagnostics"]["search_cache"].update(changes)
            with self.assertRaises(ValueError):
                lifecycle.cache_observation(before, invalid)

    def test_generation_and_missing_counter_cannot_become_a_cache_hit(self):
        before, after = status(), status()
        after["diagnostics"]["search_cache"]["graph_hits"] = 1
        after["diagnostics"]["retrieval"]["generation"]["incarnation"][0] = 2
        with self.assertRaises(ValueError):
            lifecycle.cache_observation(before, after)
        after = status()
        del after["diagnostics"]["search_cache"]["graph_misses"]
        with self.assertRaises(ValueError):
            lifecycle.cache_observation(before, after)

    def test_cpu_and_io_counter_gaps_remain_null(self):
        before = {"pid": 5, "user_cpu_ns": 100, "system_cpu_ns": 30,
                  "io": {"read_bytes": 0, "write_bytes": 0, "rchar": 100}}
        after = {"pid": 5, "user_cpu_ns": 140, "system_cpu_ns": 20,
                 "io": {"read_bytes": 0, "write_bytes": 20, "rchar": 80}}
        delta = lifecycle.usage_delta(before, after)
        self.assertEqual(delta["user_cpu_ns"], 40)
        self.assertIsNone(delta["system_cpu_ns"])
        self.assertEqual(delta["io"]["read_bytes"], 0)
        self.assertIsNone(delta["io"]["rchar"])
        self.assertIsNone(delta["io"]["wchar"])
        self.assertIsNone(lifecycle.usage_delta(before, {**after, "pid": 6}))

    def test_release_and_resource_bounds_cannot_be_reduced_to_smoke(self):
        plan = {"profile": "release", "query_samples": 400, "cold_samples": 30,
                "files": 32, "request_timeout_seconds": 30, "run_timeout_seconds": 3600,
                "artifact_budget_bytes": 256 * 1024 * 1024}
        self.assertEqual(lifecycle.validate_plan(plan), plan)
        for change in ({"query_samples": 199}, {"cold_samples": 29}, {"files": 1},
                       {"run_timeout_seconds": 7201}, {"query_samples": True},
                       {"artifact_budget_bytes": 1024}):
            with self.assertRaises(ValueError):
                lifecycle.validate_plan({**plan, **change})

    def test_owned_uniform_fixture_is_immutable_and_no_cache_is_present(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "project"
            manifest = lifecycle.make_fixture(root, 4, 10)
            self.assertEqual(len(manifest), 4)
            self.assertFalse((root / ".codecortex").exists())
            for number in range(12):
                text = (root / lifecycle.source_path(number, 4)).read_text()
                self.assertIn(f"def {lifecycle.symbol(number)}(", text)
            with self.assertRaises(FileExistsError):
                lifecycle.make_fixture(root, 4, 10)

    def test_physical_sqlite_bytes_do_not_double_count_logical_fts_pages(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            root = project / ".codecortex"
            root.mkdir()
            db = root / "index.sqlite3"
            with sqlite3.connect(db) as connection:
                connection.execute("CREATE TABLE files(id INTEGER)")
                connection.execute("INSERT INTO files VALUES(1)")
                connection.execute("CREATE VIRTUAL TABLE chunks_fts USING fts5(text)")
                connection.execute("INSERT INTO chunks_fts VALUES('source fixture')")
            observed = lifecycle.disk_objects(project)
            self.assertEqual(len(observed["objects"]), 1)
            self.assertEqual(observed["objects"][0]["component"], "shared")
            self.assertEqual(observed["objects"][0]["bytes"], db.stat().st_size)
            self.assertEqual(observed["database"]["integrity"], "ok")
            self.assertIn(observed["logical_sqlite"]["status"], ("observed", "unavailable"))
            self.assertNotIn("fts", [obj["component"] for obj in observed["objects"]])

    def test_physical_storage_keeps_actual_wal_and_shm_sidecars(self):
        with tempfile.TemporaryDirectory() as directory:
            project = Path(directory)
            root = project / ".codecortex"
            root.mkdir()
            database = root / "index.sqlite3"
            # The owned writer holds real WAL/SHM files for this accounting
            # control. It is not an observed product or performance sample.
            with closing(sqlite3.connect(database)) as connection:
                connection.execute("PRAGMA journal_mode=WAL")
                connection.execute("CREATE TABLE files(id INTEGER)")
                connection.execute("INSERT INTO files VALUES(1)")
                connection.commit()
                observed = lifecycle.disk_objects(project)
                actual = {str(path): path.stat().st_size for path in root.iterdir() if path.is_file()}
                self.assertEqual({obj["path"]: obj["bytes"] for obj in observed["objects"]}, actual)
                self.assertTrue(any(path.endswith("-wal") for path in actual))
                self.assertTrue(any(path.endswith("-shm") for path in actual))


if __name__ == "__main__":
    unittest.main()
