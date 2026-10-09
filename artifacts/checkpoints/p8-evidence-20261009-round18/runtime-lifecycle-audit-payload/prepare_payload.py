"""Preserve already-completed audit evidence. Does not execute product or validators."""
import datetime,gzip,hashlib,io,json,os,re,stat,tarfile
from pathlib import Path
OUT=Path(__file__).parent
BASE=Path('/dev/shm/a217aaae3bde/runtime-review/e-a23-artifact-review')
WORK=Path('/workspace/scratch/a217aaae3bde')
HEAD='a23bb72d3c954f385b99fe81ce9189885c208557'
IDS=[11591493782,11592154271,11592751905,11593165899,11594089439,11591821446,11591607348]
def hashes(data):
    return dict(bytes=len(data),sha256=hashlib.sha256(data).hexdigest(),git_blob_sha1=hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest())
def ref(p):return dict(path=str(p),**hashes(Path(p).read_bytes()))
def dump(path,value):
    with path.open('x') as f:json.dump(value,f,indent=2,sort_keys=True,ensure_ascii=False);f.write('\n')
sources={};excluded=[]
for p in sorted(BASE.rglob('*')):
    relative=p.relative_to(BASE)
    if 'actual-originals' in relative.parts or '__pycache__' in relative.parts:continue
    if not p.is_file():continue
    assert not p.is_symlink()
    sources['audit/'+relative.as_posix()]=p
for prefix,root in [
    ('peer-runtime',Path('/dev/shm/a217aaae3bde/scale-combined-E-review/e-helper-independent-review')),
    ('peer-lifecycle',Path('/dev/shm/a217aaae3bde/platform-review/e-lifecycle-helper-independent')),
]:
    for p in sorted(root.rglob('*')):
        if p.is_file() and '__pycache__' not in p.parts:
            assert not p.is_symlink();sources[prefix+'/'+p.relative_to(root).as_posix()]=p
for artifact_id in IDS:
    root=WORK/'ci-artifacts'/str(artifact_id)
    for name in ['artifact-metadata.json','original-zip-local-verification.json']:
        p=root/name;assert p.is_file();sources[f'original-metadata/{artifact_id}/{name}']=p
    p=BASE/'actual-originals'/str(artifact_id)/'metadata.json'
    assert p.is_file();sources[f'alias-metadata/{artifact_id}/metadata.json']=p
    meta=json.loads((root/'artifact-metadata.json').read_text())
    assert meta['id']==artifact_id and meta['workflow_run']['head_sha']==HEAD
    excluded.append(dict(kind='complete_original_ZIP_preserved_separately',artifact_id=artifact_id,path=str(root/(str(artifact_id)+'.zip')),bytes=meta['size_in_bytes'],sha256=meta['digest'].removeprefix('sha256:'),github_artifact_url=meta['url'],scope='No member is selectively omitted from the separately preserved original ZIP. This audit package is not a native raw archive.'))
original_log=Path('/dev/shm/a217aaae3bde/platform-review/pr167-execution/runtime-soak-job-113631481157-original.log')
assert original_log.is_file();sources['original-log/runtime-soak-job-113631481157.log']=original_log
layout='''# Runtime and lifecycle audit evidence for exact E

This is a deterministic preservation of local independent audits, their original process receipts, replay outputs, preflight inventories, reconstruction proofs, all twelve frozen helper files, and their independent admission records. It contains no new benchmark measurements and does not change task status. The execution is a23bb72d3c954f385b99fe81ce9189885c208557; the original ledger remains 163 done and 29 remaining.

All six runtime profiles and lifecycle were accepted within the stated scope. The soak original covers 3601 successful operations over 3600.053467347 seconds, all 2400 compound reads / 9600 protocol RPCs, and four actual completion-time quarters. Its cache isolation serializes workload operations: observed maximum is 1, not a saturated configured C4. The separate four mixed profiles provide their measured concurrent coverage. The original finite-window RSS rule, fake-provider boundary, and source identities remain unchanged.

The package is not the complete native raw evidence. The seven complete original ZIPs, including all receipt-bound executables and SQLite/WAL/SHM files, are preserved separately by the root agent and identified in payload-manifest.json. Public Git checkpoint paths must be filled only once their real blobs/parts and assembly manifest are published. A locator alone does not satisfy preservation. The final benchmarks navigation should include the small original reports, the task/V/source/schema mapping, exact published original-ZIP parts and whole hashes, and the actual restoration recipe. Concatenate ordered original ZIP slices, verify every slice and whole ZIP SHA256/size, extract to a fresh output directory with no member path changes, then use the original exact-source verify CLI with explicit --output/--build arguments. The observed P8 scripts accept explicit paths and do not require raw members at a hardcoded benchmarks path.

The roadmap tasks require artifacts/benchmarks/<run-id>/ evidence, while 09-BENCHMARK.md section11 describes the general benchmark artifact schema. Do not assert that generic names such as normalized.jsonl were produced when the actual P8 runner uses a different explicit schema. Deliver the actual original report/plan/statistics/parity/resource records by a truthful schema map and lossless original archive recovery, not invented placeholder files. This preservation layout does not waive any original acceptance gate.

Historical failure records are retained as failures. The empty verify_backfill_11591821446_derived_cleanup.py is a zero-byte ENOSPC interruption, not an executed proof. The soak wrapper preparation first failed before any helper invocation because a replacement-count expectation was 7 rather than 6. Its later cleanup-preparation attempt first failed only after member comparison because the path-alias receipt name differed; the first inventory and failure explanation remain, with a separate second inventory and actual successful root reclamation. Neither preparation error reran a native benchmark or rewrote accepted evidence.

The three earlier local E source-guard wrapper attempts are separately preserved in runtime-local-E-guard-history.tar.gz (SHA256 829785e3df8ba0c9864c61339c5cc221a36ee399db5b55454251ff934b479100). Its two preflight failures had zero CLI invocation; the third original stdout reported pass but its integer CLI exit was not persisted (null), and the outer wrapper exited1. Normal GitHub CI success is separate and never changes that local history.

The lifecycle relocated-replay-input.json was a proven reproducible derivative and was reclaimed only after exact reconstruction verification. Its original ZIP input, unchanged helper, replacement count and serialized-byte hash are retained in the cleanup candidate. The actual relocated-report.json, stdout/stderr and exit receipt remain here. No source checkout, private Git object store, private download reference, signed URL, or native executable replay copy is included.
'''
with (OUT/'README.md').open('x') as f:f.write(layout)
sources['README.md']=OUT/'README.md'
sources['preservation/prepare_payload.py']=Path(__file__)
manifest_files=json.loads((BASE/'frozen-helper-manifest.json').read_text())['files']
assert len(manifest_files)==12
for item in manifest_files.values():
    p=Path(item['path']);assert p.stat().st_size==item['bytes'] and ref(p)['sha256']==item['sha256']
