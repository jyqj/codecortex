"""Independently compare a runtime record bundle to its retained original ZIP.

This verifies archival fidelity only. It neither runs nor replaces acceptance.
Usage: python verify_and_preserve_original_bundle.py WORKSPACE ARTIFACT_ID PROFILE ACCEPTED_PROFILES
"""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import sys
import tarfile
import zipfile


HEAD = "29682890c89511dd6f477a6bf48bd969aa1537af"
RUN = 37844310853
PACKAGER_SHA = "fda55e41271e2af4b264c7688600dad4c9bae3945a00183811a284dba54f7d6b"


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def path_ok(name):
    p = Path(name)
    assert not p.is_absolute() and ".." not in p.parts and "\\" not in name


def main():
    workspace = Path(sys.argv[1]).resolve()
    aid = int(sys.argv[2])
    profile = sys.argv[3]
    accepted = int(sys.argv[4])
    assert profile in {"backfill", "mixed-c1", "mixed-c4", "mixed-c8", "mixed-c16", "soak"}
    assert 1 <= accepted <= 6
    original = workspace / "ci-artifacts" / str(aid) / (str(aid) + ".zip")
    metadata_path = original.parent / "metadata.json"
    metadata = json.loads(metadata_path.read_text())
    assert metadata["id"] == aid and metadata["workflow_run"]["id"] == RUN
    assert metadata["workflow_run"]["head_sha"] == HEAD
    assert original.stat().st_size == metadata["size_in_bytes"]
    original_sha = sha(original)
    assert metadata["digest"] == "sha256:" + original_sha
    bundle_dir = workspace / "runtime-review" / "round6-evidence-bundles"
    manifest_path = bundle_dir / (str(aid) + "-non-elf-derivation.json")
    bundle_path = bundle_dir / (str(aid) + "-non-elf-original-members.tar.gz")
    manifest = json.loads(manifest_path.read_text())
    assert manifest["artifact_id"] == aid and manifest["workflow_run"] == RUN
    assert manifest["source_head"] == HEAD and manifest["not_complete_original_zip"] is True
    assert manifest["acceptance_rerun"] is False
    assert manifest["packager"]["sha256"] == PACKAGER_SHA
    assert manifest["original_zip"]["sha256"] == original_sha
    assert manifest["original_zip"]["bytes"] == original.stat().st_size
    assert manifest["bundle"]["sha256"] == sha(bundle_path)
    assert manifest["bundle"]["bytes"] == bundle_path.stat().st_size
    review_path = Path(manifest["accepted_review"]["path"])
    assert review_path.is_relative_to(workspace / "runtime-review")
    assert sha(review_path) == manifest["accepted_review"]["sha256"]
    review = json.loads(review_path.read_text())
    assert review["artifact_id"] == aid and review["head"] == HEAD
    assert review["workflow_run"] == RUN and review["zip_sha256"] == original_sha
    assert review["unresolved_blockers"] == []
    assert review["verdict"].startswith("accepted_scoped_original_29682890_")
    included = {x["path"]: x for x in manifest["included_members"]}
    omitted = {x["path"]: x for x in manifest["omitted_receipt_bound_elf_executables"]}
    assert len(included) == len(manifest["included_members"]) == manifest["included_member_count"]
    assert len(omitted) == len(manifest["omitted_receipt_bound_elf_executables"])
    assert not (set(included) & set(omitted))
    with zipfile.ZipFile(original) as archive:
        infos = archive.infolist()
        assert len({x.filename for x in infos}) == len(infos)
        for info in infos:
            path_ok(info.filename)
            assert ((info.external_attr >> 16) & 0o170000) != 0o120000
        files = {x.filename: x for x in infos if not x.is_dir()}
        dirs = {x.filename.rstrip("/") for x in infos if x.is_dir()}
        assert set(files) == set(included) | set(omitted)
        assert dirs == set(manifest["directory_members"])
        seen, seen_dirs = set(), set()
        with tarfile.open(bundle_path, "r|gz") as tar:
            for member in tar:
                path_ok(member.name)
                if member.isdir():
                    assert member.name.rstrip("/") not in seen_dirs
                    seen_dirs.add(member.name.rstrip("/"))
                    continue
                assert member.isfile() and member.name in included and member.name not in seen
                seen.add(member.name)
                item = included[member.name]
                assert member.size == files[member.name].file_size == item["bytes"]
                h = hashlib.sha256()
                count = 0
                with archive.open(member.name) as z, tar.extractfile(member) as t:
                    while True:
                        zb, tb = z.read(1024 * 1024), t.read(1024 * 1024)
                        assert zb == tb, member.name
                        if not zb:
                            break
                        count += len(zb)
                        h.update(zb)
                assert count == item["bytes"] and h.hexdigest() == item["sha256"]
        assert seen == set(included) and seen_dirs == dirs
        for name, item in omitted.items():
            data = archive.read(name)
            assert len(data) == files[name].file_size == item["bytes"]
            assert hashlib.sha256(data).hexdigest() == item["sha256"]
            assert data[:4] == b"\x7fELF" and data[4] in (1, 2) and data[5] in (1, 2)
            assert int.from_bytes(data[16:18], "little" if data[5] == 1 else "big") in (2, 3)
            raw = archive.read(item["receipt_path"])
            assert hashlib.sha256(raw).hexdigest() == item["receipt_sha256"]
            receipt = json.loads(raw)
            if name == "p7_worker_contention":
                assert profile == "backfill"
                value = receipt
                assert value["executable_sha256"] == item["sha256"]
            else:
                assert name.startswith("p8-build/")
                value = receipt["artifacts"][Path(name).name]
                assert value["binary_sha256"] == item["sha256"]
                assert value["binary_bytes"] == item["bytes"]
            assert value["copy_source"]["sha256"] == item["sha256"]
            assert value["copy_source"]["bytes"] == item["bytes"]
            assert value["cargo_artifact"]["executable"] == value["copy_source"]["path"]
            assert value["cargo_artifact"]["target"]["name"] == Path(name).name
    assert sha(original) == original_sha
    repo = workspace / "codecortex"
    dest = repo / "artifacts/benchmarks/p8-completion-20261009/runtime-296" / profile
    assert not dest.exists()
    dest.mkdir(parents=True)
    records = []
    for source, basename in [(manifest_path, manifest_path.name), (bundle_path, bundle_path.name),
                             (review_path, "independent-review.json"), (metadata_path, "metadata.json")]:
        target = dest / basename
        original_hash = sha(source)
        shutil.copyfile(source, target)
        assert sha(target) == original_hash and target.stat().st_size == source.stat().st_size
        records.append({"original_path": str(source), "repository_path": str(target.relative_to(repo)),
                        "sha256": original_hash, "bytes": target.stat().st_size})
    result = {
        "record_type": "independent_root_raw_record_preservation_check",
        "observed_utc": datetime.now(timezone.utc).isoformat(), "artifact_id": aid, "source_head": HEAD,
        "all_original_member_partition_checked": True,
        "included_raw_members_byte_equal_original_zip_and_tar": len(included),
        "receipt_bound_elf_omissions_verified": len(omitted), "not_complete_original_zip": True,
        "original_zip_retained": True, "copied_records": records,
        "new_runtime_profiles_accepted_at_checkpoint": accepted,
        "original_todo_remaining": 29, "task_statuses_changed": False,
        "acceptance_workload_rerun": False,
        "verifier": {"path": str(Path(__file__).relative_to(repo)), "sha256": sha(Path(__file__))},
    }
    (dest / "root-preservation-review.json").write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"artifact_id": aid, "profile": profile, "included": len(included),
                      "omitted_elf": len(omitted), "copied_bytes": sum(x["bytes"] for x in records),
                      "destination": str(dest)}, indent=2))


if __name__ == "__main__":
    main()
