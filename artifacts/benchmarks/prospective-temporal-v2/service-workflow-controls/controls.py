#!/usr/bin/env python3
"""Actual temporary-Git service controls; no product/source/query execution."""
import ast
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
ROOT = Path(os.environ["GITHUB_WORKSPACE"])
OUT = Path(os.environ["RUNNER_TEMP"]) / "ptv2-service-controls"
OUT.mkdir(parents=True, exist_ok=False)
DRIVER_PATH = ROOT / "artifacts/benchmarks/prospective-temporal-v2/service-workflow/service_workflow.py"
spec = importlib.util.spec_from_file_location("actual_locked_service_driver", DRIVER_PATH)
driver = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = driver
spec.loader.exec_module(driver)

parsed = []
parse_paths = [driver.DRIVER, driver.RUST_PROBE, driver.BASE + "/implementation/temporal.py"]
for rel in parse_paths:
    path = ROOT / rel
    ast.parse(path.read_bytes(), filename=str(path))
    parsed.append({"path": rel, **driver.record(path)})
# The inline Python snippets in all three actual frozen workflows are parsed
# too. This does not claim a GitHub-run prepare/data/execute workflow.
for rel in driver.WORKFLOWS:
    rows = (ROOT / rel).read_text().splitlines()
    snippets = []
    start = None
    for idx, line in enumerate(rows):
        if "python3 -I -S - <<'PY'" in line:
            start = idx + 1
        elif start is not None and line.strip() == "PY":
            body = "\n".join(row[10:] for row in rows[start:idx]) + "\n"
            ast.parse(body, filename=rel + ":" + str(start + 1))
            snippets.append({"first_line": start + 1, "sha256": driver.sha(body.encode())})
            start = None
    if start is not None:
        raise ValueError("unterminated workflow Python heredoc")
    parsed.append({"path": rel, "workflow_record": driver.record(ROOT / rel), "inline_python": snippets})
driver.write(OUT / "actual-ast.json", {"status": "parsed_actual_python_not_measurement",
    "files": parsed, "driver": driver.record(DRIVER_PATH), "new_product_queries": 0})

git_log = []
def git(repo, *argv):
    result = subprocess.run(["git", "-C", str(repo), *argv],
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    git_log.append({"repo": repo.name, "argv": list(argv), "exit_code": result.returncode,
                    "stdout": result.stdout.decode("utf-8", "replace"),
                    "stderr": result.stderr.decode("utf-8", "replace")})
    if result.returncode:
        raise ValueError("temporary Git command failed: " + repr(argv))
    return result.stdout.decode().strip()

def commit(repo, message):
    git(repo, "add", "-A")
    git(repo, "commit", "-m", message)
    return git(repo, "rev-parse", "HEAD")

def fresh_repo(label):
    repo = OUT / "repos" / label
    repo.mkdir(parents=True)
    git(repo, "init", "--initial-branch=control")
    git(repo, "config", "user.name", "Invented Service Control")
    git(repo, "config", "user.email", "service-control@example.invalid")
    git(repo, "config", "core.autocrlf", "false")
    for rel in (*driver.LOGIC, *(driver.BASE + "/implementation/" + n for n in driver.HELPER_NAMES)):
        target = repo / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes((ROOT / rel).read_bytes())
        target.chmod(0o644)
    baseline = commit(repo, "freeze actual service source in invented temporary repository")
    contract = {"schema_version": 1, "frozen_logic_commit": baseline,
                "files": {rel: driver.record(repo / rel) for rel in driver.LOGIC}}
    driver.write(repo / driver.CONTROL, contract)
    lock = {"measurement": {"exact_commands": {
        "workflow_contract_sha256": driver.record(repo / driver.CONTROL)["sha256"]}}}
    driver.write(repo / driver.DATA / "locks.json", lock)
    head = commit(repo, "add invented metadata only; no questions or answers")
    return repo, baseline, head

class Controls(unittest.TestCase):
    def test_data_only_descendant_is_accepted(self):
        repo, baseline, head = fresh_repo("data-only")
        observed = driver.check_logic(repo, True)
        self.assertEqual(observed["utility_commit"], head)
        self.assertEqual(observed["contract"]["frozen_logic_commit"], baseline)
        self.assertEqual(driver.check_helpers(repo), repo / driver.BASE / "implementation")

    def test_code_change_then_revert_is_rejected(self):
        repo, _, _ = fresh_repo("code-and-revert")
        target = repo / driver.DRIVER
        original = target.read_bytes()
        target.write_bytes(original + b"\n# invented forbidden intermediate edit\n")
        commit(repo, "forbidden intermediate code edit")
        target.write_bytes(original)
        commit(repo, "restore exact current code bytes")
        self.assertEqual(target.read_bytes(), original)
        with self.assertRaisesRegex(ValueError, "post-seal change is not a declared data-only file"):
            driver.check_logic(repo, True)

    def test_wrong_contract_file_digest_is_rejected(self):
        repo, _, _ = fresh_repo("wrong-contract-map")
        path = repo / driver.CONTROL
        contract = driver.read(path)
        contract["files"][driver.DRIVER]["sha256"] = "0" * 64
        path.write_bytes(driver.encoded(contract))
        commit(repo, "invented wrong pinned code digest")
        with self.assertRaisesRegex(ValueError, "workflow logic changed after its seal"):
            driver.check_logic(repo, False)

    def test_extra_helper_file_is_rejected(self):
        repo, _, _ = fresh_repo("helper-extra")
        path = repo / driver.BASE / "implementation/unlisted.txt"
        path.write_text("invented closure drift\n")
        commit(repo, "invented extra helper file")
        with self.assertRaisesRegex(ValueError, "fixed six-file helper closure mismatch"):
            driver.check_helpers(repo)

    def test_wrong_data_seal_contract_digest_is_rejected(self):
        repo, _, _ = fresh_repo("wrong-data-seal")
        path = repo / driver.DATA / "locks.json"
        value = driver.read(path)
        value["measurement"]["exact_commands"]["workflow_contract_sha256"] = "1" * 64
        path.write_bytes(driver.encoded(value))
        commit(repo, "invented incorrect data seal binding")
        with self.assertRaisesRegex(ValueError, "data seal does not bind pre-author workflow contract"):
            driver.check_logic(repo, True)

    def test_executable_data_file_is_rejected(self):
        repo, _, _ = fresh_repo("executable-data")
        target = repo / driver.DATA / "ordinary.json"
        target.write_text("{}\n")
        target.chmod(0o755)
        commit(repo, "invented executable metadata file")
        with self.assertRaisesRegex(ValueError, "post-seal data cannot be executable/symlink"):
            driver.check_logic(repo, False)

if __name__ == "__main__":
    result = unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(Controls))
    driver.write(OUT / "git-commands.json", git_log)
    driver.write(OUT / "result.json", {"status": "unit_controls_passed" if result.wasSuccessful() else "unit_controls_failed",
        "tests_run": result.testsRun, "failures": len(result.failures), "errors": len(result.errors),
        "driver_record": driver.record(DRIVER_PATH), "actual_python_ast": "actual-ast.json",
        "new_product_queries": 0, "real_boltons_source_read": False, "fresh_question_or_gold_authored": False,
        "actual_prepare_data_freeze": "not_run", "formal_execute_or_replay": "not_run",
        "first_seal_or_task_acceptance": "not_decided"})
    print("P8_SERVICE_CONTROL_RESULT " + (OUT / "result.json").read_text())
    raise SystemExit(0 if result.wasSuccessful() else 1)
