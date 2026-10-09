#!/usr/bin/env python3
"""Preserve exact D0 engineering originals for an independently reviewed P5."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

HERE = Path(__file__).resolve().parent
SOURCE = 'd0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
P5 = 'f5319d6a04f83aabe9dd635137382f4782d8d189'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def main():
    selected = {}
    def one(name, path):
        path = Path(path)
        assert path.is_file() and not path.is_symlink() and name not in selected
        selected[name] = path
    def directory(prefix, path):
        for member in sorted(Path(path).rglob('*')):
            assert not member.is_symlink(), member
            if member.is_file():
                one(prefix + '/' + member.relative_to(path).as_posix(), member)
    for prefix, path in [
        ('oracle-independent-pr-review', '/dev/shm/p8-oracle-batch-independent-pr-audit'),
        ('oracle-independent-acceptance-review', '/dev/shm/p8-oracle-batch-independent-acceptance'),
        ('prefix-independent-review', '/dev/shm/p8-prefix-independent-review-833d5259'),
        ('doc-key-index-independent-review', '/dev/shm/p8-doc-key-index-independent-review'),
        ('original-diagnostic-validation', '/dev/shm/p8-D0-diagnostic-validation'),
    ]:
        directory(prefix, path)
    for name in ['review.json', 'execution.json', 'verify_build.py', 'verify.stdout', 'verify.stderr',
                 'source-before.json', 'source-after.json', 'driver-before.json', 'driver-after.json',
                 'zip-members-before-extraction.json']:
        one('original-build-validation/' + name, Path('/dev/shm/p8-D0-build-independent-review') / name)
    for name in ['audit.json', 'controls-job.log', 'checkout-build-log-review.json']:
        one('original-controls-review/' + name, Path('/dev/shm/p8-d0-controls-reception') / name)
    one('original-upstream-zips/11581509438.zip', '/dev/shm/p8-d0-controls-reception/11581509438.zip')
    for aid in [11582323631, 11582922326, 11583016525, 11582252490]:
        one('original-upstream-zips/' + str(aid) + '.zip', Path('/dev/shm/p8-engineering-shard-reception') / (str(aid) + '.zip'))
    for name in ['11582323631-audit.json', '11582922326-audit.json',
                 '11582323631-phase-summary.json', '11582922326-phase-summary.json',
                 '11583016525-capacity-integrity.json', '11582252490-capacity-integrity.json',
                 'compare-728-d0-10k.json']:
        one('engineering-reception/' + name, Path('/dev/shm/p8-engineering-shard-reception') / name)
    build = Path('/workspace/scratch/2eaa00d0f93a/p8-D0-build-11582571291/original')
    for member in sorted(build.iterdir()):
        if member.name != 'p8-scale':
            one('original-build-metadata/' + member.name, member)
    for name in ['upstream-run-completed.json', 'upstream-jobs-completed.json', 'upstream-artifacts-completed.json']:
        one('original-upstream-metadata/' + name, Path('/dev/shm/p8-D0-full-study-controller-root') / name)
    one('exact-D0-source-preparation.json', '/dev/shm/p8-D0-source-preparation/report.json')
    for name in ['review.json', 'provisional-inventory.json', 'D0-study-feasibility-review.json']:
        one('source-bridge-preparation/' + name, Path('/dev/shm/p8-next-production-delta-independent-review') / name)
    one('source-bridge-preparation/independent-D0-boundary.json', '/dev/shm/p8-D0-study-boundary-independent-review/review.json')
    for name in ['update-plan.json', 'prospective-inputs.json']:
        one('P5-review-preparation/' + name, Path('/dev/shm/p8-P5-review-preparation-pr-audit') / name)
    for entry in json.loads((HERE / 'P5-import-entries.json').read_bytes()):
        one('reviewed-D0-source/' + entry['path'], entry['local'])
    for name in ['P5-publication.json', 'P5-git-commit.json', 'P5-git-commit.raw',
                 'P5-proposed-entries.json', 'P5-import-entries.json', 'import_published_commit.py']:
        one('exact-P5-object/' + name, HERE / name)
    data = {name: path.read_bytes() for name, path in selected.items()}
    files = {name: {'bytes': len(raw), 'sha256': sha(raw), 'mode': selected[name].stat().st_mode & 0o777}
             for name, raw in data.items()}
    assert sum(row['bytes'] for row in files.values()) < 16 * 1024 * 1024
    archive = HERE / 'D0-engineering-originals.tar.gz'
    with archive.open('xb') as output:
        with gzip.GzipFile(filename='', fileobj=output, mode='wb', mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode='w') as tar:
                for name, raw in sorted(data.items()):
                    info = tarfile.TarInfo(name)
                    info.size, info.mode, info.mtime = len(raw), files[name]['mode'], 0
                    tar.addfile(info, io.BytesIO(raw))
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        assert len(members) == len(files) and {row.name for row in members} == set(files)
        for member in members:
            assert member.isfile() and not member.name.startswith('/') and '..' not in Path(member.name).parts
            raw = tar.extractfile(member).read()
            assert len(raw) == files[member.name]['bytes'] and sha(raw) == files[member.name]['sha256']
            assert member.mode == files[member.name]['mode']
    for name, path in selected.items():
        assert sha(path.read_bytes()) == files[name]['sha256'] and path.stat().st_mode & 0o777 == files[name]['mode']
    raw = archive.read_bytes()
    assert len(raw) < 3 * 1024 * 1024
    manifest = {
        'schema': 'p8-D0-engineering-originals-for-P5-source-review-v1',
        'measured_source': SOURCE, 'proposed_product_source': P5, 'upstream_run': 37846370300,
        'public_directory': 'artifacts/checkpoints/p8-completion-20261009/scale-engineering-D0',
        'scope': 'Five reviewed source blobs, actual12controls, original release build provenance and two complete diagnostics; no N30/full CI or TODO closure',
        'archive': archive.name, 'archive_bytes': len(raw), 'archive_sha256': sha(raw),
        'archive_git_blob': hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest(),
        'file_count': len(files), 'original_bytes': sum(row['bytes'] for row in files.values()),
        'all_members_roundtrip_and_all_original_files_unchanged': True, 'files': files,
        'binary_reference': {'included_in_archive': False, 'actions_artifact_id': 11582571291,
                             'run_url': 'https://github.com/jyqj/codecortex/actions/runs/37846370300',
                             'original_zip_bytes': 9214914,
                             'original_zip_sha256': '42d0c06359eaf0f0dd68e7683d491c662778cd64e9a621a2b501da1ed7f68d92',
                             'member': 'p8-scale', 'bytes': 28260856,
                             'sha256': 'c026b32f8a3a32997d9336c5159b589bd022743f0fef691f70d2bfba70454e45',
                             'blake3': 'eca1834fa1d23de5ab6b7753c8517cd10082030ea7f9203df5eaddb92eb4a5d4'},
        'prior_review_commit': '5463095d9650402161a23a332c2505b15c4d88b6',
        'prior_failed_local_study_evidence_commit': 'bf48b9125514bfe3e22a77e025b4224acdb54ff2',
        'prior_engineering_source_attempts_retained': ['833d52596df02aeb428bce526b9a5d4fc19d20ac',
            '1b9e846db340cf9a5297ef44ab548a0816712029', 'bf5be9f14000cc746e0fdcd59739ee953616fe61',
            'f9d5727914331ce191172212702ed72e85aee068', '728e795a572d355c283be69653e8828138a57dd5'],
        'independent_full150_controller': 'Separate auxiliary branch; not included among P5 source or validation inputs',
        'TODO_closed': 0, 'TODO_remaining': 29,
    }
    (HERE / 'D0-engineering-manifest.json').open('x').write(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    print(json.dumps({key: value for key, value in manifest.items() if key not in ('files', 'binary_reference')}, sort_keys=True))


if __name__ == '__main__':
    main()
