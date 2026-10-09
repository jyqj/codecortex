#!/usr/bin/env python3
"""Reproduce only the seven already SHA-verified fixed metadata commit objects.

The recorded timezone/newline choices are used only when the complete resulting
Git object SHA1 and raw SHA256 match the original immutable identities.
"""
import datetime,hashlib,json,pathlib,subprocess
P=pathlib.Path(__file__).resolve().parent
G=pathlib.Path('/dev/shm/a217aaae3bde/prospective-combined-tree-prep/git')
for row in json.loads((P/'historical-commit-reconstruction-receipt.json').read_bytes()):
    metadata=pathlib.Path(row['metadata_path']).read_bytes()
    assert hashlib.sha256(metadata).hexdigest()==row['metadata_sha256']
    d=json.loads(metadata);assert d['sha']==row['sha']
    assert not d['verification']['signature']
    head='tree '+d['tree']['sha']+'\n'+''.join('parent '+p['sha']+'\n' for p in d['parents'])
    for kind in ['author','committer']:
        timestamp=int(datetime.datetime.fromisoformat(d[kind]['date'].replace('Z','+00:00')).timestamp())
        head+=f"{kind} {d[kind]['name']} <{d[kind]['email']}> {timestamp} {row[kind+'_offset']}\n"
    message=d['message']
    if row['final_newline'] and not message.endswith('\n'):message+='\n'
    raw=(head+'\n'+message).encode()
    assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['raw_sha256']
    identity=hashlib.sha1(b'commit '+str(len(raw)).encode()+b'\0'+raw).hexdigest()
    assert identity==row['sha']
    result=subprocess.check_output(['git','--git-dir='+str(G),'hash-object','-t','commit','-w','--stdin'],input=raw).decode().strip()
    assert result==identity
    print(identity)
