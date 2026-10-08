#!/usr/bin/env python3
"""Read fixed Actions archives and emit a preflight; never approve a task."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import urllib.error
import urllib.parse
import urllib.request
import zipfile

def encoded(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n").encode()

def sha(data):
    return hashlib.sha256(data).hexdigest()

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None

def authenticated(url, token):
    return urllib.request.Request(url, headers={
        "Authorization": "Bearer " + token,
        "Accept": "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        "User-Agent": "codecortex-fixed-artifact-audit"
    })

def copy_archive(url, token, destination, expected):
    opener = urllib.request.build_opener(NoRedirect())
    try:
        response = opener.open(authenticated(url, token), timeout=60)
    except urllib.error.HTTPError as error:
        if error.code != 302:
            raise
        location = error.headers.get("Location")
        error.close()
        assert location and urllib.parse.urlsplit(location).scheme == "https", "invalid artifact redirect"
        # The artifact host receives no GitHub token.
        response = urllib.request.urlopen(location, timeout=60)
    count = 0
    digest = hashlib.sha256()
    with response, destination.open("xb") as output:
        while True:
            block = response.read(1048576)
            if not block:
                break
            count += len(block)
            assert count <= expected["bytes"], "archive exceeds fixed size"
            digest.update(block)
            output.write(block)
    assert count == expected["bytes"], "archive byte count drift"
    assert digest.hexdigest() == expected["sha256"], "archive SHA-256 drift"
    return count

def inspect_archive(path, expected, output):
    members = []
    with zipfile.ZipFile(path) as archive:
        names = archive.namelist()
        assert len(names) == len(set(names)), "duplicate ZIP paths"
        for info in archive.infolist():
            pure = PurePosixPath(info.filename)
            assert not pure.is_absolute() and ".." not in pure.parts and "\\" not in info.filename
            assert not info.is_dir(), "unexpected directory entry"
            digest = hashlib.sha256()
            size = 0
            with archive.open(info) as stream:
                while True:
                    block = stream.read(1048576)
                    if not block:
                        break
                    size += len(block)
                    digest.update(block)
            assert size == info.file_size
            members.append({"path":info.filename, "bytes":size, "sha256":digest.hexdigest()})
        result = {"kind":expected["kind"], "artifact_id":expected["id"],
                  "archive_sha256":expected["sha256"], "archive_bytes":path.stat().st_size,
                  "member_count":len(members), "expanded_bytes":sum(x["bytes"] for x in members),
                  "semantic_acceptance":"not_granted; inventory only"}
        for candidate in ("source-before.json", "source-after.json", "runner-source-before.json"):
            if candidate in names:
                value = json.loads(archive.read(candidate))
                result[candidate] = {k:v for k,v in value.items() if k != "inputs"}
        if expected["kind"] == "closeout":
            events = [json.loads(line) for line in archive.read("fault-lifecycle/events.jsonl").splitlines()]
            result["event_count"] = len(events)
            result["event_kinds"] = dict(Counter(e["kind"] for e in events))
            result["mcp_request_count"] = sum(e["kind"] == "mcp_request" for e in events)
            result["mcp_response_count"] = sum(e["kind"] == "mcp_response" for e in events)
            result["events_sha256"] = sha(archive.read("fault-lifecycle/events.jsonl"))
            result["phase_events"] = [e for e in events if e["kind"] in {
                "phase", "product_exit", "real_deadline_elapsed"}]
            for name in ("semantic-fault-matrix.log", "db-rebuild-faults.log", "http-runtime.log"):
                result[name + ":test_results"] = [
                    line for line in archive.read(name).decode().splitlines()
                    if line.startswith("test result:") or
                    ("opportunistic" in line and ("Running " in line or "... ok" in line))
                ]
        inventory = {"schema_version":1, "artifact":expected, "summary":result, "members":members}
        (output / (expected["kind"] + "-inventory.json")).write_bytes(encoded(inventory))
        return result

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--downloads", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--kinds", nargs="+")
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_bytes())
    assert manifest["repository"] == "jyqj/codecortex"
    assert manifest["source_commit"] == "ffdc6f0f97db78cc25a6c026904e7c2adde05d14"
    token = os.environ["GH_TOKEN"]
    args.downloads.mkdir(parents=True, exist_ok=False)
    args.output.mkdir(parents=True, exist_ok=False)
    rows = []
    selected = set(args.kinds) if args.kinds else {x["kind"] for x in manifest["artifacts"]}
    assert selected and selected <= {x["kind"] for x in manifest["artifacts"]}
    for expected in manifest["artifacts"]:
        if expected["kind"] not in selected:
            continue
        api = "https://api.github.com/repos/" + manifest["repository"] + "/actions/artifacts/" + str(expected["id"])
        with urllib.request.urlopen(authenticated(api, token), timeout=60) as response:
            metadata = json.load(response)
        assert metadata["id"] == expected["id"] and metadata["name"] == expected["name"]
        assert metadata["size_in_bytes"] == expected["bytes"]
        assert metadata["digest"] == "sha256:" + expected["sha256"] and not metadata["expired"]
        assert metadata["workflow_run"]["id"] == expected["run_id"]
        assert metadata["workflow_run"]["head_sha"] == manifest["source_commit"]
        (args.output / (expected["kind"] + "-github-artifact.json")).write_bytes(encoded(metadata))
        destination = args.downloads / expected["file_name"]
        assert destination.parent == args.downloads and Path(expected["file_name"]).name == expected["file_name"]
        copy_archive(api + "/zip", token, destination, expected)
        result = inspect_archive(destination, expected, args.output)
        rows.append(result)
        print("P7_AUDIT_PREFLIGHT_JSON " + json.dumps(result, ensure_ascii=True), flush=True)
    receipt = {"schema_version":1, "source_commit":manifest["source_commit"],
               "manifest_sha256":sha(args.manifest.read_bytes()),
               "script_sha256":sha(Path(__file__).read_bytes()),
               "artifact_summaries":rows, "task_ledger_changed":False,
               "acceptance":"not_granted; fixed bytes and inventory only"}
    (args.output / "preflight.json").write_bytes(encoded(receipt))
    print("P7_AUDIT_PREFLIGHT_RECEIPT " + json.dumps(receipt, ensure_ascii=True), flush=True)

if __name__ == "__main__":
    main()
