"""Root's independent archival comparison. No product acceptance is rerun."""
from pathlib import Path
from contextlib import ExitStack
import collections
import datetime
import hashlib
import json
import shutil
import tarfile
import zipfile

W = Path('/dev/shm/a217aaae3bde')
SRC = W / 'platform-review/raw-preservation-599'
REPO = W / 'codecortex'
DST = REPO / 'artifacts/benchmarks/p8-completion-20261009/platform-p7-gates-599'
HEAD = '599a7050e7d52b5b7b93975c419138e175b3f754'


def sha(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1048576), b''):
            h.update(block)
    return h.hexdigest()


def safe(name):
    p = Path(name)
    assert not p.is_absolute() and '..' not in p.parts and '\\' not in name


vp = SRC / 'preservation-verification.json'
assert sha(vp) == '69b8d0686f749c0f85057cfada0fef9eddfc42c75829d290e6b86b06bf3c09a7'
verification = json.loads(vp.read_text())
packager = W / 'platform-review/preserve-original599-raw.py'
assert sha(packager) == verification['script_sha256'] == '6eb748cc2cdbb0caf56f07f89268d24b67ee94b0703ad779478786d96af7801f'
metadata_path = REPO / 'artifacts/checkpoints/p8-completion-20261009/execution-599/original-artifact-metadata-checkpoint.json'
assert sha(metadata_path) == '5d835c647069a0b055a4f7dc6bbc4cc4d02c0dedf20cccacb901c2313c376de2'
metadata_doc = json.loads(metadata_path.read_text())
metadata_items = [a for group in metadata_doc['workflow_artifacts'] for a in group['artifacts']]
metadata = {a['id']: a for a in metadata_items}
assert len(metadata) == len(metadata_items)
assert not DST.exists()
summary, checked_ids = [], []
for result in verification['results']:
    bundle = Path(result['path'])
    mp = Path(result['manifest_path'])
    assert bundle.parent == SRC and mp.parent == SRC
    assert bundle.stat().st_size == result['bytes'] and sha(bundle) == result['sha256']
    assert sha(mp) == result['manifest_sha256']
    raw_manifest = mp.read_bytes()
    m = json.loads(raw_manifest)
    assert m['execution_head'] == HEAD and m['not_original_zip'] is True
    entries = {x['archive_member']: x for x in m['members']}
    assert len(entries) == len(m['members'])
    by_id = collections.defaultdict(dict)
    for e in m['members']:
        safe(e['original_member'])
        safe(e['archive_member'])
        assert e['archive_member'] == str(e['artifact_id']) + '/' + e['original_member']
        assert e['original_member'] not in by_id[e['artifact_id']]
        by_id[e['artifact_id']][e['original_member']] = e
    seen = set()
    native_headers = collections.Counter()
    with ExitStack() as stack:
        archives, originals = {}, []
        for a in m['artifacts']:
            zp = Path(a['original_zip_path'])
            md = metadata[a['id']]
            assert md['workflow_run']['id'] == a['workflow_run_id'] and md['workflow_run']['head_sha'] == HEAD
            assert zp.stat().st_size == md['size_in_bytes'] == a['original_zip_bytes']
            assert sha(zp) == a['original_zip_sha256'] and md['digest'] == 'sha256:' + a['original_zip_sha256']
            assert a['id'] not in archives
            z = stack.enter_context(zipfile.ZipFile(zp))
            archives[a['id']] = z
            originals.append(a)
            checked_ids.append(a['id'])
            infos = z.infolist()
            assert len(infos) == len({x.filename for x in infos})
            assert {x.filename for x in infos} == set(by_id[a['id']])
            for info in infos:
                e = by_id[a['id']][info.filename]
                assert info.file_size == e['bytes'] and f'{info.CRC:08x}' == e['original_zip_crc32']
                assert info.external_attr == e['original_zip_external_attr']
                assert ((info.external_attr >> 16) & 0o170000) in [0, 0o100000, 0o040000]
        with tarfile.open(bundle, 'r|gz') as tf:
            for member in tf:
                safe(member.name)
                assert member.name not in seen
                seen.add(member.name)
                if member.name == 'PACKAGING-MANIFEST.json':
                    assert member.isfile() and tf.extractfile(member).read() == raw_manifest
                    continue
                e = entries[member.name]
                assert e['disposition'].startswith('retained_')
                if member.isdir():
                    assert e['disposition'] == 'retained_directory'
                    continue
                assert member.isfile() and e['disposition'] == 'retained_original_bytes' and member.size == e['bytes']
                h, count = hashlib.sha256(), 0
                with archives[e['artifact_id']].open(e['original_member']) as zs, tf.extractfile(member) as ts:
                    while True:
                        zb, tb = zs.read(1048576), ts.read(1048576)
                        assert zb == tb, member.name
                        if not zb:
                            break
                        h.update(zb)
                        count += len(zb)
                assert count == e['bytes'] and h.hexdigest() == e['sha256']
        retained = {n for n, e in entries.items() if e['disposition'].startswith('retained_')}
        assert seen == retained | {'PACKAGING-MANIFEST.json'}
        omitted = [e for e in entries.values() if e['disposition'] == 'omitted_receipt_bound_native_executable']
        for e in omitted:
            z = archives[e['artifact_id']]
            h, count, header = hashlib.sha256(), 0, b''
            with z.open(e['original_member']) as stream:
                for block in iter(lambda: stream.read(1048576), b''):
                    if not header:
                        header = block[:64]
                    h.update(block)
                    count += len(block)
            assert count == e['bytes'] and h.hexdigest() == e['sha256']
            sig = header[:4]
            assert sig.hex() == e['signature_hex']
            native_headers[sig.hex()] += 1
            if sig == b'\x7fELF':
                assert header[4] in [1, 2] and header[5] in [1, 2]
                assert int.from_bytes(header[16:18], 'little' if header[5] == 1 else 'big') in [2, 3]
            else:
                assert sig in [bytes.fromhex(x) for x in ['feedface', 'cefaedfe', 'feedfacf', 'cffaedfe']]
                endian = 'little' if sig in [bytes.fromhex('cefaedfe'), bytes.fromhex('cffaedfe')] else 'big'
                assert int.from_bytes(header[12:16], endian) == 2
            binding = e['build_receipt_binding']
            receipt_raw = z.read(binding['receipt_member'])
            assert hashlib.sha256(receipt_raw).hexdigest() == binding['receipt_member_sha256']
            node = json.loads(receipt_raw)
            pointer = binding['json_pointer']
            assert pointer == '' or pointer.startswith('/')
            for token in pointer.split('/')[1:]:
                token = token.replace('~1', '/').replace('~0', '~')
                node = node[int(token)] if isinstance(node, list) else node[token]
            assert node[binding['binary_hash_field']] == binding['binary_sha256'] == e['sha256']
            if binding['binding_kind'] == 'original_cargo_artifact_and_binary_hash':
                ca = node['cargo_artifact']
                assert ca == binding['cargo_artifact'] and ca['reason'] == 'compiler-artifact'
                assert isinstance(ca['executable'], str)
            else:
                assert binding['binding_kind'] == 'original_successful_build_receipt_and_binary_hash'
                assert node['exit_code'] == 0 and 'build' in node['build_options']['command']
            for key in ['binary_bytes', 'binary_path', 'retained_executable', 'path', 'build_exit_code', 'exit_code',
                        'build_command', 'command', 'cargo_artifact', 'copy_source', 'build_options']:
                if key in binding:
                    assert node[key] == binding[key]
            if 'binary_bytes' in node:
                assert node['binary_bytes'] == e['bytes']
            if 'copy_source' in node and 'bytes' in node['copy_source']:
                assert node['copy_source']['bytes'] == e['bytes']
        for a in originals:
            assert sha(Path(a['original_zip_path'])) == a['original_zip_sha256']
        summary.append({'scope': m['scope'], 'artifacts': len(originals), 'original_member_partition': len(entries),
                        'retained_files': sum(e['disposition'] == 'retained_original_bytes' for e in entries.values()),
                        'omitted_receipt_bound_native_files': len(omitted),
                        'native_executable_headers_verified': dict(native_headers), 'bundle_sha256': result['sha256'],
                        'manifest_sha256': result['manifest_sha256'], 'all_retained_bytes_equal_original_zip_and_tar': True,
                        'embedded_manifest_byte_equal_external': True, 'original_zip_sha256_verified_before_and_after': True})
