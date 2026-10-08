#!/usr/bin/env python3
"""Package already-reviewed original ZIP members without modifying their bytes."""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import stat
import tarfile
from datetime import datetime, timezone
import zipfile

BASE = Path('/dev/shm/a217aaae3bde/platform-review')
OUT = BASE / 'raw-preservation-599'
INVENTORY = BASE / 'round6-original599-raw-persistence-inventory.json'
NATIVE = {bytes.fromhex(x) for x in ['7f454c46', 'feedface', 'cefaedfe', 'feedfacf', 'cffaedfe', 'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca']}
RECEIPT_NAMES = {'receipt.json', 'build-receipt.json', 'test-receipt.json', 'report.json', 'runner-identity.json', 'http-build-receipt.json'}

def canonical(x):
    return (json.dumps(x, ensure_ascii=False, indent=2) + '\n').encode()

def hash_stream(f):
    h = hashlib.sha256()
    n = 0
    while True:
        data = f.read(1024 * 1024)
        if not data:
            return h.hexdigest(), n
        h.update(data)
        n += len(data)

def hash_path(p):
    with p.open('rb') as f:
        return hash_stream(f)

def objects(value, pointer=''):
    if isinstance(value, dict):
        yield pointer, value
        for k, v in value.items():
            yield from objects(v, pointer + '/' + k.replace('~', '~0').replace('/', '~1'))
    elif isinstance(value, list):
        for i, v in enumerate(value):
            yield from objects(v, pointer + '/' + str(i))

def build_bindings(z):
    by_digest = {}
    for f in z.infolist():
        parts = PurePosixPath(f.filename).parts
        if f.is_dir() or parts[-1] not in RECEIPT_NAMES or any(x in parts for x in ['source', 'sources', 'reference']):
            continue
        raw = z.read(f)
        try:
            document = json.loads(raw)
        except (ValueError, UnicodeError):
            continue
        for pointer, node in objects(document):
            artifact = node.get('cargo_artifact')
            cargo_bound = isinstance(artifact, dict) and artifact.get('reason') == 'compiler-artifact' and isinstance(artifact.get('executable'), str)
            options = node.get('build_options')
            build_bound = (node.get('exit_code') == 0 and isinstance(options, dict)
                           and isinstance(options.get('command'), list) and 'build' in options['command'])
            if not (cargo_bound or build_bound):
                continue
            for key in ['binary_sha256', 'executable_sha256', 'sha256']:
                digest = node.get(key)
                if not isinstance(digest, str) or len(digest) != 64:
                    continue
                binding = {'receipt_member': f.filename, 'receipt_member_sha256': hashlib.sha256(raw).hexdigest(),
                           'json_pointer': pointer, 'binary_hash_field': key, 'binary_sha256': digest,
                           'binding_kind': 'original_cargo_artifact_and_binary_hash' if cargo_bound else 'original_successful_build_receipt_and_binary_hash'}
                for k in ['binary_bytes', 'binary_path', 'retained_executable', 'path', 'build_exit_code', 'exit_code', 'build_command', 'command', 'cargo_artifact', 'copy_source', 'build_options']:
                    if k in node:
                        binding[k] = node[k]
                if isinstance(node.get('build'), dict):
                    binding['build'] = {k: v for k, v in node['build'].items() if k in ['command', 'exit_code', 'status', 'logs_sha256']}
                by_digest.setdefault(digest, []).append(binding)
    return by_digest

class HashingReader:
    def __init__(self, wrapped):
        self.wrapped = wrapped
        self.hash = hashlib.sha256()
        self.count = 0
    def read(self, size=-1):
        data = self.wrapped.read(size)
        self.hash.update(data)
        self.count += len(data)
        return data

def add_bytes(t, name, data):
    item = tarfile.TarInfo(name)
    item.size = len(data)
    item.mode = 0o644
    t.addfile(item, io.BytesIO(data))

