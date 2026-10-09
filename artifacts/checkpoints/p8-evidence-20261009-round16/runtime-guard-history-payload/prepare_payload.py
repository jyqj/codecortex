#!/usr/bin/env python3
"""Losslessly preserve existing local E guard records; never run their code."""
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import tarfile

OUT = Path(__file__).resolve().parent
BASE = Path('/dev/shm/a217aaae3bde/runtime-review/combined-candidate-plan-v1')
META = Path('/workspace/scratch/a217aaae3bde/combined-scale-cache-candidate')
PIN = Path('/dev/shm/a217aaae3bde/platform-review/combined-254009-review/pins/final-four-pin-independent-review.json')
ARCHIVE = OUT / 'runtime-local-E-guard-history.tar.gz'
assert not ARCHIVE.exists(), 'Existing delivery must not be overwritten.'
def sha(raw): return hashlib.sha256(raw).hexdigest()
def oid(raw): return hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()
def save(name, value):
    target = OUT/name
    assert not target.exists(), str(target)
    target.write_text(json.dumps(value, sort_keys=True, indent=2)+'\n')
    target.chmod(0o644)
    return {'path': name, 'bytes': target.stat().st_size, 'sha256': sha(target.read_bytes()), 'git_blob_sha1': oid(target.read_bytes())}

selection = {'local-guard/'+str(p.relative_to(BASE)):p for p in BASE.rglob('*') if p.is_file()}
assert len(selection) == 43
metadata_names = [
    'official-product-commit.json', 'official-product-root-tree.json',
    'official-product-crates-recursive.json', 'official-product-scripts-recursive.json',
    'official-review-commit.json', 'official-review-root-tree.json',
    'official-execution-commit.json', 'official-execution-root-tree.json',
    'execution-commit-created.json', 'execution-pins-prepared.json',
    'exact-product-tree-and-input-proof.json', 'prepared-source-overlay.json',
    'review-commit-created.json', 'review-payload-manifest.json',
    'execution-overlay/scripts/verify_reviewed_source_v15.py',
    'execution-overlay/scripts/reviewed-source-registry-v15.json',
]
selection.update({'binding-inputs/'+n:META/n for n in metadata_names})
selection['binding-inputs/final-four-pin-independent-review.json'] = PIN
assert len(selection) == 60
assert not any('.git' in Path(n).parts or Path(n).suffix == '.zip' for n in selection)
rows = []
for member, source in sorted(selection.items()):
    s = source.lstat()
    assert stat.S_ISREG(s.st_mode) and not source.is_symlink(), str(source)
    raw = source.read_bytes()
    assert len(raw) == s.st_size
    text = raw.decode('utf-8')
    # Fail rather than redact originals if unexpected private download material is present.
    assert not re.search(r'https?://[^\s"<>]+[?&](?:sig|signature|token|X-Amz-Signature|X-Goog-Signature)=', text, re.I), member
    assert not re.search(r'\b(?:gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|Bearer\s+[A-Za-z0-9_.=-]{16,})', text), member
    rows.append({'member':member, 'source_path':str(source), 'source_mode':oct(stat.S_IMODE(s.st_mode)), 'archive_mode':'0644', 'bytes':len(raw), 'sha256':sha(raw), 'git_blob_sha1':oid(raw)})
assert all(len(r['member'].encode()) <= 255 for r in rows)
attempts = []
for n in ['guard-execution-a23bb72', 'guard-execution-a23bb72-v2']:
    rec = json.loads((BASE/n/'wrapper-preflight-failure.json').read_bytes())
    assert rec['original_guard_invocations'] == 0
    attempts.append({'attempt':n, 'original_guard_invocations':0, 'status':rec['status'], 'error':rec['error']})
third = json.loads((BASE/'guard-execution-a23bb72-v3/outer-finalization-failure-addendum.json').read_bytes())
assert third['original_cli_exit_code'] is None and third['outer_wrapper_exit_code'] == 1 and third['original_guard_invocations'] == 1
assert third['original_stdout']['status'] == 'passed'
attempts.append({'attempt':'guard-execution-a23bb72-v3', 'original_guard_invocations':1, 'original_cli_exit_code':None, 'outer_wrapper_exit_code':1, 'original_stdout_status':'passed', 'note':third['exit_code_note']})
fs = os.statvfs(OUT)
capacity = {'bavail_bytes':fs.f_bavail*fs.f_frsize, 'bfree_bytes':fs.f_bfree*fs.f_frsize, 'effective_uid':os.geteuid(), 'scope':'Archival storage only; workload/native acceptance budgets unchanged.'}
assert capacity['bfree_bytes'] > sum(r['bytes'] for r in rows) + 16*1024*1024
inventory = save('selection-inventory.json', {'schema':'local-E-guard-history-source-inventory-v1', 'member_count':len(rows), 'logical_bytes':sum(r['bytes'] for r in rows), 'members':rows, 'capacity_before_write':capacity, 'no_symlink_or_zip_members':True, 'unexpected_private_url_or_token_patterns':0})

