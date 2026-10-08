"""Derive a byte-preserving non-ELF evidence bundle from an accepted original ZIP.

Only receipt-bound ELF executable members may be omitted. This derivative is
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

HEAD = '29682890c89511dd6f477a6bf48bd969aa1537af'
RUN = 37844310853
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
assert review['verdict'].startswith('accepted_scoped_original_29682890_')
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
    if 'receipt.json' in archive.namelist():
        receipt_name = 'receipt.json'
        receipt_bytes = archive.read(receipt_name)
        receipt = json.loads(receipt_bytes)
        assert receipt['cargo_artifact']['target']['name'] == 'p7_worker_contention'
        assert receipt['cargo_artifact']['executable'] == receipt['copy_source']['path']
        assert receipt['executable_sha256'] == receipt['copy_source']['sha256']
        bindings = {'p7_worker_contention': {
            'sha256': receipt['executable_sha256'], 'bytes': receipt['copy_source']['bytes'],
            'receipt_field': 'executable_sha256 + copy_source + cargo_artifact.executable'}}
    else:
        receipt_name = 'p8-build/build-receipt.json'
        receipt_bytes = archive.read(receipt_name)
        receipt = json.loads(receipt_bytes)
        assert set(receipt['artifacts']) == {'codecortex', 'p8-oracle', 'p8-runtime-statistics'}
        bindings = {}
        for name, item in receipt['artifacts'].items():
            assert item['cargo_artifact']['target']['name'] == name
            assert item['cargo_artifact']['executable'] == item['copy_source']['path']
            assert item['binary_sha256'] == item['copy_source']['sha256']
            bindings['p8-build/' + name] = {
                'sha256': item['binary_sha256'], 'bytes': item['binary_bytes'],
                'receipt_field': 'artifacts.' + name + '.binary_sha256/copy_source/cargo_artifact.executable'}
    receipt_sha = hashlib.sha256(receipt_bytes).hexdigest()
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
    # Stop before allocating a large derivative; the caller can report this size.
    assert kept_uncompressed <= 200 * 1024 * 1024, ('report large bundle before writing', kept_uncompressed)
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
                        assert reader.count == entry.file_size == binding['bytes']
                        assert reader.digest.hexdigest() == binding['sha256']
                        omitted.append({'path': name, 'bytes': reader.count, 'sha256': reader.digest.hexdigest(),
                                        'reason': 'receipt-bound ELF ET_EXEC/ET_DYN executable only',
                                        'receipt_path': receipt_name, 'receipt_sha256': receipt_sha,
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
    'schema_version': 1, 'kind': 'derived_non_elf_original_member_bundle',
    'not_complete_original_zip': True, 'acceptance_rerun': False,
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
