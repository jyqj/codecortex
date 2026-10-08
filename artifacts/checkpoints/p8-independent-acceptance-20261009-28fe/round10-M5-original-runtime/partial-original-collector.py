"""Archive interrupted M5 outputs as data without opening DBs or executing originals."""
import datetime
import hashlib
import io
import json
import os
import pathlib
import stat
import tarfile
import time

ROOT = pathlib.Path('/workspace/scratch/28fef0db5e01')
OUT = ROOT / 'runtime-review/round9-partial-execution-preservation'
ARCHIVE = OUT / 'M5-interrupted-soak-and-backfill-originals.tar.xz'
ROOTS = ('runtime-review/m5-local-soak', 'runtime-review/m5-local-backfill')
CONTEXT = 'runtime-review/round9-execution-loss/interruption-observation.json'
EXCLUDED = {
    'runtime-review/m5-local-soak/source': 'reconstructable fixed M5 checkout',
    'runtime-review/m5-local-soak/cargo-home': 'reconstructable copied dependency registry',
    'runtime-review/m5-local-soak/private-target': 'reconstructable native build target',
    'runtime-review/m5-local-backfill/private-target': 'reconstructable semantic build target',
    'runtime-review/m5-local-soak/build/codecortex': 'native binary already independently archived; preserve receipt binding only',
    'runtime-review/m5-local-soak/build/p8-oracle': 'native binary already independently archived; preserve receipt binding only',
    'runtime-review/m5-local-soak/build/p8-runtime-statistics': 'native binary already independently archived; preserve receipt binding only',
}

def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def digest_file(path):
    h = hashlib.sha256()
    with path.open('rb') as f:
        while chunk := f.read(1024 * 1024):
            h.update(chunk)
    return h.hexdigest()

def snapshot_stat(path):
    s = path.lstat()
    return {'bytes': s.st_size, 'mode': stat.S_IMODE(s.st_mode), 'mtime_ns': s.st_mtime_ns, 'ctime_ns': s.st_ctime_ns, 'inode': s.st_ino, 'device': s.st_dev, 'type': 'file' if stat.S_ISREG(s.st_mode) else 'directory' if stat.S_ISDIR(s.st_mode) else 'unsupported'}

def inventory():
    out = {}
    def visit(path):
        name = path.relative_to(ROOT).as_posix()
        if name in EXCLUDED:
            return
        s = snapshot_stat(path)
        if s['type'] == 'unsupported':
            raise ValueError('Non-regular original path: ' + name)
        out[name] = s
        if s['type'] == 'directory':
            for child in sorted(path.iterdir()):
                visit(child)
    for name in ROOTS:
        visit(ROOT / name)
    visit(ROOT / CONTEXT)
    return out

class HashingReader:
    def __init__(self, stream):
        self.stream = stream
        self.hash = hashlib.sha256()
        self.count = 0
    def read(self, n=-1):
        block = self.stream.read(n)
        self.hash.update(block)
        self.count += len(block)
        return block

if ARCHIVE.exists():
    raise SystemExit('Refusing to overwrite an existing original preservation archive')
