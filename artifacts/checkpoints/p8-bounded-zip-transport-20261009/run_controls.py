"""Run only synthetic Python transport controls; never accepts an artifact URL."""
import contextlib
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

if sys.flags.optimize:
    raise SystemExit("optimized Python forbidden")
HERE = Path(__file__).resolve().parent
EXPECTED_BLOBS = {
    "verified_range.py": "039058250749aab034954d7db310d6ec4ca4647b",
    "zip_intake.py": "3708ef78539e3af92afe11752d622f244c4fa856",
    "test_verified_range.py": "b0c1d0ab1c2401110cd07ee272a51eb1394e59d0",
    "curl_transport.py": "4725f963a98f73c876422948b94e1dc628bb746b",
    "test_curl_transport.py": "bb6444f2c58e6331265eebc9ae6297dd18bcfb09"
}
EXPECTED_IDS = [
    "test_verified_range.RangeControls.test_initial_full_digest_length_and_status",
    "test_verified_range.RangeControls.test_deferred_initial_headers",
    "test_verified_range.RangeControls.test_cross_block_tail_and_bounded_read_all",
    "test_verified_range.RangeControls.test_lru_eviction_revalidates_and_stays_bounded",
    "test_verified_range.RangeControls.test_200_is_not_a_range",
    "test_verified_range.RangeControls.test_each_content_range_coordinate_is_exact",
    "test_verified_range.RangeControls.test_short_and_long_range_bodies",
    "test_verified_range.RangeControls.test_same_etag_changed_bytes_rejected_by_initial_block_sha",
    "test_verified_range.RangeControls.test_changed_etag_rejected",
    "test_verified_range.RangeControls.test_duplicate_header_content_length_and_encoding_rejected",
    "test_verified_range.RangeControls.test_request_total_bytes_and_deadline_budgets",
    "test_verified_range.RangeControls.test_negative_seek_and_oversized_read_rejected",
    "test_verified_range.ZipControls.test_stored_and_deflated_all_bytes_match_stdlib",
    "test_verified_range.ZipControls.test_descriptor_and_forced_local_zip64_via_stdlib",
    "test_verified_range.ZipControls.test_nested_no_whole_zip_buffer_and_expanded_identity",
    "test_verified_range.ZipControls.test_large_nested_stored_and_incompressible_deflate_bound_every_seek_read",
    "test_verified_range.ZipControls.test_corrupt_crc_rejected_even_when_corrupt_zip_digest_is_registered",
    "test_verified_range.ZipControls.test_duplicate_escape_directory_and_nonregular_rejected",
    "test_verified_range.ZipControls.test_encryption_and_unknown_compression_rejected",
    "test_verified_range.ZipControls.test_selected_file_and_combined_budgets_and_missing_member",
    "test_verified_range.ZipControls.test_original_seal_complete_population_and_mutation",
    "test_curl_transport.CurlTransportControls.test_origin_complete_bounded_chunks_exact_headers_and_metrics",
    "test_curl_transport.CurlTransportControls.test_range_exact_inclusive_bounds_if_match_and_full_response",
    "test_curl_transport.CurlTransportControls.test_origin_short_and_extra_are_rejected_and_reaped",
    "test_curl_transport.CurlTransportControls.test_range_wrong_status_short_and_extra_are_rejected",
    "test_curl_transport.CurlTransportControls.test_invalid_origin_authority_never_starts_curl",
    "test_curl_transport.CurlTransportControls.test_invalid_range_and_etag_never_start_curl",
    "test_curl_transport.CurlTransportControls.test_early_generator_abandonment_is_cleaned_by_context",
    "test_curl_transport.CurlTransportControls.test_second_origin_or_concurrent_curl_is_refused",
    "test_curl_transport.CurlTransportControls.test_malformed_folded_oversized_and_interim_headers_fail_closed",
    "test_curl_transport.CurlTransportControls.test_redirect_headers_are_not_followed_or_published",
    "test_curl_transport.CurlTransportControls.test_process_spawn_and_nonzero_errors_do_not_expose_URL",
    "test_curl_transport.CurlTransportControls.test_timeout_is_sanitized_and_owned_kill_is_bounded"
]

