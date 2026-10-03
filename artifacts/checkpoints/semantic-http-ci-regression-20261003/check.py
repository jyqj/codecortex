"""Bounded existing production tests; fail on empty/drifted discovery or execution."""
import argparse
import json
import os
from pathlib import Path
import re
import subprocess


DB = [
    "all_fields_keep_the_pinned_snapshot_across_a_committed_publication",
    "unwired_read_skips_semantic_counts_and_does_not_write",
    "unsupported_error_and_undefined_identity_results_are_never_unmoved",
    "normal_rename_checks_the_actual_open_file_without_rebuild_or_wal",
    "incarnation_validation_rejects_in_place_replacement_but_not_epoch_churn",
    "missing_count_table_errors_are_not_zero_or_a_partial_ready_snapshot",
]
STATUS = [
    "stale_wiring_metadata_cannot_claim_an_unattached_port_is_ready",
    "wired_status_requires_an_active_space_and_reports_publication_gaps",
    "attached_port_with_degradation_reports_degraded_state_and_reasons",
    "unattached_healthy_or_unreported_states_are_never_degraded",
    "query_network_authorization_is_separate_and_requires_live_encoder",
    "old_active_space_cannot_report_configured_new_model_ready",
    "real_worker_publication_between_status_reads_never_mixes_epochs",
    "ordinary_index_churn_returns_one_complete_observation_per_poll",
    "detectable_incarnation_change_retries_then_returns_only_the_new_observation",
    "identity_churn_is_bounded_and_never_reports_old_database_ready",
]
GROUPS = [
    ("db", ["-p", "cc-db", "--lib", "capability_read::tests::"],
     {"capability_read::tests::" + name for name in DB}),
    ("status", ["-p", "cc-server", "--features", "semantic-http", "--lib",
                "capability_status::tests::"],
     {"capability_status::tests::" + name for name in STATUS}),
    ("stdio", ["-p", "cc-server", "--features", "semantic-http", "--test",
               "p7_v11_ready_epoch_independent_review"],
     {"ready_snapshot_bounds_query_generation_without_claiming_quiescence"}),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, required=True)
    output = parser.parse_args().output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, CARGO_TERM_COLOR="never",
               P7_V11_READY_REVIEW_EVIDENCE_DIR=str(output / "stdio-observations"))
    receipts = []
    for label, args, expected in GROUPS:
        assert expected, "an empty expected set cannot prove coverage"
        for phase, extra in [("list", ["--list"]),
                             ("run", ["--show-output", "--test-threads=1"])]:
            command = ["cargo", "test", *args, "--locked", "--", *extra]
            print("COMMAND " + json.dumps(command), flush=True)
            result = subprocess.run(command, env=env, text=True,
                                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                    timeout=600)
            (output / f"{label}-{phase}.log").write_text(result.stdout)
            print(result.stdout, end="", flush=True)
            if result.returncode:
                raise SystemExit(result.returncode)
            if phase == "list":
                names = re.findall(r"^(.+): test$", result.stdout, re.MULTILINE)
            else:
                # Child-process/tracing FD output can interrupt libtest's progress
                # lines even with capture. --show-output ends with a canonical
                # successes name list; require that list AND the exact summary.
                successes = result.stdout.rsplit("\nsuccesses:\n", 1)[-1]
                successes = successes.split("\ntest result:", 1)[0]
                names = re.findall(r"^    (\S+)$", successes, re.MULTILINE)
            if set(names) != expected or len(names) != len(expected):
                raise SystemExit(f"{label}/{phase}: expected {sorted(expected)}, got {names}")
            if phase == "run" and not re.search(
                rf"test result: ok\. {len(expected)} passed; 0 failed; 0 ignored;", result.stdout
            ):
                raise SystemExit(f"{label}: missing exact nonempty success summary")
            receipts.append({"group": label, "phase": phase, "command": command,
                             "exit_code": result.returncode, "names": sorted(names),
                             "count": len(names)})
    (output / "commands.json").write_text(json.dumps(receipts, indent=2) + "\n")
    print("VERIFIED 17 existing tests (6 DB + 10 semantic-http status + 1 stdio)")


if __name__ == "__main__":
    main()