started = now()
begin = time.monotonic()
before = inventory()
files = []
anomalies = []
with tarfile.open(ARCHIVE, mode='w:xz', preset=3, format=tarfile.PAX_FORMAT) as tar:
    for name, initial in before.items():
        path = ROOT / name
        current = snapshot_stat(path)
        if current != initial:
            anomalies.append({'path': name, 'kind': 'changed_between_initial_inventory_and_capture', 'before': initial, 'current': current})
        info = tarfile.TarInfo(name)
        info.mode = initial['mode']
        info.mtime = initial['mtime_ns'] / 1_000_000_000
        if initial['type'] == 'directory':
            info.type = tarfile.DIRTYPE
            tar.addfile(info)
            continue
        info.size = current['bytes']
        with path.open('rb') as stream:
            reader = HashingReader(stream)
            tar.addfile(info, reader)
        after_read = snapshot_stat(path)
        if after_read != current:
            anomalies.append({'path': name, 'kind': 'changed_during_capture', 'before': current, 'after': after_read})
        files.append({'path': name, 'bytes': reader.count, 'sha256': reader.hash.hexdigest(), 'initial_stat': initial, 'capture_before': current, 'capture_after': after_read})
    after = inventory()
    for name in sorted(set(before) | set(after)):
        if before.get(name) != after.get(name):
            anomalies.append({'path': name, 'kind': 'inventory_or_metadata_changed', 'before': before.get(name), 'after': after.get(name)})
    for item in files:
        path = ROOT / item['path']
        try:
            s0 = snapshot_stat(path)
            hash_after = digest_file(path)
            s1 = snapshot_stat(path)
            item['post_capture_sha256'] = hash_after
            item['post_capture_stat'] = s1
            item['post_capture_matches'] = hash_after == item['sha256'] and s0 == s1 == item['capture_after']
            if not item['post_capture_matches']:
                anomalies.append({'path': item['path'], 'kind': 'post_capture_hash_or_metadata_changed'})
        except OSError as exc:
            item['post_capture_matches'] = False
            anomalies.append({'path': item['path'], 'kind': 'post_capture_unreadable', 'error': repr(exc)})
    final_inventory = inventory()
    if final_inventory != after:
        anomalies.append({'kind': 'inventory_changed_during_final_hash_verification'})
    original_receipts = {}
    for base in ROOTS:
        name = base + '/execution-receipt.json'
        receipt = json.loads((ROOT / name).read_bytes())
        original_receipts[name] = {'status': receipt['status'], 'phases': [{'name': p['name'], 'status': p.get('status'), 'exit_code': p.get('exit_code')} for p in receipt['phases']], 'no_termination_invented': True}
    manifest = {
        'schema_version': 1, 'kind': 'partial_original_execution_byte_preservation',
        'collector': '/root/runtime_review', 'capture_started_at_utc': started,
        'source_checks_finished_at_utc': now(),
        'source_commit': 'fec0698c7fa4d76076b828cc17cf277ad8b307e0',
        'source_roots': list(ROOTS), 'excluded_exact_paths_or_subtrees': EXCLUDED,
        'source_directory_entries': len(before), 'original_file_count': len(files),
        'original_file_bytes': sum(f['bytes'] for f in files),
        'observed_stable_during_capture': not anomalies, 'anomalies': anomalies,
        'original_receipts': original_receipts,
        'limitations': [
            'This is preservation of partial output after observed session loss, not completed execution, acceptance, or a new seal.',
            'Original running receipts are retained unchanged; no terminal exit code or verifier result is invented.',
            'Stable before/after bytes cover only this capture interval, not an atomic execution snapshot or proof of host-wide process absence.',
            'SQLite and WAL/SHM bytes are opaque files; no database connection, checkpoint, replay, repair, or consistency certification was performed.',
            'Product source checkout, dependency cache, targets and previously archived native binaries are excluded; original source/binary hash bindings and observer-source evidence remain included.',
            'All project fixture files and intermediate result/log bytes still present at capture are included; absent original output is not fabricated.',
        ],
        'ledger': {'total': 192, 'done': 163, 'remaining': 29, 'new_completions': 0},
        'directories': {k:v for k,v in before.items() if v['type'] == 'directory'},
        'files': files,
    }
    manifest_bytes = (json.dumps(manifest, ensure_ascii=False, indent=2) + '\n').encode()
    info = tarfile.TarInfo('preservation-manifest.json')
    info.size = len(manifest_bytes)
    info.mode = 0o644
    tar.addfile(info, io.BytesIO(manifest_bytes))
    (OUT / 'preservation-manifest.json').write_bytes(manifest_bytes)
expected = {x['path']: (x['bytes'], x['sha256']) for x in files}
expected['preservation-manifest.json'] = (len(manifest_bytes), hashlib.sha256(manifest_bytes).hexdigest())
seen = set()
member_checks = []
with tarfile.open(ARCHIVE, mode='r:xz') as tar:
    for member in tar:
        if member.isdir():
            assert member.name in before and before[member.name]['type'] == 'directory'
            continue
        assert member.isfile() and member.name not in seen and member.name in expected
        seen.add(member.name)
        h = hashlib.sha256()
        count = 0
        with tar.extractfile(member) as stream:
            while chunk := stream.read(1024 * 1024):
                h.update(chunk)
                count += len(chunk)
        assert (count, h.hexdigest()) == expected[member.name], member.name
        member_checks.append({'path': member.name, 'bytes': count, 'sha256': h.hexdigest()})
assert seen == set(expected)
report = {
    'schema_version': 1, 'kind': 'interrupted_M5_originals_preservation_receipt',
    'completed_at_utc': now(), 'elapsed_seconds': time.monotonic() - begin,
    'status': 'partial_evidence_preserved_stable_during_capture' if not anomalies else 'partial_evidence_preserved_but_source_unstable',
    'archive': {'name': ARCHIVE.name, 'bytes': ARCHIVE.stat().st_size, 'sha256': digest_file(ARCHIVE)},
    'manifest': {'name': 'preservation-manifest.json', 'bytes': len(manifest_bytes), 'sha256': hashlib.sha256(manifest_bytes).hexdigest()},
    'original_file_count': len(files), 'original_file_bytes': sum(f['bytes'] for f in files),
    'all_archive_regular_members_read_back_and_hash_verified': True,
    'verified_regular_members_including_manifest': len(member_checks),
    'source_before_after_stable': not anomalies, 'anomaly_count': len(anomalies),
    'original_receipts_unchanged_running': all(v['status'] == 'running' for v in original_receipts.values()),
    'original_execution_completion_claimed': False, 'new_product_seal_created': False,
    'artifact_code_or_binary_executed': False, 'database_opened': False,
    'source_directories_modified': False, 'publication_performed': False,
}
(OUT / 'preservation-receipt.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report), flush=True)
