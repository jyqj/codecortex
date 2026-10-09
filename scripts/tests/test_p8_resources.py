"""Owned-process mapping contracts; fixture counters are not resource evidence."""
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import p8_resources as resources


class ResourcesTest(unittest.TestCase):
    def test_stat_parser_keeps_units_and_handles_comm_delimiters(self):
        fields = ["0"] * 22
        fields[0], fields[1], fields[11], fields[12] = "S", "100", "12", "3"
        fields[17], fields[19], fields[21] = "2", "400", "500"
        text = f"101 (name ) ( with spaces) {' '.join(fields)}"
        observed = resources._stat(text)
        self.assertEqual(observed["resident_pages"], 500)
        self.assertEqual(observed["user_ticks"], 12)
        self.assertEqual(observed["start_ticks"], 400)
        for bad in ("bad", text.replace(") S", ") Z"), text[:-3] + "-1"):
            with self.assertRaises((ValueError, IndexError)):
                resources._stat(bad)

    def test_namespace_status_rejects_conflicting_identity(self):
        text = "Pid:\t101\nPPid:\t100\nTgid:\t101\nNSpid:\t101 6\n"
        self.assertEqual(resources._status(text)["namespace_pids"], [101, 6])
        for bad in (text.replace("101 6", "102 6"), text + "Pid: 101\n",
                    text.replace("101 6", "101 0")):
            with self.assertRaises(ValueError):
                resources._status(bad)

    def test_invalid_pid_and_owner_are_rejected(self):
        for pid in (0, -1, True, "5"):
            with self.assertRaises(ValueError):
                resources.OwnedProcessProbe(pid)
        with self.assertRaises(ValueError):
            resources.OwnedProcessProbe(5, owner="unrelated")

    def fixture(self):
        temporary = tempfile.TemporaryDirectory()
        root = Path(temporary.name)
        namespace = root / "namespace"
        namespace.write_text("fixture namespace, not a kernel measurement")
        for host, parent, pid, children in ((100, 99, 5, "101"), (101, 100, 6, "102"),
                                             (102, 101, 7, "")):
            directory = root / str(host)
            (directory / "ns").mkdir(parents=True)
            (directory / "ns/pid").symlink_to(namespace)
            (directory / "exe").symlink_to(sys.executable)
            (directory / "status").write_text(
                f"Pid: {host}\nPPid: {parent}\nTgid: {host}\nNSpid: {host} {pid}\n")
            fields = ["0"] * 22
            fields[0], fields[1], fields[11], fields[12] = "S", str(parent), "12", "3"
            fields[17], fields[19], fields[21] = "1", str(host * 100), "500"
            (directory / "stat").write_text(f"{host} (fixture) {' '.join(fields)}")
            (directory / "io").write_text(
                "rchar: 100\nwchar: 200\nsyscr: 2\nsyscw: 3\nread_bytes: 0\nwrite_bytes: 0\ncancelled_write_bytes: 0\n")
            (directory / f"task/{host}").mkdir(parents=True)
            (directory / f"task/{host}/children").write_text(children)
        def directory(pid):
            return os.open(root / str(pid), os.O_RDONLY | os.O_DIRECTORY)
        def owner(_mode):
            fd = directory(100)
            return fd, resources._identity(fd)[0]
        return temporary, root, directory, owner

    def test_real_owned_tree_fixture_and_reused_pid_are_not_confused(self):
        temporary, root, directory, owner = self.fixture()
        with temporary, patch.object(resources, "_directory", directory), \
                patch.object(resources, "_owned_owner", owner):
            probe = resources.OwnedProcessProbe(6, sys.executable)
            result = probe.snapshot()
            self.assertEqual(result["status"], "observed")
            self.assertEqual(result["identity"]["host_pid"], 101)
            self.assertEqual(len(result["tree"]["members"]), 2)
            self.assertEqual(result["tree"]["resident_bytes"], 1000 * os.sysconf("SC_PAGE_SIZE"))
            self.assertEqual(result["tree"]["read_bytes"], 0)
            stat = root / "101/stat"
            stat.write_text(stat.read_text().replace("10100", "20100"))
            reused = probe.snapshot()
            self.assertEqual(reused["status"], "unavailable")
            self.assertIsNone(reused["process"])
            self.assertIn("identity changed", reused["unavailable_reason"])

    def test_missing_descendant_io_and_wrong_owner_never_lower_tree_total(self):
        temporary, root, directory, owner = self.fixture()
        with temporary, patch.object(resources, "_directory", directory), \
                patch.object(resources, "_owned_owner", owner):
            (root / "102/io").unlink()
            result = resources.OwnedProcessProbe(6, sys.executable).snapshot()
            self.assertEqual(result["status"], "partial")
            self.assertIsNone(result["tree"]["read_bytes"])
            self.assertIsNotNone(result["tree"]["resident_bytes"])
            status = root / "101/status"
            status.write_text(status.read_text().replace("PPid: 100", "PPid: 99"))
            invalid = resources.OwnedProcessProbe(6, sys.executable).snapshot()
            self.assertEqual(invalid["status"], "unavailable")
            self.assertIsNone(invalid["tree"]["resident_bytes"])

    def test_live_owned_child_is_attributed_or_explicitly_unavailable(self):
        # Some managed procfs mounts omit task/children. That is measured as
        # unavailable, never a test excuse for claiming process-tree coverage.
        child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(10)"])
        try:
            result = resources.OwnedProcessProbe(child.pid, sys.executable).snapshot()
            if result["process"] is not None:
                self.assertEqual(result["identity"]["namespace_pids"][-1], child.pid)
                self.assertGreater(result["process"]["resident_bytes"], 0)
            else:
                self.assertEqual(result["status"], "unavailable")
                self.assertIsNone(result["tree"]["resident_bytes"])
                self.assertTrue(result.get("unavailable_reason") or
                                result["tree"].get("unavailable_reasons"))
        finally:
            child.terminate()
            child.wait(timeout=5)


if __name__ == "__main__":
    unittest.main()
