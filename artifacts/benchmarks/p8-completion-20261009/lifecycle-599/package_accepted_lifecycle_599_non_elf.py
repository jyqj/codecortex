"""Derive a byte-preserving non-ELF evidence bundle from an accepted original ZIP.

Fixed original 599 lifecycle artifact only; only receipt-bound ELF executable members may be omitted. This derivative is
explicitly NOT the complete original ZIP. No acceptance workload is rerun.
"""
import calendar
import gzip
import hashlib
import json
from pathlib import Path
import sys
import tarfile
import zipfile

HEAD = '599a7050e7d52b5b7b93975c419138e175b3f754'
RUN = 37835809247
zip_path, review_path, output_root = map(Path, sys.argv[1:])

def digest_file(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(chunk)
    return h.hexdigest()

class HashReader:
    def __init__(self, stream):
        self.stream = stream
        self.digest = hashlib.sha256()
        self.count = 0
    def read(self, count=-1):
        data = self.stream.read(count)
        self.digest.update(data)
        self.count += len(data)
        return data

review = json.loads(review_path.read_text())
metadata_path = zip_path.parent / 'metadata.json'
metadata = json.loads(metadata_path.read_text())
aid = metadata['id']
assert review['head'] == metadata['workflow_run']['head_sha'] == HEAD
assert review['workflow_run'] == metadata['workflow_run']['id'] == RUN
assert review['artifact_id'] == aid and review.get('unresolved_blockers') == []
assert aid == 11576107053
assert review['status'] == 'accepted_scoped_actual_lifecycle_observation'
zip_sha = digest_file(zip_path)
assert metadata['digest'] == 'sha256:' + zip_sha and review['zip_sha256'] == zip_sha
assert zip_path.stat().st_size == metadata['size_in_bytes'] == review['zip_bytes']
output_root.mkdir(parents=True, exist_ok=True)
bundle = output_root / (str(aid) + '-non-elf-original-members.tar.gz')
manifest_path = output_root / (str(aid) + '-non-elf-derivation.json')
assert not bundle.exists() and not manifest_path.exists()
included, omitted, directories = [], [], []

with zipfile.ZipFile(zip_path) as archive:
    entries = archive.infolist()
    assert len({entry.filename for entry in entries}) == len(entries)
    for entry in entries:
        path = Path(entry.filename)
        assert not path.is_absolute() and '..' not in path.parts
    # These original lifecycle receipts bind copied bytes by SHA, not size;
    # report each ZIP member's verified size without inventing a receipt field.
    product_receipt_path = 'product/build-receipt.json'
    replay_receipt_path = 'replay-build-receipt.json'
    product_bytes = archive.read(product_receipt_path)
    replay_bytes = archive.read(replay_receipt_path)
    product = json.loads(product_bytes)
    replay = json.loads(replay_bytes)
    assert product['cargo_artifact']['target']['name'] == 'codecortex'
    assert product['cargo_artifact']['target']['kind'] == ['bin']
    assert product['binary_path'].endswith('/product/codecortex')
    assert product['binary_sha256'] == review['product_sha256']
    assert replay['artifact']['target']['name'] == 'p8-measurements'
    assert replay['artifact']['target']['kind'] == ['bin']
    assert replay['artifact']['executable'] == replay['copy_source']['path']
    assert replay['binary_sha256'] == replay['copy_source']['sha256'] == review['evaluator_sha256']
    bindings = {
        'product/codecortex': {
            'sha256': product['binary_sha256'],
            'receipt_path': product_receipt_path,
            'receipt_sha256': hashlib.sha256(product_bytes).hexdigest(),
            'receipt_field': 'binary_path + binary_sha256 + cargo_artifact.target; size independently measured from original ZIP, not present in receipt'},
        'p8-measurements': {
            'sha256': replay['binary_sha256'],
            'receipt_path': replay_receipt_path,
            'receipt_sha256': hashlib.sha256(replay_bytes).hexdigest(),
            'receipt_field': 'binary_sha256 + copy_source.sha256/path + artifact.executable; size independently measured from original ZIP, not present in receipt'},
    }
    omit_names = set()
    for entry in entries:
        if entry.filename not in bindings or entry.is_dir():
            continue
        with archive.open(entry) as stream:
            header = stream.read(64)
        if header[:4] == b'\x7fELF' and len(header) >= 18 and header[4] in (1, 2) and header[5] in (1, 2):
            elf_type = int.from_bytes(header[16:18], 'little' if header[5] == 1 else 'big')
            if elf_type in (2, 3):
                omit_names.add(entry.filename)
    kept_uncompressed = sum(entry.file_size for entry in entries if entry.filename not in omit_names)
    # Inventory (344,752,411 retained bytes; ~40 MB compressed) was reported
    # before this streaming write. No original database or raw file is dropped.
    assert omit_names == set(bindings)
    assert len([entry for entry in entries if not entry.is_dir()]) == 2391
    assert kept_uncompressed == 344752411
    with bundle.open('xb') as raw_bundle, gzip.GzipFile(fileobj=raw_bundle, mode='wb', filename='', mtime=0) as zipped:
        with tarfile.open(fileobj=zipped, mode='w|', format=tarfile.PAX_FORMAT) as tar:
            for entry in entries:
                name = entry.filename
                info = tarfile.TarInfo(name)
                info.mtime = calendar.timegm(entry.date_time + (0, 0, 0))
                info.mode = (entry.external_attr >> 16) & 0o777 or (0o755 if entry.is_dir() else 0o644)
                if entry.is_dir():
                    info.type = tarfile.DIRTYPE
                    tar.addfile(info)
                    directories.append(name.rstrip('/'))
                    continue
                with archive.open(entry) as stream:
                    reader = HashReader(stream)
                    if name in omit_names:
                        while reader.read(1024 * 1024):
                            pass
                        binding = bindings[name]
                        assert reader.count == entry.file_size
                        assert reader.digest.hexdigest() == binding['sha256']
                        omitted.append({'path': name, 'bytes': reader.count, 'sha256': reader.digest.hexdigest(),
                                        'reason': 'receipt-bound ELF ET_EXEC/ET_DYN executable only',
                                        'receipt_path': binding['receipt_path'], 'receipt_sha256': binding['receipt_sha256'],
                                        'receipt_binding': binding['receipt_field']})
                    else:
                        info.size = entry.file_size
                        tar.addfile(info, reader)
                        assert reader.count == entry.file_size
                        included.append({'path': name, 'bytes': reader.count, 'sha256': reader.digest.hexdigest()})

# Independently reopen the gzip/tar and hash every retained member, checking the
# complete path set as well as bytes and sizes. Unlisted ELF members stay retained.
expected = {item['path']: item for item in included}
seen, seen_dirs = set(), set()
with tarfile.open(bundle, mode='r|gz') as tar:
    for entry in tar:
        if entry.isdir():
            seen_dirs.add(entry.name.rstrip('/'))
            continue
        assert entry.isfile() and entry.name not in seen and entry.name in expected
        seen.add(entry.name)
        stream = tar.extractfile(entry)
        assert stream is not None
        reader = HashReader(stream)
        while reader.read(1024 * 1024):
            pass
        assert reader.count == entry.size == expected[entry.name]['bytes']
        assert reader.digest.hexdigest() == expected[entry.name]['sha256']
assert seen == set(expected) and seen_dirs == set(directories)
assert digest_file(zip_path) == zip_sha
manifest = {
    'schema_version': 1, 'kind': 'derived_non_elf_original_lifecycle_member_bundle',
    'not_complete_original_zip': True, 'acceptance_rerun': False,
    'scope_todos': ['P8-008', 'P8-009'],
    'source_head': HEAD, 'workflow_run': RUN, 'artifact_id': aid,
    'original_zip': {'path': str(zip_path), 'bytes': zip_path.stat().st_size, 'sha256': zip_sha,
                     'api_url': metadata['url'],
                     'github_url': 'https://github.com/jyqj/codecortex/actions/runs/' + str(RUN) + '/artifacts/' + str(aid),
                     'preserved_unchanged': True},
    'accepted_review': {'path': str(review_path), 'sha256': digest_file(review_path)},
    'bundle': {'path': str(bundle), 'bytes': bundle.stat().st_size, 'sha256': digest_file(bundle)},
    'included_members': included, 'included_uncompressed_bytes': kept_uncompressed,
    'included_member_count': len(included), 'directory_members': directories,
    'omitted_receipt_bound_elf_executables': omitted,
    'every_retained_member_independently_rehashed_after_tar_write': True,
    'scope': 'All original ZIP members retained byte-for-byte except listed receipt-bound actual ELF executables. Original seal files are retained unchanged, but full seal verification/replay requires restoring the omitted binaries from the original ZIP. This bundle is not the complete ZIP.',
    'packager': {'path': str(Path(__file__).resolve()), 'sha256': digest_file(Path(__file__))},
}
manifest_path.write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
print(json.dumps({'artifact_id': aid, 'bundle': manifest['bundle'], 'manifest': str(manifest_path),
                  'manifest_sha256': digest_file(manifest_path), 'included_members': len(included),
                  'omitted_elf': len(omitted), 'included_uncompressed_bytes': kept_uncompressed}, indent=2))
