#!/usr/bin/env python3
"""Checkpoint completed round-10 evidence without changing measured sources."""
from pathlib import Path
import gzip
import hashlib
import io
import json
import stat
import tarfile
import zipfile

OUT = Path(__file__).resolve().parent
WORK = Path('/workspace/scratch/2eaa00d0f93a')
SHM = Path('/dev/shm')
G4 = WORK / 'p8-formal-evidence-G4'

def sha(data):
    return hashlib.sha256(data).hexdigest()

def main():
    selected, origins = {}, {}
    def add(label, path):
        path = Path(path)
        assert label not in selected and stat.S_ISREG(path.lstat().st_mode)
        raw = path.read_bytes()
        selected[label] = raw
        origins[label] = {'kind': 'original_local_file', 'path': str(path),
                          'bytes': len(raw), 'sha256': sha(raw)}
    for name in ['inspect_original.py', 'inspection.json', 'member-hashes.json', 'run.json', 'jobs.json',
                 'artifacts.json', 'job-113576446441.log']:
        add('gates/reception/' + name, G4 / 'gates' / name)
    for name in ['independent_review.py', 'independent_review.v1.py', 'adapter.diff', 'adapter-plan.json',
                 'adapter-v2.diff', 'receive_archive.py', 'expected-artifact.json', 'member-hashes.json',
                 'run.json', 'jobs.json', 'artifacts.json', 'job-113576451817.log',
                 'independent-review.stdout', 'independent-review.stderr', 'execution-receipt.json']:
        add('lifecycle/reception/' + name, G4 / 'lifecycle' / name)
    for path in sorted((G4 / 'lifecycle/independent-review-01').iterdir()):
        if path.is_file() and path.name != 'original-p8-measurements':
            add('lifecycle/reception/independent-review-01/' + path.name, path)
    for name in ['review_mixed.py', 'adapter.diff', 'adapter-plan.json']:
        add('mixed/preparation-only/' + name, G4 / 'mixed' / name)
    peer = SHM / 'p8-G4-receivers-independent-pr-audit'
    for name in ['adapter-audit.json', 'audit_adapters.py', 'lifecycle-storage-audit.json',
                 'gates-audit.json', 'audit_gates.py', 'D0-actual-admission-replay.json', 'D0-v2-receiver-audit.json']:
        add('peer-reviews/' + name, peer / name)
    for name in ['security-audit.json', 'security-job-113576447489.log',
                 'check-failure-audit.json', 'check-job-113576447718.log']:
        add('G4-CI/' + name, SHM / 'p8-G4-CI-reception-pr-audit' / name)
    for label, path in [
        ('G4-actual-published-identity.json', SHM / 'p8-P5-review-preparation-pr-audit/G4-published-identity-bridge.json'),
        ('D0-to-P5-actual-byte-bridge.json', SHM / 'p8-D0-P5-byte-bridge-independent-review/review.json'),
        ('G4-exact-source-preparation.json', SHM / 'p8-G4-source-preparation/report.json'),
        ('main341-source-invariance-only.json', SHM / 'p8-five-source-publication-root/new-main-341-invariance.json')]:
        add('source/' + label, path)
    admission = SHM / 'p8-D0-study-admission-first-review'
    for name in ['review.json', 'verify.py', 'verify.stdout', 'verify.stderr',
                 '11584016580.zip-directory.json', '11584291128.zip-directory.json',
                 'api-jobs-page1.json', 'api-jobs-page2.json', 'api-artifacts-page1.json']:
        add('D0-first-admission/' + name, admission / name)
    add('D0-first-capacity/review.json', SHM / 'p8-D0-capacity-first-three-review/review.json')
    receiver = SHM / 'p8-D0-sequential-receiver'
    for name in ['receive.py', 'receive-v1.py', 'upstream-members.json', 'handoff.json',
                 'receive.py.diff', 'receive-v1-to-v2.diff', 'test_receive.py', 'test_receive.py.diff',
                 'test_d0_protocol.py', 'build_draft.py', 'admission.fragment.py']:
        add('D0-receiver/' + name, receiver / name)
    for control in ['controls-original13', 'controls-all15']:
        for name in ['receipt.json', 'stdout', 'stderr']:
            add('D0-receiver/' + control + '/' + name, receiver / control / name)
    for name in ['review.py', 'review.json', 'review.stdout', 'review.stderr']:
        add('D0-receiver/root-review/' + name, SHM / 'p8-D0-receiver-root-review' / name)
    for name in ['G-100000-1-observation.json', 'G-100000-1-job-113495325456.json',
                 'G-100000-1-job-113495325456.log', 'snapshot-20261008-230958.json']:
        add('original-G-failure/' + name, WORK / 'p8-scale-observations-20261008' / name)

    references = []
    gate_inventory = json.loads((G4 / 'gates/member-hashes.json').read_text())
    specifications = [
        ('gates', G4 / 'gates/artifact-11584996050.zip', 37854847767, 11584996050, 32977177,
         'b5d6fd945c468822700aafe0f66fcf482697c2333e2197fd2e0039969671a529'),
        ('lifecycle', G4 / 'lifecycle/original-artifact.zip', 37854847926, 11585181176, 49348679,
         '81756f7d1990147961c87f905625c33da69b57d703f8eb4afa7e0f690ca52c31')]
    lifecycle_metadata = {
        'replay-build-receipt.json', 'replay-build.stderr', 'replay-build.jsonl',
        'product/build-receipt.json', 'product/source-inputs.json', 'product/cargo-build.stderr.log',
        'product/cargo-build.jsonl', 'measurement/replay.stdout', 'measurement/replay.stderr',
        'measurement/plan.json', 'measurement/receipt.json', 'measurement/report.json',
        'measurement/report-replayed.json', 'measurement/repeat-replay-receipt.json'}
    for kind, archive, run, artifact, byte_count, digest in specifications:
        assert archive.stat().st_size == byte_count and sha(archive.read_bytes()) == digest
        included, omitted = [], []
        with zipfile.ZipFile(archive) as z:
            for item in z.infolist():
                name = item.filename
                include = (kind == 'gates' and name != 'cc-eval' and not name.endswith('/fault-test')) or (
                    kind == 'lifecycle' and name in lifecycle_metadata)
                if not include:
                    omitted.append(name)
                    continue
                label = kind + '/original/' + name
                assert label not in selected
                raw = z.read(item)
                if kind == 'gates':
                    assert len(raw) == gate_inventory[name]['bytes'] and sha(raw) == gate_inventory[name]['sha256']
                selected[label] = raw
                origins[label] = dict(kind='unchanged_original_ZIP_member', artifact_id=artifact,
                                      archive_sha256=digest, member=name, bytes=len(raw), sha256=sha(raw))
                included.append(name)
        references.append(dict(source_commit='260f596582f2d82b8d7c707b61a6b8b6a43b069f', run_id=run,
                               run_attempt=1, artifact_id=artifact, bytes=byte_count, sha256=digest,
                               run_url=f'https://github.com/jyqj/codecortex/actions/runs/{run}',
                               artifact_url=f'https://github.com/jyqj/codecortex/actions/runs/{run}/artifacts/{artifact}',
                               complete_original_ZIP_retained=True, complete_member_inventory_in_packet=True,
                               packet_original_members=included, original_members_not_duplicated_in_packet=omitted))
    # Preserve the full original ZIP references and their full inventories. The
    # packet is explicitly a review checkpoint, not a substitute whole raw ZIP.
    refs_bytes = (json.dumps(references, sort_keys=True, indent=2) + '\n').encode()
    selected['original-artifact-references.json'] = refs_bytes
    origins['original-artifact-references.json'] = dict(kind='reference_metadata', bytes=len(refs_bytes), sha256=sha(refs_bytes))
    packet = OUT / 'round10-evidence.tar.gz'
    with packet.open('xb') as raw_output:
        with gzip.GzipFile(filename='', fileobj=raw_output, mode='wb', mtime=0) as zipped:
            with tarfile.open(fileobj=zipped, mode='w') as archive:
                for name, raw in sorted(selected.items()):
                    entry = tarfile.TarInfo(name)
                    entry.size, entry.mode, entry.mtime = len(raw), 0o644, 0
                    archive.addfile(entry, io.BytesIO(raw))
    with tarfile.open(packet, 'r:gz') as archive:
        assert {entry.name for entry in archive.getmembers()} == set(selected)
        for entry in archive.getmembers():
            assert entry.isfile() and archive.extractfile(entry).read() == selected[entry.name]
    for origin in origins.values():
        if origin['kind'] == 'original_local_file':
            assert sha(Path(origin['path']).read_bytes()) == origin['sha256']
    manifest = dict(schema_version=1, scope='Round10 review checkpoint; zero original TODO closures',
                    TODO_total=192, TODO_done=163, TODO_remaining=29, TODO_closed_this_round=0,
                    archive=dict(path=packet.name, bytes=packet.stat().st_size, sha256=sha(packet.read_bytes())),
                    member_count=len(selected), member_bytes=sum(map(len, selected.values())),
                    all_roundtrip_member_bytes_equal=True, original_local_inputs_unchanged=True,
                    files=origins, complete_original_artifact_references=references)
    (OUT / 'round10-manifest.json').write_text(json.dumps(manifest, sort_keys=True, indent=2) + '\n')
    print(json.dumps({key: manifest[key] for key in ('archive', 'member_count', 'member_bytes', 'TODO_closed_this_round', 'TODO_remaining')}))

if __name__ == '__main__':
    main()
