"""Archive explicitly selected engineering originals without modifying them."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import tarfile

base = Path('/workspace/scratch/2eaa00d0f93a')
repo = base / 'codecortex-runtime-followup-sparse'
out = base / 'p8-followup-engineering-archive'
out.mkdir()
limit = 4_194_303
old = 'bb9a96d71622458c39a143055360cc97f0d11d78'
head = 'd0fe84a3e3f556c4f5907cc62190b20902a9f84d'
published = 'b11addeed93b02c7b840bd3b61a452ce0e671287'
def git(*args): return subprocess.check_output(['git', '-C', str(repo), *args])
def sha(path):
    digest = hashlib.sha256()
    with Path(path).open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''): digest.update(block)
    return digest.hexdigest()
def write(path, value): path.write_text(json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + '\n')
assert git('rev-parse', 'HEAD').decode().strip() == head
assert not git('status', '--porcelain', '--untracked-files=all')
assert git('rev-parse', head + '^{tree}') == git('rev-parse', published + '^{tree}')
commits = git('rev-list', head, '^' + old).decode().splitlines()
assert commits == [head, 'd5d4d570d97b59cda29a55251db875235ff6e507', '88d447390fa8a7dd04d9a6679fe90c1477bfa280']

groups = {
 'authored_runtime_controls': ['p8-runtime-followup-controls'],
 'authored_collector_controls': ['p8-collector-followup-controls'],
 'independent_runtime_review': ['p8-runtime-independent-review-d5d4d570'],
 'independent_collector_review': ['p8-collector-independent-review-d0fe84a3'],
 'independent_followup_summary': ['p8-followup-independent-review-d0fe84a3.json'],
 'original_c709_approval_attempts': ['p8-final-source-approval-c7098ea5',
     'p8-final-source-approval-c7098ea5-missing42', 'p8-final-source-approval-c7098ea5-missing42-attempt2',
     'p8-final-source-approval-c7098ea5-combined-coverage.json'],
 'scale_input_invariance': ['p8-final-independent-source-review/scale-input-invariance-G-to-d0fe84a3.json',
                          'p8-final-independent-source-review/review_scale_invariance.py'],
 'actual_cargo_feature_protocol': ['p8-pr159-readonly-audit/' + name for name in [
     'feature-control', 'run_feature_control.py', 'feature-control-report.json',
     'backfill-88d44739-independent-review.json', 'rustc-version.stdout',
     'lock.stdout', 'lock.stderr', 'original.stdout', 'original.stderr',
     'explicit-no-default.stdout', 'explicit-no-default.stderr']],
}
# Retain the exact two small original protocol executables named by Cargo,
# without copying Cargo target caches or unrelated peer metadata/downloads.
for name in ('original', 'explicit-no-default'):
    original = base / 'p8-pr159-readonly-audit' / (name + '.stdout')
    rows = [json.loads(line) for line in original.read_bytes().splitlines()]
    artifacts = [row for row in rows if row.get('reason') == 'compiler-artifact']
    assert len(artifacts) == 1
    executable = Path(artifacts[0]['executable'])
    assert executable.is_file() and not executable.is_symlink()
    assert executable.parent == base / 'p8-pr159-readonly-audit' / ('target-' + name) / 'release/deps'
    groups['actual_cargo_feature_protocol'].append(executable.relative_to(base).as_posix())

entries, directories = {}, {}
for group, roots in groups.items():
    for name in roots:
        path = base / name
        assert path.exists() and not path.is_symlink(), name
        paths = [path, *sorted(path.rglob('*'))] if path.is_dir() else [path]
        for entry in paths:
            relative = entry.relative_to(base).as_posix()
            info = entry.lstat()
            assert not entry.is_symlink(), 'unexpected symlink: ' + relative
            mode = stat.S_IMODE(info.st_mode)
            if entry.is_dir():
                directories[relative] = mode
            else:
                assert stat.S_ISREG(info.st_mode), relative
                assert relative not in entries
                # This selection excludes all peer API/download records.
                # Detect actual signed download URLs, without retaining them.
                if info.st_size < 4 * 1024 * 1024:
                    body = entry.read_bytes()
                    assert not re.search(rb'https?://[^\s"<>]*(?:X-Amz-Signature|X-Goog-Signature|[?&]sig=)', body, re.I), relative
                entries[relative] = {'bytes': info.st_size, 'sha256': sha(entry), 'mode': mode, 'group': group}
                for parent in Path(relative).parents:
                    if str(parent) != '.': directories.setdefault(parent.as_posix(), stat.S_IMODE((base / parent).stat().st_mode))

# Recheck retained inventories already emitted by the original observers.
checks = []
for name, field in [
    ('p8-runtime-followup-controls/validation-receipt.json', 'evidence_inventory'),
    ('p8-collector-followup-controls/validation-receipt.json', 'inventory'),
    ('p8-runtime-independent-review-d5d4d570/audit.json', 'artifacts'),
    ('p8-collector-independent-review-d0fe84a3/audit.json', 'artifacts'),
    ('p8-final-source-approval-c7098ea5/report.json', 'files'),
    ('p8-final-source-approval-c7098ea5-missing42/report.json', 'files'),
    ('p8-final-source-approval-c7098ea5-missing42-attempt2/report.json', 'files')]:
    report = json.loads((base / name).read_text())
    records = report[field]
    for relative, expected in records.items():
        path = (base / name).parent / relative
        assert path.stat().st_size == expected['bytes'] and sha(path) == expected['sha256'], str(path)
    checks.append({'receipt': name, 'field': field, 'verified_files': len(records), 'sha256': sha(base / name)})

manifest = {
    'schema_version': 1,
    'classification': 'Immutable engineering originals and independent reviews; no product, new-source CI, TODO or release approval implied',
    'source_identity': {'public_prerequisite_G': old, 'local_tip': head, 'local_commits': commits,
        'published_P3': published, 'local_and_published_whole_tree': git('rev-parse', head + '^{tree}').decode().strip(),
        'whole_tree_equality_independently_checked': True},
    'source_groups': groups,
    'scope_notes': {
        'authored_runtime_controls': 'Protocol fakes with real threads; all red attempts and raw, first green, final green and sparse-fixture setup failures retained. Raw-budget failure does not claim all raw terminal rows were retained.',
        'authored_collector_controls': 'Real old/new CLI empty-input behavior plus strict synthetic protocol controls. Earlier uncommitted not_run-count draft is retained as superseded; final receipt counts are null.',
        'independent_reviews': 'Exact reviewer exports, original command outputs and six real-thread protocol fixtures retained without modification, including five seals and one explicitly unsealed case.',
        'actual_cargo_feature_protocol': 'Actual Rust 1.95 release Cargo feature-label comparison on a dependency-free explicit protocol crate, plus original two test executables. Not a product or backfill run.',
        'original_c709_approval_attempts': 'Original full suite remains failed: 139 methods passed, three setup errors from ENOSPC. First missing42 attempt remains not_run for capacity. Second attempt has original 21/7/14 methods; combined report accounts for 181 unique methods, not a rewritten full-suite success. Selected v15 and historical gate retain original c709 identities.',
        'scale_input_invariance': 'Static G-to-d0 input identity only; original G scale run and capacity/raw evidence are not relabeled P3.'},
    'original_receipt_inventory_checks': checks,
    'excluded': ['signed artifact URLs and peer PR metadata/downloads', 'Cargo caches except the two original small protocol executables',
                 'heldout corpus bodies', 'new formal G recovery artifacts and future P3 executions'],
    'file_count': len(entries), 'file_bytes': sum(record['bytes'] for record in entries.values()),
    'files': entries, 'directories': directories,
    'tar_prefix': 'evidence/', 'max_deliverable_bytes': limit,
}
write(out / 'manifest.json', manifest)

archive = out / 'engineering-originals.tar.gz'
with archive.open('xb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', mtime=0, filename='') as compressed, tarfile.open(fileobj=compressed, mode='w|', format=tarfile.PAX_FORMAT) as tar:
    for name, mode in sorted(directories.items()):
        item = tarfile.TarInfo('evidence/' + name)
        item.type, item.mode, item.mtime = tarfile.DIRTYPE, mode, 0
        tar.addfile(item)
    for name, record in sorted(entries.items()):
        path = base / name
        assert path.stat().st_size == record['bytes'] and sha(path) == record['sha256']
        item = tarfile.TarInfo('evidence/' + name)
        item.size, item.mode, item.mtime = record['bytes'], record['mode'], 0
        with path.open('rb') as stream: tar.addfile(item, stream)

bundle = out / 'followup-local-commits.bundle'
branch = 'refs/heads/task/p8-runtime-followup-20261009'
assert git('rev-parse', branch).decode().strip() == head
git('bundle', 'create', str(bundle), branch, '^' + old)
verification = subprocess.run(['git', '-C', str(repo), 'bundle', 'verify', str(bundle)], capture_output=True)
(out / 'bundle-verify.stdout').write_bytes(verification.stdout)
(out / 'bundle-verify.stderr').write_bytes(verification.stderr)
assert verification.returncode == 0
heads = git('bundle', 'list-heads', str(bundle)).decode()
(out / 'bundle-heads.txt').write_text(heads)
assert heads == head + ' ' + branch + '\n'
header = bundle.read_bytes().split(b'\n\n', 1)[0].decode()
prerequisites = [line[1:41] for line in header.splitlines() if line.startswith('-')]
assert prerequisites == [old], prerequisites
write(out / 'bundle-provenance.json', {
    'public_prerequisites': prerequisites, 'advertised_ref': branch, 'tip': head,
    'commits_not_reachable_from_G': commits, 'object_tree': git('rev-parse', head + '^{tree}').decode().strip(),
    'verification_exit_code': verification.returncode,
    'commit_records': {commit: git('cat-file', '-p', commit).decode() for commit in commits},
    'bundle_sha256': sha(bundle), 'bundle_bytes': bundle.stat().st_size})

# Read every member back through gzip/tar and compare to the frozen manifest.
seen_files, seen_dirs = set(), set()
with tarfile.open(archive, 'r:gz') as tar:
    for item in tar:
        assert item.name.startswith('evidence/') and '..' not in Path(item.name).parts
        name = item.name.removeprefix('evidence/')
        if item.isdir():
            assert name in directories and name not in seen_dirs and item.mode == directories[name]
            seen_dirs.add(name)
        else:
            assert item.isfile() and name in entries and name not in seen_files
            expected = entries[name]
            assert item.size == expected['bytes'] and item.mode == expected['mode']
            stream = tar.extractfile(item)
            digest, count = hashlib.sha256(), 0
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                digest.update(block); count += len(block)
            assert count == expected['bytes'] and digest.hexdigest() == expected['sha256']
            seen_files.add(name)
assert seen_files == set(entries) and seen_dirs == set(directories)
for name, expected in entries.items():
    assert sha(base / name) == expected['sha256'] and (base / name).stat().st_size == expected['bytes']
assert not git('status', '--porcelain', '--untracked-files=all')
write(out / 'archive-verification.json', {
    'status': 'all_originals_and_tar_members_byte_identical',
    'archive': archive.name, 'archive_sha256': sha(archive), 'archive_bytes': archive.stat().st_size,
    'manifest_sha256': sha(out / 'manifest.json'), 'files_verified': len(seen_files),
    'directories_verified': len(seen_dirs), 'uncompressed_file_bytes': manifest['file_bytes'],
    'all_original_inputs_rechecked_after_tar_creation': True,
    'source_worktree_remained_clean': True,
    'bundle_verified': True, 'bundle_public_prerequisites': prerequisites})
for path in out.iterdir(): assert path.stat().st_size <= limit, path
print(json.dumps({'output': str(out), 'files': len(entries), 'original_bytes': manifest['file_bytes'],
    'archive_bytes': archive.stat().st_size, 'bundle_bytes': bundle.stat().st_size,
    'manifest_bytes': (out / 'manifest.json').stat().st_size}))
