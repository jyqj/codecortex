"""Measure a future frozen mixed evidence archive without creating a second copy."""
import datetime
import hashlib
import json
import lzma
import pathlib
import stat
import tarfile

ROOT = pathlib.Path('/workspace/scratch/28fef0db5e01')
BASE = ROOT / 'runtime-review/round9-m5-mixed-review'
OUT = ROOT / 'runtime-review/round9-partial-execution-preservation'
manifest_path = BASE / 'evidence-manifest.json'
old = json.loads(manifest_path.read_bytes())['files']
selected = set(old) | {'evidence-manifest.json', 'final-independent-handoff.json'}

class Counter:
    def __init__(self):
        self.compressor = lzma.LZMACompressor(format=lzma.FORMAT_XZ, preset=3)
        self.count = 0
        self.sha = hashlib.sha256()
    def write(self, block):
        compressed = self.compressor.compress(block)
        self.count += len(compressed)
        self.sha.update(compressed)
        return len(block)
    def finish(self):
        compressed = self.compressor.flush()
        self.count += len(compressed)
        self.sha.update(compressed)

class Reader:
    def __init__(self, stream):
        self.stream = stream
        self.count = 0
        self.sha = hashlib.sha256()
    def read(self, n=-1):
        b = self.stream.read(n)
        self.count += len(b)
        self.sha.update(b)
        return b

counter = Counter()
entries = []
with tarfile.open(fileobj=counter, mode='w|', format=tarfile.PAX_FORMAT) as tar:
    for name in sorted(selected):
        path = BASE / name
        before = path.stat()
        assert path.is_file() and not path.is_symlink()
        member = 'runtime-review/round9-m5-mixed-review/' + name
        info = tarfile.TarInfo(member)
        info.size = before.st_size
        info.mode = stat.S_IMODE(before.st_mode)
        info.mtime = 0
        with path.open('rb') as source:
            reader = Reader(source)
            tar.addfile(info, reader)
        after = path.stat()
        assert (before.st_size, before.st_mtime_ns, before.st_ctime_ns, before.st_ino) == (after.st_size, after.st_mtime_ns, after.st_ctime_ns, after.st_ino)
        if name in old:
            assert (reader.count, reader.sha.hexdigest()) == (old[name]['bytes'], old[name]['sha256']), name
        entries.append({'path': name, 'bytes': reader.count, 'sha256': reader.sha.hexdigest()})
counter.finish()
partial = json.loads((OUT / 'preservation-receipt.json').read_bytes())
report = {
    'schema_version': 1, 'measured_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'kind': 'frozen_M5_four_mixed_evidence_package_size_plan',
    'proposed_scope': 'Original 99-file independent mixed evidence inventory, already including the final handoff, plus its original manifest: 100 files total, including all 84 failed-originals files.',
    'original_file_count': len(entries), 'original_file_bytes': sum(e['bytes'] for e in entries),
    'failed_originals_count': sum(e['path'].startswith('failed-originals/') for e in entries),
    'exact_compressed_stream_size_for_this_recipe': counter.count,
    'compressed_stream_sha256_for_this_recipe': counter.sha.hexdigest(),
    'recipe': 'sorted regular files, POSIX PAX tar, original permission mode, uid/gid 0, empty owner names, mtime 0, no directory records, XZ preset3/CRC64; tarfile streaming buffer defaults',
    'compressed_bytes_retained': False,
    'no_duplicate_large_copy_created': True,
    'all_original_manifest_bytes_rechecked': True,
    'already_saved_partial_archive_bytes': partial['archive']['bytes'],
    'separate_two_archive_combined_bytes': partial['archive']['bytes'] + counter.count,
    'recommendation': 'Git can persist the saved partial archive and a separate frozen mixed archive with their manifests. No need to expand or duplicate the 173MB failed-originals directory. Do not replace original failure statuses or claim complete runtime acceptance.',
    'files': entries,
}
(OUT / 'mixed-package-size-plan.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k:v for k,v in report.items() if k != 'files'}))
