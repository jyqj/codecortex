"""Offline default-product index upgrades; no query/gold/provider IO."""
import argparse, hashlib, json, os, pathlib, queue, sqlite3, subprocess, threading

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def audit(root):
    path = root / '.codecortex/index.sqlite3'
    if not path.exists():
        return None
    with sqlite3.connect('file:' + str(path) + '?mode=ro', uri=True) as db:
        manifests = [json.loads(r[0]) for r in db.execute('SELECT payload FROM resolution_manifests')]
        return dict(schema=db.execute('PRAGMA user_version').fetchone()[0],
                    generation=dict(db.execute("SELECT key,value FROM metadata WHERE key IN ('index_incarnation','index_epoch','evidence_epoch')")),
                    files=db.execute('SELECT count(*) FROM files').fetchone()[0],
                    calls=db.execute('SELECT count(*) FROM call_edges').fetchone()[0],
                    manifests=len(manifests), versions=sorted({m['version'] for m in manifests}),
                    empty_keys=db.execute("SELECT count(*) FROM resolution_dependencies WHERE key='' ").fetchone()[0],
                    punctuation_types=sum(1 for m in manifests for r in m['records'] if r['site_kind']=='semantic' and r['query'].strip() in {'...', '.', '::', '?', '|', '&', '*', '[', ']', '(', ')', ','}),
                    duplicate_sites=sum(len(m['records'])-len({(r['site_kind'],r['site_id']) for r in m['records']}) for m in manifests))

def run(binary, root, full, logfile):
    env = dict(os.environ, CODECORTEX_PPID_POLL_MS='0', XDG_CONFIG_HOME=str(root/'.config'), XDG_CACHE_HOME=str(root/'.cache'))
    with logfile.open('w') as err:
        process = subprocess.Popen([str(binary), 'mcp', '--project-path', str(root)], cwd=root, env=env,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=err, text=True)
        responses = queue.Queue()
        def reader():
            for line in process.stdout:
                responses.put(json.loads(line))
        threading.Thread(target=reader, daemon=True).start()
        def send(value):
            process.stdin.write(json.dumps(value)+'\n'); process.stdin.flush()
        def call(i, method, params):
            send(dict(jsonrpc='2.0', id=i, method=method, params=params))
            while True:
                response = responses.get(timeout=120)
                if response.get('id') == i:
                    return response
        try:
            call(1, 'initialize', dict(protocolVersion='2024-11-05', capabilities={}, clientInfo=dict(name='index-fix-upgrade',version='1')))
            send(dict(jsonrpc='2.0', method='notifications/initialized'))
            opened = audit(root)
            response = call(2, 'tools/call', dict(name='index', arguments=dict(path=str(root),full=full)))
            return dict(opened=opened, response=response, after=audit(root))
        finally:
            process.terminate()
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired: process.kill(); process.wait()

def success(item, schema=24, manifest=3):
    r=item['response']
    assert 'error' not in r and not r.get('result',{}).get('isError'), r
    for key in ('empty_keys','punctuation_types','duplicate_sites'):
        assert item['after'][key] == 0, item
    assert item['after']['schema'] == schema and item['after']['versions'] == [manifest], item

def main():
    p=argparse.ArgumentParser()
    for name in ('old','new','output','requests','gin'): p.add_argument('--'+name,type=pathlib.Path,required=True)
    p.add_argument('--old-source-sha',default='ace2bc7983be2955831c9384e44d1bdd0749c909')
    a=p.parse_args(); a.output.mkdir(parents=True,exist_ok=True)
    receipt=dict(old_sha=a.old_source_sha,new_sha=subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
                 binary_sha256=dict(old=digest(a.old),new=digest(a.new)),provider_calls=0, search_calls=0,cases=[])
    fixtures={
        'requests_public': {str(f.relative_to(a.requests)):f.read_bytes() for f in (a.requests/'src/requests').glob('*.py')},
        'gin_public': {'context.go':(a.gin/'context.go').read_bytes()},
        'old_cache_go': {'main.go':b'package main\nfunc f() { a.B(); a.C() }\n'},
        'old_cache_type': {'main.py':b'def f() -> tuple[int, str]:\n    return ()\n'},
    }
    fixtures['requests_public']['pyproject.toml']=(a.requests/'pyproject.toml').read_bytes()
    receipt['public_sources']={name:dict(source_sha=subprocess.check_output(['git','-C',str(root),'rev-parse','HEAD'],text=True).strip(),file_sha256={n:hashlib.sha256(b).hexdigest() for n,b in fixtures[name].items()}) for name,root in [('requests_public',a.requests),('gin_public',a.gin)]}
    for label,files in fixtures.items():
        root=a.output/label; root.mkdir(exist_ok=True)
        for name,body in files.items():
            target=root/name;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(body)
        (root/'.codecortex.json').write_text('{"auto_index":{"enabled":false}}')
        old=run(a.old,root,True,a.output/(label+'-old.stderr'))
        old_error=old['response'].get('error',{}).get('message')
        if label.endswith('public'):
            expected='invalid dependency key' if label.startswith('requests') else 'conflicting duplicate call site'
            assert expected in (old_error or ''), old
        else:
            assert old_error is None and old['after']['schema']==22 and old['after']['versions']==[1],old
        upgraded=run(a.new,root,False,a.output/(label+'-new.stderr')); success(upgraded)
        assert upgraded['opened']['schema']==24 and upgraded['opened']['files']==0,upgraded
        assert old['after']['generation']['index_incarnation'] != upgraded['after']['generation']['index_incarnation']
        noop=run(a.new,root,False,a.output/(label+'-noop.stderr'));success(noop)
        assert noop['after']==upgraded['after'],(noop,upgraded)
        receipt['cases'].append(dict(label=label,old=old,upgraded=upgraded,noop=noop))
    # Previously failing syntax must work on a genuinely populated old database
    # after adding it under the new build, without modifying database keys.
    root=a.output/'old_cache_go'
    (root/'main.go').write_text('package main\nfunc f() { a.B().C().D() }\n')
    changed=run(a.new,root,False,a.output/'nested-new.stderr');success(changed)
    root=a.output/'old_cache_type'
    (root/'main.py').write_text('def f() -> tuple[int, ...]:\n    return ()\n')
    changed_type=run(a.new,root,False,a.output/'variadic-new.stderr');success(changed_type)
    receipt['changed_after_upgrade']=dict(go=changed,python=changed_type)
    (a.output/'receipt.json').write_text(json.dumps(receipt,indent=2,sort_keys=True)+'\n')
    print(json.dumps(dict(cases=len(receipt['cases']),status='pass',new_sha=receipt['new_sha'])))
if __name__=='__main__':main()
