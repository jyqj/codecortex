#!/usr/bin/env python3
"""Plan raw custody, retaining exact original ZIP bytes; no workload or ref write."""
from pathlib import Path
import hashlib
import json
import stat

BASE = Path('/workspace/scratch/a217aaae3bde')
REVIEW = BASE / 'scale-pr180-C-review'
OUT = Path(__file__).resolve().parent
PREFIX = 'artifacts/benchmarks/original-custody-C3ff-scale-37910924354-20261009'
HEAD = '3ffcefc3b28ee1a4ed80caecebd7208a45c3e302'
PARENT = '1d56747eafcef6c22ceaec949fabe30a3362862c'
CHUNK_BYTES = 2097152
IDS = [11607655032, 11610312063, 11610456545, 11610782288, 11611019227]

def sha(b): return hashlib.sha256(b).hexdigest()
def oid(b): return hashlib.sha1(b'blob ' + str(len(b)).encode() + b'\0' + b).hexdigest()
def canonical(v): return (json.dumps(v, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
def write_new(p, b):
    assert not p.exists() and not p.is_symlink(), str(p)
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.open('xb') as f: f.write(b)
    assert p.read_bytes() == b
def signature(s): return [s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns]

admitted = json.loads((REVIEW / 'admitted-subset-004.json').read_bytes())
assert admitted['admitted_shards'] == 4 and admitted['admitted_samples'] == 41
chunks = {}
records = []
proof = []
elements = []
for identity in IDS:
    source = REVIEW / f'artifacts/{identity}/{identity}.zip'
    meta_path = REVIEW / f'artifact-metadata/{identity}.json'
    metadata = json.loads(meta_path.read_bytes())
    before = source.stat()
    assert stat.S_ISREG(before.st_mode) and not source.is_symlink()
    assert metadata['id'] == identity and metadata['workflow_run']['id'] == 37910924354
    assert metadata['workflow_run']['head_sha'] == HEAD
    expected = metadata['digest'].removeprefix('sha256:')
    whole = hashlib.sha256()
    ordered = []
    offset = 0
    with source.open('rb') as f:
        while block := f.read(CHUNK_BYTES):
            whole.update(block)
            digest = sha(block)
            row = {'path': f'chunks/{digest}.bin', 'offset': offset, 'bytes': len(block), 'sha256': digest, 'git_blob': oid(block)}
            ordered.append(row)
            transport = {'source_zip': str(source), 'source_offset': offset,
                         'path': PREFIX + '/' + row['path'], 'bytes': len(block), 'sha256': digest, 'git_blob': row['git_blob'], 'mode': '100644'}
            old = chunks.setdefault(digest, transport)
            assert old['bytes'] == len(block) and old['git_blob'] == row['git_blob']
            offset += len(block)
    assert before.st_size == metadata['size_in_bytes'] == offset and whole.hexdigest() == expected
    after = source.stat()
    assert signature(before) == signature(after)
    second = hashlib.sha256()
    for part in ordered:
        with source.open('rb') as f:
            f.seek(part['offset']); block = f.read(part['bytes'])
        assert len(block) == part['bytes'] and sha(block) == part['sha256'] and oid(block) == part['git_blob']
        second.update(block)
    assert second.hexdigest() == expected and signature(source.stat()) == signature(before)
    if identity == IDS[0]:
        review_path = REVIEW / 'recovery/R27/build-init-original-review.json'
        location = {'commit': '76369a0306026ebd90239b700f3ba16138a24c5c',
                    'path': 'artifacts/checkpoints/p8-round27-a217-20261009/C3ff-scale/build-init-original-review.json',
                    'git_blob': 'f179b938ce1ae065c4a757ea5bc85ab1f15118b4'}
        scope = 'original-fixed-release-build-identity; zero measurement samples'
        samples = 0
    else:
        review_path = REVIEW / f'validated/shards/{identity}/review.json'
        round_num = 29 if identity == 11611019227 else 28
        inventory = json.loads((REVIEW / f'recovery/r{round_num}_inventory.json').read_bytes())
        item = next(x for x in inventory['files'] if x['source'] == str(review_path.relative_to(BASE)))
        location = {'commit': '76369a0306026ebd90239b700f3ba16138a24c5c',
                    'container_path': f'artifacts/checkpoints/p8-round{round_num}-a217-20261009/round{round_num}-public-evidence.tar.gz',
                    'container_git_blob': inventory['container']['git_blob'],
                    'container_sha256': inventory['container']['sha256'], 'member': item['target']}
        samples = next(x['sample_count'] for x in admitted['records'] if x['artifact_id'] == identity)
        scope = 'accepted original rep0 shard only; full matrix and 100k remain pending'
    review_bytes = review_path.read_bytes()
    record = {'schema': 'codecortex-original-actions-zip-custody-v1',
              'artifact_id': identity, 'artifact_name': metadata['name'], 'run_id': 37910924354,
              'scope': scope, 'source_commit': HEAD, 'actual_execution_checkout': HEAD,
              'original_zip_filename': f'{identity}.zip', 'original_zip_bytes': offset,
              'original_zip_sha256': expected, 'official_artifact_locator': metadata['url'],
              'official_created_at': metadata['created_at'], 'official_expires_at': metadata['expires_at'],
              'chunk_bytes': CHUNK_BYTES, 'chunks': ordered,
              'reconstruction': 'concatenate all chunk bytes in ascending offset; no ZIP repacking',
              'accepted_measurement_samples': samples,
              'acceptance_reviews': [{'bytes': len(review_bytes), 'sha256': sha(review_bytes),
                                      'git_blob': oid(review_bytes), 'published_location': location}],
              'recovery_transport_scope': 'The same immutable Actions original was retrieved again after scratch loss. This is not a first/unique download or a repeated native measurement.',
              'new_native_execution': False, 'new_task_completion': False}
    payload = canonical(record)
    path = OUT / f'manifests/{identity}.json'
    write_new(path, payload)
    elements.append({'source': str(path), 'path': PREFIX + f'/manifests/{identity}.json', 'mode': '100644', 'bytes': len(payload), 'sha256': sha(payload), 'git_blob': oid(payload)})
    records.append({'artifact_id': identity, 'bytes': offset, 'sha256': expected, 'chunks': len(ordered),
                    'manifest': f'manifests/{identity}.json', 'manifest_bytes': len(payload), 'manifest_sha256': sha(payload),
                    'accepted_measurement_samples': samples})
    proof.append({'artifact_id': identity, 'source_zip': str(source), 'stat_before_after': signature(before),
                  'bytes': offset, 'sha256': expected, 'second_contiguous_readback_sha256': second.hexdigest(),
                  'ordered_chunk_count': len(ordered), 'metadata_sha256': sha(meta_path.read_bytes()), 'source_unchanged': True})

catalog = {'schema': 'codecortex-original-actions-custody-catalog-v1', 'source_commit': HEAD,
           'source_tree': '0e655b4258afd9ca040dd7c1ace868bf1c322a30', 'run_id': 37910924354,
           'artifact_count': len(records), 'artifacts': records, 'original_zip_bytes': sum(r['bytes'] for r in records),
           'unique_chunk_count': len(chunks), 'unique_chunk_bytes': sum(r['bytes'] for r in chunks.values()),
           'accepted_shards': 4, 'accepted_samples': 41, 'required_shards': 150, 'required_samples': 1500,
           'registered_repetitions': 30, 'existing_custody_preserved_at': PARENT,
           'existing_custody_prefix': 'artifacts/benchmarks/original-custody-C3ff-20261009',
           'existing_catalog_git_blob': 'a1bb2d4b3be20e4ed9b6a326175bd9f170559e6b',
           'publication_state': 'planned local inputs only; no new Git object, commit, ref or durable-custody claim',
           'transport_only': True, 'new_native_execution': False, 'new_task_completion': False}
write_new(OUT / 'catalog-planned.json', canonical(catalog))
write_new(OUT / 'source-byte-preservation-proof.json', canonical({'schema': 'scale-five-original-ZIP-plan-byte-proof-v1', 'artifacts': proof, 'zip_members_extracted': False, 'helper_replay': False, 'native_execution': False}))
readme = ('# Fixed C3ff scale original ZIP custody\n\n'
          'This sibling collection preserves one original release build and four already accepted repetition-0 shards from run 37910924354 at 3ffcefc3b28ee1a4ed80caecebd7208a45c3e302. '
          'The originals are split into ordered 2 MiB chunks with exact whole and chunk hashes; no ZIP is repacked. The existing 26-artifact custody tree and catalog remain unchanged.\n\n'
          'The input copies were recovered by repeated transport of the same immutable Actions artifact IDs after scratch loss. No workload or already accepted shard was rerun. '
          'The accepted scope remains 4/150 shards and 41/1500 samples. The original 100k job and complete matrix remain pending; N30, all budgets and the original validator are unchanged.\n\n'
          'Use the unchanged restore_original_zip.py with one manifests/<artifact-id>.json and a previously absent --output path. It verifies every chunk and the full ZIP, does not extract archives, and refuses an existing output.\n')
write_new(OUT / 'README.md', readme.encode())
restorer = OUT / 'restore_original_zip.py'
assert len(restorer.read_bytes()) == 4339 and oid(restorer.read_bytes()) == 'b4972957fa531e3ad64fe131a075700f8e3e0ff2'
for name in ('catalog-planned.json', 'source-byte-preservation-proof.json', 'README.md', 'restore_original_zip.py', 'prepare_plan.py'):
    p = OUT / name; b = p.read_bytes()
    elements.append({'source': str(p), 'path': PREFIX + '/' + name, 'mode': '100644', 'bytes': len(b), 'sha256': sha(b), 'git_blob': oid(b)})
elements.extend(sorted(chunks.values(), key=lambda x: x['path']))
assert len({x['path'] for x in elements}) == len(elements)
plan = {'schema': 'scale-five-ZIP-sibling-custody-append-plan-v1',
        'expected_branch': 'evidence/p8-originals-c3ff-a217-20261009', 'expected_parent': PARENT,
        'expected_parent_tree': '4ebcb772f9bd4d0732b93d4c3e0de12a006b82e0',
        'new_prefix': PREFIX, 'only_new_prefix_additions': True, 'elements': elements,
        'artifact_count': 5, 'original_zip_bytes': catalog['original_zip_bytes'],
        'chunk_count': len(chunks), 'file_count': len(elements),
        'existing_26_artifacts_and_catalog': 'all unchanged; no edits outside new sibling prefix',
        'implementation': 'unchanged public restore helper b4972957fa531e3ad64fe131a075700f8e3e0ff2 and custody manifest v1',
        'accepted_shards': 4, 'accepted_samples': 41, 'formal_completion': False, 'remaining_todos': 29,
        'remote_objects_created': False, 'refs_changed': False, 'native_or_helper_replay': False}
write_new(OUT / 'append-plan.json', canonical(plan))
print(json.dumps({'path': str(OUT / 'append-plan.json'), 'bytes': (OUT / 'append-plan.json').stat().st_size,
                  'sha256': sha((OUT / 'append-plan.json').read_bytes()), 'files': len(elements),
                  'chunks': len(chunks), 'original_bytes': catalog['original_zip_bytes']}))