def generate(sink):
    with gzip.GzipFile(filename='', fileobj=sink, mode='wb', compresslevel=9, mtime=0) as gz:
        with tarfile.open(fileobj=gz, mode='w|', format=tarfile.USTAR_FORMAT) as tf:
            for row in rows:
                raw = selection[row['member']].read_bytes()
                assert sha(raw) == row['sha256'] and oid(raw) == row['git_blob_sha1']
                info = tarfile.TarInfo(row['member'])
                info.size = len(raw); info.mode = 0o644; info.uid = info.gid = info.mtime = 0
                info.uname = info.gname = ''; info.type = tarfile.REGTYPE
                tf.addfile(info, io.BytesIO(raw))
with ARCHIVE.open('xb') as out: generate(out)
ARCHIVE.chmod(0o644)
with tarfile.open(ARCHIVE, 'r:gz') as tf:
    members = tf.getmembers()
    assert [m.name for m in members] == [r['member'] for r in rows]
    for m, row in zip(members, rows):
        assert m.isreg() and m.type == tarfile.REGTYPE and m.size == row['bytes']
        assert m.mode == 0o644 and m.uid == m.gid == m.mtime == 0 and m.uname == m.gname == ''
        assert not m.pax_headers and not m.linkname
        raw = tf.extractfile(m).read()
        assert raw == selection[m.name].read_bytes()
        assert sha(raw) == row['sha256'] and oid(raw) == row['git_blob_sha1']

class CompareSink:
    def __init__(self, reader): self.reader=reader; self.bytes=0
    def write(self, data):
        assert self.reader.read(len(data)) == data
        self.bytes += len(data)
        return len(data)
    def flush(self): pass
with ARCHIVE.open('rb') as original:
    sink=CompareSink(original);generate(sink);assert original.read(1) == b''
    assert sink.bytes == ARCHIVE.stat().st_size
rechecks=[]
for row in rows:
    p=selection[row['member']];raw=p.read_bytes()
    assert len(raw)==row['bytes'] and sha(raw)==row['sha256'] and oid(raw)==row['git_blob_sha1']
    assert oct(stat.S_IMODE(p.stat().st_mode)) == row['source_mode']
    rechecks.append({'member':row['member'], 'source_bytes_mode_unchanged':True})
archive_raw=ARCHIVE.read_bytes()
manifest={
    'schema':'local-E-guard-history-delivery-v1',
    'scope':'Byte-preserving local evidence packaging only; no guard/product/native execution, no remote operations.',
    'execution':'a23bb72d3c954f385b99fe81ce9189885c208557',
    'tree':'58147c952505c44da1f41eb4b9c31643f2303b96',
    'product':'254009277688d64677361a0ca33e5dea73f295ef',
    'review':'3a11f30f9f00a89fe3cd481b3b7609066728baad',
    'historical_attempts':attempts,
    'history_rule':'Any later normal GitHub CI success is separate evidence. It cannot change the two zero-invocation preflight failures or turn the third local CLI null exit into a recorded zero; outer exit remains 1.',
    'view_scope':'Required source/validation/historical-file projection; never claimed as a full main checkout or CI result.',
    'evidence_archive':{'path':ARCHIVE.name,'bytes':len(archive_raw),'sha256':sha(archive_raw),'git_blob_sha1':oid(archive_raw),'format':'USTAR inside gzip, sorted names, file mode0644, uid/gid/mtime0, uname/gname/filename empty, compression9','members':rows,'actual_members_reopened_and_verified':True,'actual_second_generation_byte_equal':True},
    'inventory':inventory,
    'source_rechecks':rechecks,
    'excluded_material':['Private .git/object stores', 'Derived historical checkout directories', 'Original measurement ZIPs and binary libraries', 'Signed/private download references'],
    'original_sources_preserved':True,
    'source_or_task_or_ref_or_index_changes':False,
    'formal_task_completion':False,
    'task_counts':{'done':163,'remaining':29},
    'packager':{'path':Path(__file__).name,'sha256':sha(Path(__file__).read_bytes()),'git_blob_sha1':oid(Path(__file__).read_bytes())},
}
receipt=save('payload-manifest.json',manifest)
print(json.dumps({'archive':{k:v for k,v in manifest['evidence_archive'].items() if k!='members'},'manifest':receipt,'member_count':len(rows),'logical_bytes':sum(r['bytes'] for r in rows)},sort_keys=True))
