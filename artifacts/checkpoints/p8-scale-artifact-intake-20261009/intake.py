#!/usr/bin/env python3
"""One-shot transport and orchestration of the unchanged b107 scale ZIP reviewer.

No workload, build, artifact extraction, source modification, or combine is
implemented here. A failed intake is not a reclassification of native evidence.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

sys.dont_write_bytecode = True
REPOSITORY = "jyqj/codecortex"
BRANCH = "task/p8-scale-artifact-intake-a217-20261009"
SOURCE = "275e8799d4947d297329073eaa3ca675d3fd0777"
RUN = 37896198208
ATTEMPT = 1
HELPER_SHA256 = "b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee"
MANIFEST = "593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83"
API = "https://api.github.com/repos/" + REPOSITORY
RESERVE = 128 * 1024 * 1024
MAX_API_BYTES = 16 * 1024 * 1024


class IntakeError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise IntakeError(message)


def utc():
    return datetime.now(timezone.utc).isoformat()


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def unique(pairs):
    out = {}
    for key, value in pairs:
        require(key not in out, "duplicate JSON key")
        out[key] = value
    return out


def decode(data):
    return json.loads(data, object_pairs_hook=unique,
                      parse_constant=lambda value: (_ for _ in ()).throw(IntakeError("nonfinite JSON")))


def write_new(path, value):
    with path.open("x", encoding="utf-8") as handle:
        json.dump(value, handle, sort_keys=True, ensure_ascii=False, indent=2, allow_nan=False)
        handle.write("\n")


def exact_head(root):
    return subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True).strip()


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def connection(url, headers):
    # A separate opener disables both automatic redirect and implicit proxy use.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
    try:
        return opener.open(urllib.request.Request(url, headers=headers), timeout=60)
    except urllib.error.HTTPError as error:
        if error.code in (301, 302, 303, 307, 308):
            return error
        status = error.code
        error.close()
        raise IntakeError("HTTP status " + str(status)) from None
    except Exception as error:
        # Never print urllib exception strings: they may embed a signed URL.
        raise IntakeError("network exception " + type(error).__name__) from None


def api_headers(token):
    return {"Authorization": "Bearer " + token,
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "User-Agent": "codecortex-fixed-scale-artifact-intake"}


def api_json(suffix, token, output):
    require(re.fullmatch(r"/actions/(runs/\d+(/artifacts\?per_page=100&page=\d+)?|artifacts/\d+)", suffix),
            "unrecognized constructed API path")
    with connection(API + suffix, api_headers(token)) as response:
        require(response.status == 200, "metadata API did not return 200")
        data = response.read(MAX_API_BYTES + 1)
    require(len(data) <= MAX_API_BYTES, "metadata response exceeds transport limit")
    require(token.encode() not in data, "metadata response contains credential bytes")
    # Preserve original response bytes before parsing; no response headers are saved.
    with output.open("xb") as handle:
        handle.write(data)
    return decode(data)


def storage_url(url):
    parsed = urllib.parse.urlsplit(url)
    host = (parsed.hostname or "").lower()
    allowed = (host.endswith(".blob.core.windows.net")
               or host.endswith(".actions.githubusercontent.com")
               or re.fullmatch(r"[a-z0-9.-]+\.s3(?:[.-][a-z0-9-]+)?\.amazonaws\.com", host))
    require(parsed.scheme == "https" and parsed.port in (None, 443)
            and parsed.username is None and parsed.password is None
            and not parsed.fragment and bool(allowed), "unexpected artifact storage redirect")
    return url


def metadata_identity(value):
    return {key: value[key] for key in ("id", "name", "size_in_bytes", "digest")} | {
        "run_id": value["workflow_run"]["id"],
        "source": value["workflow_run"]["head_sha"]}


def check_metadata(value, frozen):
    require(metadata_identity(value) == metadata_identity(frozen),
            "official artifact differs from frozen ID/name/size/digest/run/source")
    require(value.get("expired") is False, "original artifact expired")
    require(value["workflow_run"].get("repository_id") == 1249213794
            and value["workflow_run"].get("head_repository_id") == 1249213794,
            "artifact repository differs")
    require(type(value["size_in_bytes"]) is int and value["size_in_bytes"] > 0,
            "invalid ZIP size")
    require(re.fullmatch(r"sha256:[0-9a-f]{64}", value["digest"]), "invalid ZIP digest")


def check_run(value):
    require(value["id"] == RUN and value["head_sha"] == SOURCE
            and value["run_attempt"] == ATTEMPT, "original run/attempt/source differs")
    require(value["repository"]["full_name"] == REPOSITORY
            and value["head_repository"]["full_name"] == REPOSITORY,
            "original run repository differs")


def download_original(metadata, token, destination):
    size = metadata["size_in_bytes"]
    require(shutil.disk_usage(destination.parent).free >= size + RESERVE,
            "insufficient transport space; original reserve unchanged")
    require(not destination.exists(), "original ZIP destination exists")
    partial = destination.with_suffix(".zip.partial")
    require(not partial.exists(), "partial download destination exists")
    url = API + "/actions/artifacts/" + str(metadata["id"]) + "/zip"
    headers = api_headers(token)
    started = time.monotonic()
    redirects = 0
    response = connection(url, headers)
    try:
        while response.status in (301, 302, 303, 307, 308):
            require(redirects < 5, "artifact storage redirect limit exceeded")
            location = response.headers.get("Location")
            response.close()
            require(location is not None, "artifact redirect lacks Location")
            # No signed Location or query is logged or written to an artifact.
            url = storage_url(urllib.parse.urljoin(url, location))
            headers = {"User-Agent": "codecortex-fixed-scale-artifact-intake"}
            redirects += 1
            response = connection(url, headers)  # deliberately no Authorization
        require(response.status == 200, "artifact body did not return 200")
        total = 0
        sha = hashlib.sha256()
        with partial.open("xb") as handle:
            while True:
                require(time.monotonic() - started <= 900, "artifact transport deadline exceeded")
                block = response.read(1024 * 1024)
                if not block:
                    break
                total += len(block)
                require(total <= size, "artifact body exceeded official size")
                sha.update(block)
                handle.write(block)
        require(total == size, "artifact body size mismatch")
        require("sha256:" + sha.hexdigest() == metadata["digest"], "artifact body SHA mismatch")
        partial.rename(destination)
        return {"status": "verified_original_zip", "bytes": total,
                "sha256": sha.hexdigest(), "storage_redirect_count": redirects,
                "authorization_sent_only_to_constructed_github_api": True}
    finally:
        response.close()


def emit_original_review(path):
    if not path.is_file():
        return None
    require(not path.is_symlink() and path.stat().st_size <= 8 * 1024 * 1024,
            "review JSON missing or oversized")
    data = path.read_bytes()
    parsed = decode(data)
    # Original review content, not a newly calculated acceptance report.
    print("ORIGINAL_HELPER_REVIEW " + json.dumps(
        {"path": str(path), "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
         "review": parsed}, sort_keys=True, ensure_ascii=False, allow_nan=False), flush=True)
    return parsed


def call_helper(command, label, output, review_path):
    env = os.environ.copy()
    for name in ("GH_TOKEN", "GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN"):
        env.pop(name, None)
    stdout = output / (label + ".stdout.log")
    stderr = output / (label + ".stderr.log")
    result = {"command": command, "started_utc": utc(), "deadline_seconds": 900,
              "measurements_started": False, "returncode": None, "timed_out": False}
    with stdout.open("xb") as so, stderr.open("xb") as se:
        child = subprocess.Popen(command, stdout=so, stderr=se, env=env, start_new_session=True)
        try:
            try:
                result["returncode"] = child.wait(timeout=900)
            except subprocess.TimeoutExpired:
                result["timed_out"] = True
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    result["returncode"] = child.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    result["returncode"] = child.wait()
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGKILL)
                result["returncode"] = child.wait()
    result.update(finished_utc=utc(), stdout_sha256=digest(stdout), stderr_sha256=digest(stderr))
    write_new(output / (label + ".execution.json"), result)
    print("ORIGINAL_HELPER_COMMAND " + json.dumps(result, sort_keys=True), flush=True)
    review = emit_original_review(review_path)
    return result, review


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--admission-root", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--execution-sha", required=True)
    args = parser.parse_args()
    # Never merge with an existing state/output, including on a repeated attempt.
    output = args.output.resolve()
    require(not output.exists(), "exclusive output already exists")
    output.mkdir(parents=True)
    receipt = {"schema": "p8-existing-scale-artifact-intake-result-v1",
               "started_utc": utc(), "status": "failed", "execution_sha": args.execution_sha,
               "source": SOURCE, "run_id": RUN, "run_attempt": ATTEMPT,
               "accepted_artifact_ids": [], "helper_invocations": [],
               "compile": False, "measurement_launch": False, "combine": False,
               "formal_task_completion": False, "task_statuses_changed": False,
               "done": 163, "remaining": 29}
    code = 1
    try:
        admission = args.admission_root.resolve(strict=True)
        source = args.source_root.resolve(strict=True)
        require(re.fullmatch(r"[0-9a-f]{40}", args.execution_sha), "full execution SHA required")
        require(exact_head(admission) == args.execution_sha, "admission checkout HEAD differs")
        require(exact_head(source) == SOURCE, "tested checkout HEAD differs")
        require(os.environ.get("GITHUB_REPOSITORY") == REPOSITORY
                and os.environ.get("GITHUB_EVENT_NAME") == "create"
                and os.environ.get("GITHUB_REF") == "refs/heads/" + BRANCH
                and os.environ.get("GITHUB_RUN_ATTEMPT") == "1", "one-shot creation identity differs")
        root = admission / "artifacts/checkpoints/p8-scale-artifact-intake-20261009"
        require(Path(__file__).resolve() == root / "intake.py", "driver path differs")
        plan_path = root / "plan.json"
        plan = decode(plan_path.read_bytes())
        require(plan["source"] == SOURCE and plan["run_id"] == RUN
                and plan["run_attempt"] == ATTEMPT and plan["creation_branch"] == BRANCH,
                "plan identity differs")
        require(plan["source_manifest_sha256"] == MANIFEST
                and plan["source_input_count"] == 1089, "expected source inventory differs")
        require(plan["limits"]["helper_reserved_free_bytes"] == RESERVE
                and plan["limits"]["helper_expanded_bytes"] == 600 * 1024 * 1024,
                "original helper bounds differ")
        helper = admission / plan["helper"]["path"]
        require(helper.is_file() and not helper.is_symlink()
                and helper.stat().st_size == 19399 and digest(helper) == HELPER_SHA256,
                "frozen original helper bytes differ")
        require(plan["helper"]["sha256"] == HELPER_SHA256, "helper plan differs")
        selected = [plan["build"]] + plan["shards"]
        require(1 <= len(plan["shards"]) <= 5
                and len({x["id"] for x in selected}) == len(selected), "invalid frozen subset")
        require(plan["build"]["id"] == 11600309282
                and plan["build"]["size_in_bytes"] == 9218801
                and plan["build"]["digest"] == "sha256:9c13733e1bdb207301920f3dddb0bde4dcab278b31e4cdfd0dc8bc1f3c75ab67",
                "original build pin differs")
        for item in plan["shards"]:
            require(re.fullmatch(r"p8-scale-shard-(1000|5000|10000|50000|100000)-0-37896198208",
                                 item["name"]), "only frozen rep0 shards allowed")
        receipt.update(helper_sha256=digest(helper), driver_sha256=digest(Path(__file__)),
                       plan_sha256=digest(plan_path), selected_artifact_ids=[x["id"] for x in selected])
        api_dir = output / "official-api"
        originals = output / "originals"
        commands = output / "helper-logs"
        for directory in (api_dir, originals, commands):
            directory.mkdir()
        token = os.environ.pop("GH_TOKEN", "")
        require(bool(token), "missing Actions read token")
        before_run = api_json("/actions/runs/" + str(RUN), token, api_dir / "run-before.json")
        check_run(before_run)
        collection = {}
        expected_total = None
        for page in range(1, 21):
            data = api_json("/actions/runs/" + str(RUN) + "/artifacts?per_page=100&page=" + str(page),
                            token, api_dir / ("artifacts-page-" + str(page) + ".json"))
            require(type(data.get("artifacts")) is list, "invalid artifacts page")
            total = data.get("total_count")
            require(type(total) is int and total >= 0, "invalid artifact total_count")
            if expected_total is None:
                expected_total = total
            require(total == expected_total, "artifact collection changed during pagination")
            for item in data["artifacts"]:
                require(item["id"] not in collection, "artifact pagination repeated an ID")
                collection[item["id"]] = item
            if not data["artifacts"]:
                break
        else:
            raise IntakeError("artifact pagination exceeded bounded transport")
        require(len(collection) == expected_total, "artifact pagination did not cover total_count")
        receipt["official_artifact_pages"] = page
        receipt["official_artifact_total_count"] = expected_total
        for frozen in selected:
            require(frozen["id"] in collection, "frozen artifact absent from complete collection")
            check_metadata(collection[frozen["id"]], frozen)
            directory = originals / str(frozen["id"])
            directory.mkdir()
            metadata_path = directory / "artifact-metadata.json"
            meta = api_json("/actions/artifacts/" + str(frozen["id"]), token, metadata_path)
            check_metadata(meta, frozen)
            archive = directory / (str(frozen["id"]) + ".zip")
            transport = download_original(meta, token, archive)
            write_new(directory / "zip-transport-verification.json", transport)
        after_run = api_json("/actions/runs/" + str(RUN), token, api_dir / "run-after-download.json")
        check_run(after_run)
        token = ""  # never passed to the helper or included in receipts
        state = output / "state"
        require(not state.exists(), "new helper state already exists")
        build_dir = originals / str(plan["build"]["id"])
        command = [sys.executable, "-B", str(helper), "init", "--root", str(source),
                   "--source", SOURCE, "--run-id", str(RUN), "--state", str(state),
                   "--archive", str(build_dir / (str(plan["build"]["id"]) + ".zip")),
                   "--metadata", str(build_dir / "artifact-metadata.json")]
        result, review = call_helper(command, "init", commands, state / "initialization-review.json")
        receipt["helper_invocations"].append(result)
        require(result["returncode"] == 0 and not result["timed_out"] and review is not None
                and review.get("status") == "passed_fixed_source_build_admission",
                "original build helper did not admit")
        require(review["source_manifest_sha256"] == MANIFEST
                and review["input_count"] == 1089 and review["source_commit"] == SOURCE,
                "actual initialized inventory differs")
        receipt["actual_initialized_source_manifest_sha256"] = review["source_manifest_sha256"]
        receipt["actual_initialized_source_input_count"] = review["input_count"]
        failed = False
        for frozen in plan["shards"]:
            directory = originals / str(frozen["id"])
            command = [sys.executable, "-B", str(helper), "shard", "--state", str(state),
                       "--archive", str(directory / (str(frozen["id"]) + ".zip")),
                       "--metadata", str(directory / "artifact-metadata.json")]
            result, review = call_helper(command, "shard-" + str(frozen["id"]), commands,
                                         state / "shards" / str(frozen["id"]) / "review.json")
            receipt["helper_invocations"].append(result)
            accepted = (result["returncode"] == 0 and not result["timed_out"] and review is not None
                        and review.get("status") == "passed_original_validate_shard")
            if accepted:
                receipt["accepted_artifact_ids"].append(frozen["id"])
            else:
                failed = True
        require(exact_head(source) == SOURCE and exact_head(admission) == args.execution_sha
                and digest(helper) == HELPER_SHA256
                and digest(plan_path) == receipt["plan_sha256"]
                and digest(Path(__file__)) == receipt["driver_sha256"], "immutable inputs changed")
        require(not failed, "one or more original shard reviews failed")
        receipt["status"] = "passed_frozen_existing_subset_only"
        code = 0
    except BaseException as error:
        receipt["error_type"] = type(error).__name__
        receipt["error"] = str(error) if isinstance(error, IntakeError) else "details retained only in applicable bounded child logs"
        receipt["scope_note"] = "An intake failure is not a changed result of the original native measurement."
    finally:
        receipt["finished_utc"] = utc()
        receipt["exit_code"] = code
        write_new(output / "intake-receipt.json", receipt)
        print("INTAKE_RECEIPT " + json.dumps(receipt, sort_keys=True, ensure_ascii=False), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