def package(scope, rows):
    destination = OUT / (scope.replace('_', '-') + '-original-raw.tar.gz')
    manifest_path = OUT / (scope.replace('_', '-') + '-manifest.json')
    assert not destination.exists() and not manifest_path.exists(), 'refuse overwrite'
    manifest = {'schema_version': 1, 'scope': scope, 'execution_head': '599a7050e7d52b5b7b93975c419138e175b3f754',
                'packaging_scope': 'Complete original member partition; all non-executable members preserved byte-for-byte. Native executables may be omitted only when their independently computed SHA256 matches an original Cargo/build receipt.',
                'not_original_zip': True, 'not_self_contained_executable_replay': True,
                'restore_requirement': 'For original validators requiring complete original trees, restore every omitted native executable to its original member path with exactly the recorded size/SHA256 from the untouched original ZIP. Do not alter original receipts, manifests, paths, statuses or source identity.',
                'original_zip_retention': 'All original ZIP files remain untouched in ci-artifacts; this package is an additional durable raw-record archive.',
                'artifacts': [], 'members': []}
    original_stat = {}
    with destination.open('xb') as output:
        with gzip.GzipFile(fileobj=output, mode='wb', filename='', mtime=0, compresslevel=6) as compressed:
            with tarfile.open(fileobj=compressed, mode='w|', format=tarfile.PAX_FORMAT) as t:
                for row in rows:
                    path = Path(row['original_zip_path'])
                    before = path.stat()
                    digest, size = hash_path(path)
                    assert digest == row['original_zip_sha256_from_retained_GitHub_metadata'].removeprefix('sha256:')
                    assert size == row['original_zip_bytes_size_rechecked']
                    original_stat[path] = (before.st_size, before.st_mtime_ns, before.st_ino)
                    artifact = {'id': row['artifact_id'], 'name': row['name'], 'workflow_run_id': row['workflow_run_id'],
                                'execution_head': row['execution_head'], 'original_zip_path': str(path),
                                'original_zip_sha256': digest, 'original_zip_bytes': size, 'github_expires_at': row['github_expires_at']}
                    manifest['artifacts'].append(artifact)
                    with zipfile.ZipFile(path) as z:
                        files = z.infolist()
                        names = [x.filename for x in files]
                        assert len(set(names)) == len(names), 'duplicate ZIP member'
                        bindings = build_bindings(z)
                        for f in files:
                            pure = PurePosixPath(f.filename)
                            assert not pure.is_absolute() and '..' not in pure.parts and '\\' not in f.filename
                            mode = (f.external_attr >> 16) & 0xffff
                            assert stat.S_IFMT(mode) in [0, stat.S_IFREG, stat.S_IFDIR], 'nonregular ZIP member requires explicit preservation review'
                            name = str(row['artifact_id']) + '/' + f.filename
                            entry = {'artifact_id': row['artifact_id'], 'original_member': f.filename, 'archive_member': name,
                                     'bytes': f.file_size, 'original_zip_crc32': f'{f.CRC:08x}', 'original_zip_external_attr': f.external_attr}
                            if f.is_dir():
                                info = tarfile.TarInfo(name); info.type = tarfile.DIRTYPE; info.mode = 0o755
                                t.addfile(info); entry.update(disposition='retained_directory', sha256=hashlib.sha256(b'').hexdigest())
                                manifest['members'].append(entry); continue
                            with z.open(f) as stream:
                                magic = stream.read(16)
                            if magic[:4] in NATIVE:
                                with z.open(f) as stream:
                                    binary_digest, binary_size = hash_stream(stream)
                                assert binary_size == f.file_size
                                candidates = bindings.get(binary_digest, [])
                                candidates = [x for x in candidates if x.get('binary_bytes', f.file_size) == f.file_size]
                                if candidates:
                                    def proximity(binding):
                                        a = pure.parts; b = PurePosixPath(binding['receipt_member']).parts
                                        return sum(1 for i in range(min(len(a), len(b))) if a[:i+1] == b[:i+1])
                                    binding = sorted(candidates, key=lambda x: (-proximity(x), x['receipt_member'], x['json_pointer']))[0]
                                    entry.update(disposition='omitted_receipt_bound_native_executable', sha256=binary_digest,
                                                 signature_hex=magic[:4].hex(), build_receipt_binding=binding)
                                    manifest['members'].append(entry); continue
                                entry['native_executable_without_receipt_binding_retained'] = True
                            with z.open(f) as stream:
                                wrapper = HashingReader(stream)
                                info = tarfile.TarInfo(name); info.size = f.file_size; info.mode = (mode & 0o777) or 0o644
                                t.addfile(info, wrapper)
                                assert wrapper.count == f.file_size
                                entry.update(disposition='retained_original_bytes', sha256=wrapper.hash.hexdigest())
                            manifest['members'].append(entry)
                    print(json.dumps({'packaged_artifact': row['artifact_id'], 'scope': scope}), flush=True)
                manifest['counts'] = {'original_members': len(manifest['members']),
                                      'retained_files': sum(x['disposition'] == 'retained_original_bytes' for x in manifest['members']),
                                      'omitted_native_executables': sum(x['disposition'] == 'omitted_receipt_bound_native_executable' for x in manifest['members']),
                                      'unbound_native_executables_retained': sum(x.get('native_executable_without_receipt_binding_retained', False) for x in manifest['members'])}
                encoded = canonical(manifest)
                add_bytes(t, 'PACKAGING-MANIFEST.json', encoded)
    manifest_path.write_bytes(encoded)
    expected = {x['archive_member']: x for x in manifest['members'] if x['disposition'].startswith('retained_')}
    seen = set()
    with tarfile.open(destination, mode='r|gz') as t:
        for item in t:
            assert item.name not in seen, 'duplicate new tar member'
            seen.add(item.name)
            if item.name == 'PACKAGING-MANIFEST.json':
                assert t.extractfile(item).read() == encoded
            else:
                entry = expected[item.name]
                if item.isdir():
                    assert entry['disposition'] == 'retained_directory'
                else:
                    digest, size = hash_stream(t.extractfile(item))
                    assert digest == entry['sha256'] and size == entry['bytes']
    assert seen == set(expected) | {'PACKAGING-MANIFEST.json'}
    for path, before in original_stat.items():
        after = path.stat()
        assert before == (after.st_size, after.st_mtime_ns, after.st_ino)
    digest, size = hash_path(destination)
    result = {'scope': scope, 'path': str(destination), 'bytes': size, 'sha256': digest,
              'manifest_path': str(manifest_path), 'manifest_sha256': hashlib.sha256(encoded).hexdigest(),
              'all_retained_members_reopened_and_rehashed': True, 'original_zip_sha256_verified_before_streaming': True,
              'original_zip_stat_unchanged_after_streaming': True, 'counts': manifest['counts']}
    (OUT / (scope.replace('_', '-') + '-verification.json')).write_bytes(canonical(result))
    print(json.dumps(result), flush=True)
    if size > 100_000_000:
        raise RuntimeError('Package exceeds100MB; preserve result and report before proceeding')
    return result

