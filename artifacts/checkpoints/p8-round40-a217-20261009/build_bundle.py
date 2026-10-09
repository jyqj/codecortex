"""Finite explicit-list adaptation of the admitted deterministic USTAR builder."""
import gzip,hashlib,io,json,re,stat,tarfile
from pathlib import Path
OUT=Path(__file__).resolve().parent
def identity(raw):
    return {'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()}
rows=json.loads((OUT/'selected-files.json').read_text());assert len(rows)==54
members=[]
for row in rows:
    source=Path(row['source']);assert source.is_file() and not source.is_symlink()
    raw=source.read_bytes();text=raw.decode('utf-8');name=row['name']
    assert identity(raw)=={k:row[k] for k in ('bytes','sha256','git_blob')}, source
    assert not re.search(r'file_[0-9a-f]{20,}',text),source
    assert not re.search(r'library://[A-Za-z0-9]',text),source
    assert not re.search(r'https?://[^\s"<>]*(?:[?&](?:sig|signature|X-Amz-Signature|X-Goog-Signature|token)=)',text,re.I),source
    assert not re.search(r'"(?:file_uri|file_id|download_file_reference|access_token|refresh_token)"\s*:\s*(?!null|""|\[\]|\{\})',text),source
    assert len(name.encode())<=100 and not Path(name).is_absolute() and '..' not in Path(name).parts
    mode=stat.S_IMODE(source.stat().st_mode);assert mode in (0o644,0o444,0o755)
    assert mode==row['mode'], source
    members.append({'name':name,'source':str(source),'mode':mode,**identity(raw),'raw':raw})
members.sort(key=lambda x:x['name']);assert len({x['name'] for x in members})==len(rows)
def serialize():
    out=io.BytesIO()
    with gzip.GzipFile(fileobj=out,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
        with tarfile.open(fileobj=gz,mode='w',format=tarfile.USTAR_FORMAT) as tar:
            for item in members:
                info=tarfile.TarInfo(item['name']);info.type=tarfile.REGTYPE;info.size=item['bytes'];info.mode=item['mode'];info.mtime=0;info.uid=0;info.gid=0;info.uname='';info.gname=''
                tar.addfile(info,io.BytesIO(item['raw']))
    return out.getvalue()
packed=serialize();assert serialize()==packed
archive=OUT/'round40-public-evidence.tar.gz';archive.open('xb').write(packed)
with tarfile.open(fileobj=io.BytesIO(packed),mode='r:gz') as tar:
    infos=tar.getmembers();assert [x.name for x in infos]==[x['name'] for x in members]
    for info,expected in zip(infos,members):
        assert info.isreg() and not info.pax_headers and info.uid==info.gid==info.mtime==0 and info.uname==info.gname=='' and info.mode==expected['mode']
        actual=tar.extractfile(info).read();assert actual==expected['raw']==Path(expected['source']).read_bytes()
        assert stat.S_IMODE(Path(expected['source']).stat().st_mode)==expected['mode']
        assert identity(actual)=={k:expected[k] for k in ('bytes','sha256','git_blob')}

old_sources=set()
for old_path in ['p8-source-admission-followups-public/selected-files.json','p8-round39-public-evidence/selected-files.json']:
    old=json.loads((OUT.parent/old_path).read_text());old_sources.update(x['source'] for x in old)
assert not {x['source'] for x in rows}.intersection(old_sources)
manifest={'schema':'round40-finite-public-evidence-bundle-v1','parent':'579f3dbbba3650cceb0a2c4af6acf1618e57ff99','member_count':len(members),'member_bytes':sum(x['bytes'] for x in members),'archive':{'path':str(archive),**identity(packed)},'members':[{k:v for k,v in x.items() if k!='raw'} for x in members],'scope':'PR193 scoped source reviews/overlap; ChunkG7da actual source-admission including outer index-refresh failure; Cold10ec ordinary CI and G6Bsoak original logs; complete equal-lifetime candidate and Gf675 actual source admission; three root round/priority/first-intake records. Every record retains its original time and source.','exclusions':['prior95e67 and57940 source files','guard-view/private Git or raw-index binaries','full API/tool envelopes','native/raw artifact ZIP or ELF','private transfer capture/file IDs/signed download URLs','late #192 format repair and later study results not selected'],'prior107_source_intersection':0,'source_integrity':'All54 original bytes and modes retained; no original source/state/result/timestamp rewritten.','privacy_scan':'No actual private file ID, signed URL, Library URI or nonempty credential/transfer fields detected.','verification':'Sorted regular USTAR; actual modes; zero times/owners; no PAX; complete reopened member/source hashes equal; second serialization byte-identical. Packaging verification only.','publication':'candidate only; no ref change by packager','formal_new_tasks':0,'done':163,'remaining':29}
(OUT/'payload-manifest.json').open('x').write(json.dumps(manifest,sort_keys=True,indent=2)+'\n')
receipt={'archive':identity(packed),'members_verified':len(members),'source_bytes':manifest['member_bytes'],'all_source_members_byte_identical':True,'all_source_modes_unchanged':True,'deterministic_second_serialization_identical':True,'prior107_source_intersection':0,'builder':identity(Path(__file__).read_bytes())}
(OUT/'readback-receipt.json').open('x').write(json.dumps(receipt,sort_keys=True,indent=2)+'\n');print(json.dumps(receipt))