rows=[]
for name,p in sorted(sources.items()):
    data=p.read_bytes();s=p.stat()
    assert stat.S_ISREG(s.st_mode) and not p.is_symlink()
    assert not any(v in name.lower() for v in ['private-download','download-reference','.git/'])
    # Do not print potential content if an unsafe download credential is detected.
    assert not re.search(rb'https?://[^\s\"<>]+[?&](?:sig|signature|x-amz-signature|x-goog-signature)=',data,re.I),name
    rows.append(dict(member=name,source_path=str(p),source_mode=stat.S_IMODE(s.st_mode),archive_mode=0o644,**hashes(data)))
selection=dict(schema=1,head=HEAD,member_count=len(rows),uncompressed_bytes=sum(r['bytes'] for r in rows),files=rows,excluded_native_originals=excluded)
dump(OUT/'selection-inventory.json',selection)
archive=OUT/'runtime-lifecycle-audit-evidence.tar.gz';assert not archive.exists()
def emit(target):
    with gzip.GzipFile(fileobj=target,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
        with tarfile.open(fileobj=gz,mode='w|',format=tarfile.USTAR_FORMAT) as tar:
            for row in rows:
                p=Path(row['source_path']);data=p.read_bytes();assert hashes(data)=={k:row[k] for k in ['bytes','sha256','git_blob_sha1']}
                info=tarfile.TarInfo(row['member']);info.size=len(data);info.mode=0o644;info.uid=info.gid=info.mtime=0;info.uname=info.gname='';info.type=tarfile.REGTYPE
                tar.addfile(info,io.BytesIO(data))
with archive.open('xb') as f:emit(f)
with tarfile.open(archive,'r:gz') as tar:
    members=tar.getmembers();assert [m.name for m in members]==[r['member'] for r in rows]
    for m,row in zip(members,rows):
        assert m.isfile() and m.mode==0o644 and m.uid==m.gid==m.mtime==0 and m.uname==m.gname=='' and not m.pax_headers
        data=tar.extractfile(m).read();assert data==Path(row['source_path']).read_bytes()
        assert hashes(data)=={k:row[k] for k in ['bytes','sha256','git_blob_sha1']}
second=io.BytesIO();emit(second);assert second.getvalue()==archive.read_bytes()
for row in rows:
    p=Path(row['source_path']);assert ref(p)['sha256']==row['sha256'] and stat.S_IMODE(p.stat().st_mode)==row['source_mode']
manifest=dict(schema=1,head=HEAD,tree='58147c952505c44da1f41eb4b9c31643f2303b96',product='254009277688d64677361a0ca33e5dea73f295ef',canonical_1087_manifest_sha256='4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00',archive=ref(archive),selection=ref(OUT/'selection-inventory.json'),member_count=len(rows),uncompressed_bytes=sum(r['bytes'] for r in rows),format='sorted regular USTAR mode0644 uid/gid/mtime0 uname/gname empty; gzip filename empty,mtime0,level9',members=rows,excluded_native_originals=excluded,frozen_helper_count=12,full_reopen_all_member_source_byte_equality=True,independent_second_serialization_byte_identical=True,all_sources_bytes_and_modes_unchanged=True,native_or_acceptance_rerun=False,formal_task_completion=False,counts=dict(done=163,remaining=29),historical_incomplete_file={'member':'audit/verify_backfill_11591821446_derived_cleanup.py','bytes':0,'status':'interrupted_ENOSPC_not_executed_proof'},scope='Audit preservation only. Complete original native ZIPs preserved separately; no original sample or report outcome changed.')
dump(OUT/'payload-manifest.json',manifest)
print(json.dumps(dict(archive=manifest['archive'],manifest=ref(OUT/'payload-manifest.json'),member_count=len(rows),uncompressed_bytes=manifest['uncompressed_bytes'])))