def blob(raw):
    return hashlib.sha1(b"blob " + str(len(raw)).encode() + b"\0" + raw).hexdigest()

def snapshot():
    result = {}
    for name, expected in EXPECTED_BLOBS.items():
        raw = (HERE / name).read_bytes()
        if blob(raw) != expected:
            raise ValueError("fixed control input changed: " + name)
        result[name] = dict(bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(), git_blob=expected)
    return result

def flatten(suite):
    for item in suite:
        if isinstance(item, unittest.TestSuite):
            yield from flatten(item)
        else:
            yield item.id()

def main():
    if len(sys.argv) != 2:
        raise SystemExit("exact one owned output directory required; no network/URL arguments")
    out = Path(sys.argv[1]).resolve()
    runner_temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    if out != runner_temp / "p8-bounded-zip-controls" or out.exists():
        raise SystemExit("refuse non-owned or existing control output")
    out.mkdir()
    started = time.monotonic()
    report = dict(status="incomplete_not_certified", actual_product_or_network_execution=False,
                  local_actual_artifact_reception=False, source_P_R_G_changed=False,
                  TODO_closed=0, TODO_remaining=29)
    exit_code = 2
    try:
        before = snapshot()
        head = subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip()
        if head != os.environ["GITHUB_SHA"] or os.environ["GITHUB_RUN_ATTEMPT"] != "1":
            raise ValueError("wrong actual controller commit or attempt")
        report.update(controller_commit=head, input_before=before, expected_test_ids=EXPECTED_IDS)
        suite = unittest.TestSuite()
        for name in ("test_verified_range", "test_curl_transport"):
            module = importlib.import_module(name)
            suite.addTests(unittest.defaultTestLoader.loadTestsFromModule(module))
        actual_ids = list(flatten(suite))
        if sorted(actual_ids) != sorted(EXPECTED_IDS) or len(actual_ids) != len(set(actual_ids)):
            raise ValueError("missing, duplicate or unexpected synthetic control")
        with (out / "tests.stdout.log").open("x") as stdout, (out / "tests.stderr.log").open("x") as stderr:
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                result = unittest.TextTestRunner(stream=stdout, verbosity=2, failfast=False).run(suite)
        after = snapshot()
        report.update(input_after=after, actual_test_ids=actual_ids, tests_run=result.testsRun,
                      failures=len(result.failures), errors=len(result.errors), skipped=len(result.skipped),
                      expected_failures=len(result.expectedFailures),
                      unexpected_successes=len(result.unexpectedSuccesses))
        if (before != after or result.testsRun != len(EXPECTED_IDS) or not result.wasSuccessful()
                or result.skipped or result.expectedFailures or result.unexpectedSuccesses):
            raise ValueError("synthetic controls failed or did not execute exact population")
        report["status"] = "passed_synthetic_controls_only"
        exit_code = 0
    except BaseException as error:
        report["error_type"] = type(error).__name__
        if isinstance(error, ValueError):
            report["error"] = str(error)
    finally:
        report.update(exit_code=exit_code, wall_seconds=time.monotonic() - started)
        (out / "report.json").write_text(json.dumps(report, sort_keys=True, indent=2) + "\n")
        print(json.dumps(report, sort_keys=True))
        # Only fixed synthetic fixtures run here; mirror their preserved files.
        for name, stream in (("tests.stdout.log", sys.stdout), ("tests.stderr.log", sys.stderr)):
            path = out / name
            body = path.read_text() if path.is_file() else "<not created>\n"
            stream.write("\nBEGIN P8_SYNTHETIC " + name + "\n")
            stream.write(body)
            stream.write("\nEND P8_SYNTHETIC " + name + "\n")
            stream.flush()
    return exit_code

if __name__ == "__main__":
    raise SystemExit(main())
