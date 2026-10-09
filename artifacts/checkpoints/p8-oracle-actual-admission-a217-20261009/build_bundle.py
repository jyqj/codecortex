import gzip,hashlib,io,json,re,stat,tarfile
from pathlib import Path

OUT=Path(__file__).resolve().parent
BASE=OUT.parent
def identity(raw):
    return {'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest(),'git_blob':hashlib.sha1(b'blob '+str(len(raw)).encode()+b'\0'+raw).hexdigest()}
handoff=json.loads((BASE/'final-handoff.json').read_text())
rows=list(handoff['files'])
rows.append({'source':str(BASE/'final-handoff.json'),**identity((BASE/'final-handoff.json').read_bytes())})
assert len(rows)==19
members=[]
for row in rows:
    source=Path(row['source']);raw=source.read_bytes()
    assert identity(raw)=={k:row[k] for k in ['bytes','sha256','git_blob']},source
    assert not source.is_symlink() and source.is_file()
    text=raw.decode('utf-8')
    assert not re.search(r'file_[0-9a-f]{20,}',text),source
    assert not re.search(r'library://[A-Za-z0-9]',text),source
    assert not re.search(r'https?://[^\s"<>]*(?:[?&](?:sig|signature|X-Amz-Signature|X-Goog-Signature|token)=)',text,re.I),source
    # Captures of repository Git APIs are public metadata. File-transfer
    # responses, raw ZIPs, source projections and object caches are not selected.
    assert not re.search(r'"(?:file_uri|file_id|download_file_reference|access_token|refresh_token)"\s*:\s*(?!null|""|\[\]|\{\})',text),source
    mode=stat.S_IMODE(source.stat().st_mode)
    name='oracle/'+source.name
    assert len(name.encode())<=100 and mode in (0o644,0o444,0o755)
    members.append({'name':name,'source':str(source.resolve()),'mode':mode,**identity(raw),'raw':raw})
members.sort(key=lambda x:x['name']);assert len({x['name']for x in members})==19
def serialize():
    out=io.BytesIO()
    with gzip.GzipFile(fileobj=out,mode='wb',filename='',mtime=0,compresslevel=9) as gz:
        with tarfile.open(fileobj=gz,mode='w',format=tarfile.USTAR_FORMAT) as tar:
            for item in members:
                info=tarfile.TarInfo(item['name']);info.type=tarfile.REGTYPE;info.size=item['bytes'];info.mode=item['mode'];info.mtime=0;info.uid=0;info.gid=0;info.uname='';info.gname=''
                tar.addfile(info,io.BytesIO(item['raw']))
    return out.getvalue()
packed=serialize();assert serialize()==packed
package=OUT/'oracle-source-admission-evidence.tar.gz';package.write_bytes(packed)
with tarfile.open(fileobj=io.BytesIO(packed),mode='r:gz') as tar:
    infos=tar.getmembers();assert [x.name for x in infos]==[x['name']for x in members]
    for info,expected in zip(infos,members):
        assert info.isreg() and not info.pax_headers and info.uid==info.gid==info.mtime==0 and info.uname==info.gname=='' and info.mode==expected['mode']
        actual=tar.extractfile(info).read();assert actual==expected['raw']==Path(expected['source']).read_bytes()
        assert identity(actual)=={k:expected[k]for k in ['bytes','sha256','git_blob']}
manifest={'schema_version':1,'scope':'19 fixed small oracle admission records; exact original bytes; no new execution','source':'eded24951c3e99693410ce959a627ab575f66bec','tree':'b95b5d069cf35f0181c0534ea7855ca48c503cc4','member_count':19,'member_bytes':sum(x['bytes']for x in members),'archive':{'path':str(package.resolve()),**identity(packed)},'members':[{k:v for k,v in x.items()if k!='raw'}for x in members],'excluded':'No guard-view files, historical Git blob objects/cache, private transfer capture, raw artifact ZIP, executable or new CI monitoring snapshot. No object data or private source projection is included.','private_reference_scan':'No actual file IDs, library URI, signed URL or nonempty transfer/credential fields in selected bytes.','validation':'All regular USTAR headers checked; source bytes/mode/SHA256/Git blob matched before and after; full tar reopen; independent second serialization by this same builder byte-identical.','publication':'local candidate only; no commit/ref modification','formal_new_tasks':0,'remaining_tasks':29}
(OUT/'payload-manifest.json').write_text(json.dumps(manifest,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
readback={'archive':identity(packed),'members_verified':19,'source_bytes':manifest['member_bytes'],'all_source_members_byte_identical':True,'deterministic_second_serialization_identical':True,'builder':identity(Path(__file__).read_bytes())}
(OUT/'readback-receipt.json').write_text(json.dumps(readback,sort_keys=True,indent=2)+'\n')
print(json.dumps(readback))