assert sum(x['retained_files'] for x in summary) == 9776
assert sum(x['omitted_receipt_bound_native_files'] for x in summary) == 50
assert len(checked_ids) == len(set(checked_ids)) == 24
DST.mkdir(parents=True)
copies = []


def copy(source, target):
    target.parent.mkdir(parents=True, exist_ok=True)
    h = sha(source)
    shutil.copyfile(source, target)
    assert sha(target) == h and target.stat().st_size == source.stat().st_size
    copies.append({'original_path': str(source), 'repository_path': str(target.relative_to(REPO)),
                   'bytes': target.stat().st_size, 'sha256': h})


for path in sorted(SRC.iterdir()):
    assert path.is_file()
    copy(path, DST / path.name)
copy(metadata_path, DST / metadata_path.name)
copy(packager, DST / packager.name)
ip = W / 'platform-review/round6-original599-raw-persistence-inventory.json'
assert sha(ip) == verification['inventory_sha256']
copy(ip, DST / ip.name)
copy(Path(__file__), DST / Path(__file__).name)
report = {'kind': 'independent_root_original599_archive_fidelity_review',
          'observed_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'source_head': HEAD,
          'packages': summary, 'copied_records': copies, 'official_metadata_artifact_ids_checked': checked_ids,
          'acceptance_workload_rerun': False, 'task_statuses_changed': False, 'original_todo_remaining': 29,
          'original_zips_retained': True, 'not_complete_original_zips': True,
          'runtime_acceptance_not_reasserted_by_archival_check': True,
          'initial_archival_lookup_issue': 'An initial parent check assumed per-artifact metadata.json files. Six older originals use the already committed official metadata collection instead. No ZIP bytes or acceptance inputs changed; the corrected check reads that exact collection.'}
(DST / 'root-preservation-review.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
print(json.dumps({'destination': str(DST), 'packages': summary, 'copied_files': len(copies),
                  'copied_bytes': sum(x['bytes'] for x in copies)}, indent=2))