def main():
    OUT.mkdir(exist_ok=True)
    free = os.statvfs(OUT).f_bavail * os.statvfs(OUT).f_frsize
    assert free > 300_000_000, 'insufficient RAM scratch margin; report before proceeding'
    inventory = json.loads(INVENTORY.read_text())
    results = []
    for scope in ['platform_recovery', 'gates', 'p7_engineering', 'p7_closeout']:
        results.append(package(scope, [x for x in inventory['original_artifacts'] if x['scope'] == scope]))
    report = {'schema_version': 1, 'created_at_utc': datetime.now(timezone.utc).isoformat(), 'packager': '/root/pr_audit',
              'method': 'One-time raw-record packaging from already reviewed immutable originalZIPs; not a rerun of acceptance or measurements.',
              'script_sha256': hash_path(Path(__file__))[0], 'inventory_sha256': hash_path(INVENTORY)[0], 'results': results,
              'source_ref_or_root_mutations': [], 'task_counts': {'total': 192, 'done': 163, 'remaining': 29},
              'pending': 'Independent parent package review and persistence in final task/evidence PR; these temporary files alone are not Git persistence.'}
    (OUT / 'preservation-verification.json').write_bytes(canonical(report))
    print(json.dumps({'completed': True, 'report': str(OUT / 'preservation-verification.json'), 'total_archive_bytes': sum(x['bytes'] for x in results)}), flush=True)

if __name__ == '__main__':
    main()
