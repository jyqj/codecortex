"""Finite explicit-list adaptation of the admitted deterministic USTAR builder."""
import gzip,hashlib,io,json,re,stat,tarfile
from pathlib import Path
OUT=Path(__file__).resolve().parent
def identity(raw):
    return {'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()}
rows=json.loads((OUT/'selected-files.json').read_text());assert len(rows)==67
members=[]
for row in rows:
    source=Path(row['source']);assert source.is_file() and not source.is_symlink()
    raw=source.read_bytes();text=raw.decode('utf-8');name=row['name']
    assert not re.search(r'file_[0-9a-f]{20,}',text),source
    assert not re.search(r'library://[A-Za-z0-9]',text),source
    assert not re.search(r'https?://[^\s"<>]*(?:[?&](?:sig|signature|X-Amz-Signature|X-Goog-Signature|token)=)',text,re.I),source
    assert not re.search(r'"(?:file_uri|file_id|download_file_reference|access_token|refresh_token)"\s*:\s*(?!null|""|\[\]|\{\})',text),source
    assert len(name.encode())<=100 and not Path(name).is_absolute() and '..' not in Path(name).parts
    mode=stat.S_IMODE(source.stat().st_mode);assert mode in (0o644,0o444,0o755)
    members.append({'name':name,'source':str(source),'mode':mode,**identity(raw),'raw':raw})
members.sort(key=lambda x:x['name']);assert len({x['name'] for x in members})==67
def serialize():
    out=io.BytesIO()
    with gzip.GzipFile(fileobj=out,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
        with tarfile.open(fileobj=gz,mode='w',format=tarfile.USTAR_FORMAT) as tar:
            for item in members:
                info=tarfile.TarInfo(item['name']);info.type=tarfile.REGTYPE;info.size=item['bytes'];info.mode=item['mode'];info.mtime=0;info.uid=0;info.gid=0;info.uname='';info.gname=''
                tar.addfile(info,io.BytesIO(item['raw']))
    return out.getvalue()
packed=serialize();assert serialize()==packed
archive=OUT/'source-admission-followups.tar.gz';archive.open('xb').write(packed)
with tarfile.open(fileobj=io.BytesIO(packed),mode='r:gz') as tar:
    infos=tar.getmembers();assert [x.name for x in infos]==[x['name'] for x in members]
    for info,expected in zip(infos,members):
        assert info.isreg() and not info.pax_headers and info.uid==info.gid==info.mtime==0 and info.uname==info.gname=='' and info.mode==expected['mode']
        actual=tar.extractfile(info).read();assert actual==expected['raw']==Path(expected['source']).read_bytes()
        assert stat.S_IMODE(Path(expected['source']).stat().st_mode)==expected['mode']
        assert identity(actual)=={k:expected[k] for k in ('bytes','sha256','git_blob')}
bindings=[{'scope':'cold formatting followup','execution':'10ec828beb94200ec679302c45115760ebd55ee9','review':'9c8ecddf295db708ffaa6bacef7f6ede1bf4b658','core_path':'artifacts/checkpoints/p8-cold-format-repair-a217-20261009/independent-source-review.json','core_sha256':'7da2dd2852ed788940e5077b170ebc1bbd4ba8f7b9c7b70e206e7d88af69ec1f','core_git_blob':'2c4e9f5eeb160edf1978e20c5b9276b27fea0f19'}, {'scope':'oracle formatting followup','execution':'9ddae045f6925140a4d652c5f28f64add459eff1','review':'dab9ad69f766f4f910d7e1cb8664e46fb7280240','core_path':'artifacts/checkpoints/p8-oracle-format-repair-a217-20261009/independent-source-review.json','core_sha256':'ea7c1c0b59352a9fa489c1904d1f4463a3328a26d091cd0167c4fe9306220510','core_git_blob':'974de022b8e546d45d9ef13492492b7f4ae09656'}, {'scope':'chunk statement retention source admission','execution':'87c2274e497c3d0c9d40de4b538794318672e028','review':'7d555379c7c5fd7fd89e9432fb9acb056dda597f','core_path':'artifacts/checkpoints/p8-rebuild-chunk-source-a217-20261009/independent-source-review.json','core_sha256':'99f65ceefeb94f7f753f4ebb192cfd98f141e7d65405c83e62d32fde2b9eb78e','core_git_blob':'fe248f208591e067408561c284ee158355e05c0c'}]
manifest={'schema':'three-completed-source-admissions-public-bundle-v1','parent':'c7096df66308a5b805d941e66aa0c9195da0ede7','member_count':67,'member_bytes':sum(x['bytes'] for x in members),'archive':{'path':str(archive),**identity(packed)},'members':[{k:v for k,v in x.items() if k!='raw'} for x in members],'fixed_core_locators_not_duplicated':bindings,'scope':'Original source v15/taskplan executions, source proofs and peer records for three distinct fixed execution heads; one separate original Oracle100-record lifetime/order analysis. No new execution, source equivalence or TODO completion claimed.','exclusions':['guard-view directories and actual source payload','private Git/object/history caches','full API envelopes','native/raw artifact ZIP or ELF','profile6e1 running or later results','private transfer capture/file IDs/signed download URLs'],'source_integrity':'Every selected member is original bytes; old records remain unchanged. No report timestamps or source labels rewritten.','privacy_scan':'No actual private file ID, signed URL, library URI or nonempty credential/transfer fields detected.','verification':'Sorted regular USTAR; actual source modes; zero times/owners; no PAX; full reopen and all member/source hashes equal; same-builder second serialization byte-identical. This is packaging verification, not an additional task acceptance gate.','publication':'candidate only; no ref change by packager','formal_new_tasks':0,'done':163,'remaining':29}
(OUT/'payload-manifest.json').open('x').write(json.dumps(manifest,sort_keys=True,indent=2)+'\n')
receipt={'archive':identity(packed),'members_verified':67,'source_bytes':manifest['member_bytes'],'all_source_members_byte_identical':True,'all_source_modes_unchanged':True,'deterministic_second_serialization_identical':True,'builder':identity(Path(__file__).read_bytes())}
(OUT/'readback-receipt.json').open('x').write(json.dumps(receipt,sort_keys=True,indent=2)+'\n');print(json.dumps(receipt))
