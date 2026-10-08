#!/usr/bin/env python3
"""Recheck retained public DEV runs; do not rerun retrieval or rescore results."""

import argparse
from datetime import datetime, timezone
from pathlib import Path, PurePosixPath
import sys
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "scripts"))

import p8_compat as compat
import p8_corpus_audit as corpus


ARCHIVE = ROOT / "artifacts/checkpoints/p8-next-ten-20261008/compat/control-runs.tar.gz"
ARCHIVE_SHA256 = "31366e8130b97d5ee092e17b48dda60505a17dde4f5401d490daee71cb9214a4"
PREFIX = "p8-compat-control-ebacee/"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    output = compat.path(args.output, exists=False)
    compat.require(not output.exists(), "retain earlier receipts")
    archive_record = compat.file_record(ARCHIVE)
    compat.require(archive_record["sha256"] == ARCHIVE_SHA256, "historical archive identity drift")
    blobs, admission = corpus.load_inputs(ROOT)
    locked_suites = {}
    for entry in corpus.suite_entries(admission["repo_results"]["express"]):
        ref, filename = corpus.split_entry(entry)
        suite = blobs.json(entry)
        profile = {v: k for k, v in compat.PROFILES.items()}[suite["scoring"]]
        query_entry = ref + ":" + str(PurePosixPath(filename).parent / suite["queries"])
        queries = corpus.query_rows(blobs.read(query_entry))
        locked_suites[profile] = {"suite": suite, "query_rows": len(queries),
                                  "query_snapshot": queries, "backend_kind": "rg"}

    checked_runs = []
    with tempfile.TemporaryDirectory(prefix="p8-retained-compat-check-") as temporary:
        root = Path(temporary)
        with tarfile.open(ARCHIVE, "r:gz") as archive:
            members = archive.getmembers()
            compat.require(len(members) <= 1000 and len({m.name for m in members}) == len(members),
                           "archive count or duplicate entry")
            compat.require(sum(m.size for m in members) <= compat.MAX_RAW, "archive byte budget")
            for member in members:
                compat.name(member.name)
                chosen = any(member.name.startswith(PREFIX + run + "/") for run in ("run-b", "run-c"))
                if not chosen:
                    continue
                compat.require(member.isfile() and member.size <= compat.MAX_FILE, "nonregular or oversized archive member")
                target = root / member.name
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.extractfile(member) as source, target.open("xb") as destination:
                    raw = source.read(compat.MAX_FILE + 1)
                    compat.require(len(raw) == member.size, "truncated archive member")
                    destination.write(raw)
            comparison_member = archive.getmember(PREFIX + "compare-bc/comparison-receipt.json")
            compat.require(comparison_member.isfile() and comparison_member.size <= compat.MAX_FILE,
                           "invalid retained comparison receipt")
            with archive.extractfile(comparison_member) as source:
                comparison = corpus.parse_json(source.read(compat.MAX_FILE + 1))
        for run in ("run-b", "run-c"):
            receipt = compat.load_run(root / PREFIX / run)
            compat.require(receipt["identity"]["dataset"] == "public-dev-control"
                           and receipt["identity"]["backend"]["kind"] == "rg", "unexpected historical control identity")
            for profile, suite in locked_suites.items():
                result = compat.check_run(root / PREFIX / run / profile, suite, receipt["exit_code"])
                checked_runs.append({"run": run, "profile": profile, "gate": result["gate"],
                                     "metrics": result["metrics"], "adapter": result["adapter"],
                                     "original_evaluator_source_sha": receipt["identity"]["evaluator_source_sha"],
                                     "original_evaluator_sha256": receipt["identity"]["evaluator_sha256"]})
    compat.require(compat.file_record(ARCHIVE) == archive_record, "historical archive changed")
    record = {"schema_version": 1, "status": "passed_retained_public_control_identity_checks",
              "observed_at_utc": datetime.now(timezone.utc).isoformat(),
              "scope": "structural revalidation of retained public DEV artifacts against frozen input blobs",
              "source_head": corpus.git(ROOT, "rev-parse", "HEAD").decode().strip(),
              "source_files_sha256": {str(p.relative_to(ROOT)): compat.file_record(p)["sha256"]
                                      for p in (Path(__file__), ROOT / "scripts/p8_compat.py",
                                                ROOT / "scripts/p8_corpus_audit.py")},
              "archive": {"path": str(ARCHIVE.relative_to(ROOT)), **archive_record},
              "runs": checked_runs,
              "retained_measured_requests_rechecked": sum(r["metrics"]["measured_rows"] for r in checked_runs),
              "original_comparison": {"status": comparison["status"], "exit_code": comparison["exit_code"]},
              "fresh_retrieval_requests": 0, "evaluator_executed": False, "rescored": False,
              "original_archive_modified": False, "holdout_body_reads": 0,
              "external_targets_completed": [], "release_certified": False,
              "original_p8_tasks_completed": []}
    output.parent.mkdir(parents=True, exist_ok=True)
    compat.write_json(output, record)
    print(compat.canonical(record).decode(), end="")


if __name__ == "__main__":
    main()
