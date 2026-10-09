#!/usr/bin/env python3
"""Freeze public round-34 evidence without source or task changes."""
from pathlib import Path
import gzip, hashlib, io, json, tarfile

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "round34-public-archive"
PREFIX = "artifacts/checkpoints/p8-round34-a217-20261009"
PARENT = "64dbafa6897f44699f31570e4bda06938f624daa"
OUT.mkdir(exist_ok=True)

def raw_json(value):
    return (json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n").encode()

def digest(data):
    return hashlib.sha256(data).hexdigest()

def oid(data):
    return hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()

def write_once(path, data):
    if path.exists():
        if path.read_bytes() != data:
            raise ValueError("existing output differs: " + str(path))
    else:
        path.write_bytes(data)

entries = []
runtime_manifest = ROOT / "original-LG2-runtime/round34-public-archive/public-files-manifest.json"
runtime = json.loads(runtime_manifest.read_text())
assert runtime["file_count"] == 26
for row in runtime["files"]:
    entries.append((row["source"], "runtime/" + row["target"], row["sha256"], row["bytes"]))
entries.append((str(runtime_manifest.relative_to(ROOT)), "inputs/runtime-public-files-manifest.json", None, None))
for path in sorted((ROOT / "round34-root-operations").glob("*.json")):
    entries.append((str(path.relative_to(ROOT)), "root/" + path.name, None, None))
for name in ("candidate-proof.json",):
    entries.append(("original-LG2-runtime/original-custody-final/" + name, "g2-custody/" + name, None, None))
for name in ("original.log", "exact-log-byte-receipt.json", "initial-text-transfer-extra-newline.log"):
    entries.append(("original-C3ff-platform-gates/M6-current-ci/failed-check-113859546484/" + name, "M6-current-ci-failure/" + name, None, None))
for name in ("build-progress-projection.json", "stage-timing-summary-v1.json", "phase-time-attribution-v1.json", "long-resume-progress-independent-review.json", "original-failure-ZIP-recovery-receipt.json"):
    entries.append(("C8-resume-diagnostic/" + name, "C8-observed-diagnostic/" + name, None, None))
for name in ("candidate.patch", "candidate-manifest.json", "independent-review-scale-closeout.json"):
    entries.append(("C8-resume-diagnostic/candidate-reverse-exclusion/" + name, "unpublished-first-SQL-candidate/" + name, None, None))
seen = set()
payload = []
inventory = []
for source, target, expected_sha, expected_size in sorted(entries, key=lambda row: row[1]):
    if target in seen or target.startswith("/") or ".." in Path(target).parts:
        raise ValueError("unsafe or duplicate archive member")
    seen.add(target)
    path = ROOT / source
    data = path.read_bytes()
    sha = digest(data)
    if expected_sha is not None and (sha != expected_sha or len(data) != expected_size):
        raise ValueError("frozen public input changed: " + source)
    mode = path.stat().st_mode & 0o777
    inventory.append(dict(source=source, path=target, bytes=len(data), sha256=sha, git_blob=oid(data), mode=oct(mode)))
    payload.append((target, data, mode))
tar_buffer = io.BytesIO()
with tarfile.open(fileobj=tar_buffer, mode="w", format=tarfile.PAX_FORMAT) as archive:
    for target, data, mode in payload:
        info = tarfile.TarInfo(target)
        info.size, info.mode, info.mtime = len(data), mode, 0
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        archive.addfile(info, io.BytesIO(data))
tar_bytes = tar_buffer.getvalue()
buffer = io.BytesIO()
with gzip.GzipFile(filename="", mode="wb", fileobj=buffer, mtime=0, compresslevel=9) as stream:
    stream.write(tar_bytes)
bundle = buffer.getvalue()
with tarfile.open(fileobj=io.BytesIO(bundle), mode="r:gz") as archive:
    for row in inventory:
        data = archive.extractfile(row["path"]).read()
        assert len(data) == row["bytes"] and digest(data) == row["sha256"]
for row in inventory:
    assert digest((ROOT / row["source"]).read_bytes()) == row["sha256"]
write_once(OUT / "round34-public-evidence.tar.gz", bundle)
write_once(OUT / "inventory.json", raw_json(dict(schema="round34-public-evidence-inventory-v1", file_count=len(inventory), payload_bytes=sum(row["bytes"] for row in inventory), files=inventory)))
summary = dict(schema="round34-public-evidence-inputs-v1", parent=PARENT, repository_prefix=PREFIX, original_tasks_total=192, original_tasks_done=163, original_tasks_remaining=29, original_tasks_completed_this_session=0, member_count=len(inventory), payload_bytes=sum(row["bytes"] for row in inventory), bundle_bytes=len(bundle), bundle_sha256=digest(bundle), tar_bytes=len(tar_bytes), tar_sha256=digest(tar_bytes), original_studies_unchanged=True, raw_zip_sources={"G2_all_five":"94f96e0d7692c2549a9c4486d63e47220dc10d90", "C8_all_twelve":"92b5624b821da81bbf3bf4c1eb05d1d3c44d7c1c"}, notes=["R34 C16 acceptance remains distinct from the three accepted cohorts in R33.", "First SQL candidate and its static review are unpublished design work, not Rust execution or speedup evidence.", "M6 current main CI failed at fmt before Clippy/Rust/workspace tests and v15.", "PR184 was closed externally as superseded, not merged; its exact source remains in PR180.", "Private transfer references and signed URLs and original ZIPs are not included in this small evidence bundle."])
write_once(OUT / "inputs.json", raw_json(summary))
readme = """# Round 34 original-task progress
Original ledger: 192 tasks, 163 done, 29 remaining. This session has closed 0 of the requested minimum 10 original tasks.

The complete public bundle preserves the fourth G2 cohort intake, all-four scope index, original M6 fmt failure and exact byte receipt, original C8 progress/timing analysis, the initial unpublished SQL candidate and independent static review, PR184 external superseded closure, and G2 five-ZIP custody publication. Current C source remains 4/150 accepted shards and 41/1500 samples; its original 100k job was still running at the cutoff. No source study, sample count, budget, task definition or dependency is changed.

The gzip archive is a deterministic container. inventory.json binds every full public member. Original five G2 ZIPs and twelve C8 ZIPs are stored losslessly at the immutable commits in inputs.json; this small bundle does not repackage them. R33 and earlier evidence remains in the parent tree.

All source observations and failures retain their actual identities. SQL returned-row reductions are not a universal timing claim or proof of resolving the five-hour deadline. New M7 source and a separately identified strict cold-only study are preparation work for round 35.
"""
write_once(OUT / "README.md", readme.encode())
public_names = ["round34-public-evidence.tar.gz", "inventory.json", "inputs.json", "README.md", "prepare_round34.py"]
files = []
for name in public_names:
    data = (OUT / name).read_bytes()
    files.append(dict(path=PREFIX + "/" + name, source=str(OUT / name), bytes=len(data), sha256=digest(data), git_blob=oid(data), mode="100644", type="blob"))
plan = dict(schema="round34-evidence-five-leaf-upload-plan-v1", parent=PARENT, branch="task/p8-round25-receipts-a217-20261009", prefix=PREFIX, files=files, only_new_prefix=True)
write_once(OUT / "git-upload-plan.json", raw_json(plan))
print(json.dumps(summary, ensure_ascii=False))
