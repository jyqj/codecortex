#!/usr/bin/env python3
"""Discover exact missing tree objects along the verifier's fixed blob paths."""
import hashlib,json,pathlib,subprocess
P=pathlib.Path('/workspace/scratch/a217aaae3bde/prospective-combined-tree-prep')
G='/dev/shm/a217aaae3bde/prospective-combined-tree-prep/git'
def git(*args):return subprocess.check_output(['git','--git-dir='+G,*args])
presence={}
def have(sha):
    if sha not in presence:
        presence[sha]=subprocess.run(['git','--git-dir='+G,'cat-file','-e',sha],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0
    return presence[sha]
def object_id(kind,raw):return hashlib.sha1((kind+' '+str(len(raw))+'\0').encode()+raw).hexdigest()
api=P/'historical-tree-metadata';api.mkdir(exist_ok=True)
for path in api.glob('*.json'):
    d=json.loads(path.read_bytes());assert d['sha']==path.stem and d['truncated'] is False
    entries=sorted(d['tree'],key=lambda e:(e['path']+('/' if e['type']=='tree' else '')).encode())
    assert all('/' not in e['path'] for e in entries)
    raw=b''.join((str(int(e['mode']))+' '+e['path']).encode()+b'\0'+bytes.fromhex(e['sha']) for e in entries)
    assert object_id('tree',raw)==d['sha']
    wrote=subprocess.check_output(['git','--git-dir='+G,'hash-object','-t','tree','-w','--stdin'],input=raw).decode().strip()
    assert wrote==d['sha']
closure=json.loads((P/'historical-v2-complete-read-closure.json').read_bytes())
frontier={}; missing_blobs={}; resolved=[]; tree_cache={}; commit_cache={}
for pair in closure['unique_git_blob_reads']:
    if pair['ref'] not in commit_cache:commit_cache[pair['ref']]=git('cat-file','commit',pair['ref']).splitlines()[0].decode().split()[1]
    current=commit_cache[pair['ref']]
    parts=pair['path'].split('/')
    for index,part in enumerate(parts):
        if not have(current):
            frontier.setdefault(current,[]).append(pair);break
        if current not in tree_cache:
            rows={}
            for row in git('ls-tree','-z',current).split(b'\0'):
                if row:
                    hdr,name=row.split(b'\t');mode,kind,oid=hdr.decode().split();rows[name.decode()]=(mode,kind,oid)
            tree_cache[current]=rows
        assert part in tree_cache[current],(pair,part)
        mode,kind,oid=tree_cache[current][part]
        if index<len(parts)-1:
            assert kind=='tree';current=oid
        else:
            assert kind=='blob'
            bound=dict(pair,blob=oid,mode=mode)
            if not have(oid):missing_blobs.setdefault(oid,[]).append(bound)
            else:resolved.append(bound)
result={'frontier':[{'sha':s,'needed_by':v} for s,v in sorted(frontier.items())],
        'missing_blobs':missing_blobs,'resolved':resolved,'unique_reads':len(closure['unique_git_blob_reads'])}
(P/'historical-provenance-frontier-state.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'frontier':[e['sha'] for e in result['frontier']],
                  'missing_blob_count':len(missing_blobs),'resolved_count':len(resolved),'total_count':result['unique_reads']}))
