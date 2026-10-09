"""Inspect first; extract only after a successful exact-source/space preflight.

This is an auditor storage bound, never a workload-budget or acceptance change.
Original archives/metadata are never overwritten or removed.
"""
import argparse
import hashlib
import json
from pathlib import Path,PurePosixPath
import shutil
import stat
import zipfile
from e_raw_cache_audit import HEAD,RUN,file_hash


def inventory(archive, metadata, destination):
    archive,destination=Path(archive),Path(destination)
    meta=json.loads(Path(metadata).read_text())
    assert meta['workflow_run']['head_sha']==HEAD and meta['workflow_run']['id']==RUN
    assert archive.stat().st_size==meta['size_in_bytes'] and file_hash(archive)==meta['digest'].removeprefix('sha256:')
    assert meta['digest'].startswith('sha256:') and not destination.exists()
    members=[];seen=set();normalized=set()
    with zipfile.ZipFile(archive) as z:
        for m in z.infolist():
            path=PurePosixPath(m.filename)
            assert not path.is_absolute() and path.parts and '..' not in path.parts and '\\' not in m.filename
            assert m.filename not in seen;seen.add(m.filename)
            assert str(path) not in normalized;normalized.add(str(path))
            mode=(m.external_attr>>16)&0xffff
            assert not stat.S_ISLNK(mode) and stat.S_IFMT(mode) in (0,stat.S_IFREG,stat.S_IFDIR)
            members.append(dict(path=m.filename,bytes=m.file_size,compressed_bytes=m.compress_size,directory=m.is_dir(),zip_mode=mode))
    total=sum(m['bytes'] for m in members)
    replay_copy=sum(m['bytes'] for m in members if m['path'].startswith(('p8-runtime/project/','p8-runtime/fresh-full/')) or m['path'] in ('p8-build/p8-oracle','p8-build/p8-runtime-statistics'))
    required=total+replay_copy+48*1024*1024
    parent=destination.parent
    while not parent.exists():parent=parent.parent
    free=shutil.disk_usage(parent).free
    return dict(artifact_id=meta['id'],head=HEAD,run=RUN,archive=str(archive),zip_bytes=archive.stat().st_size,zip_sha256=file_hash(archive),destination=str(destination),members=members,file_count=sum(not m['directory'] for m in members),uncompressed_bytes=total,estimated_native_replay_copy_bytes=replay_copy,review_reserve_bytes=48*1024*1024,required_free_bytes=required,actual_free_bytes=free,space_sufficient=free>=required,scope='Auditor storage preflight only. Every original member retained; no workload, raw budget or acceptance threshold changed.')


def extract_originals(record):
    assert record['space_sufficient'], 'insufficient auditor storage; keep ZIP and wait for space'
    dest=Path(record['destination']);assert not dest.exists();dest.mkdir(parents=True)
    copied=[]
    with zipfile.ZipFile(record['archive']) as z:
        for m in record['members']:
            p=dest/m['path']
            if m['directory']:p.mkdir(parents=True,exist_ok=True);continue
            p.parent.mkdir(parents=True,exist_ok=True);h=hashlib.sha256();count=0
            with z.open(m['path']) as src,p.open('xb') as dst:
                for b in iter(lambda:src.read(1024*1024),b''):dst.write(b);h.update(b);count+=len(b)
            assert count==m['bytes'] and file_hash(p)==h.hexdigest()
            p.chmod(0o444);copied.append(dict(path=m['path'],bytes=count,sha256=h.hexdigest()))
    assert len(copied)==record['file_count']
    assert file_hash(record['archive'])==record['zip_sha256']
    return dict(extracted_files=copied,zip_unchanged=True,original_bytes_verified=True,scope='Read-only original bytes; executable copies for replay must be separate.')


if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('zip');p.add_argument('metadata');p.add_argument('destination');p.add_argument('receipt');p.add_argument('--extract',action='store_true');a=p.parse_args()
    r=inventory(a.zip,a.metadata,a.destination)
    if a.extract:r['extraction']=extract_originals(r)
    with Path(a.receipt).open('x') as out:json.dump(r,out,indent=2,sort_keys=True);out.write('\n')
    print(json.dumps({k:v for k,v in r.items() if k not in ('members','extraction')},sort_keys=True))
