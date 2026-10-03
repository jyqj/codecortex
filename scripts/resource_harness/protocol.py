#!/usr/bin/env python3
"""Pure helpers extracted from fixed preparation source; no import-time execution."""
import argparse, concurrent.futures, hashlib, http.server, json, os, pathlib, queue
import shutil, sqlite3, subprocess, sys, threading, time, traceback

def write(path, value):
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2)+'\n')
def digest(path):
    with path.open('rb') as f:
        h=hashlib.sha256()
        for chunk in iter(lambda:f.read(1024*1024),b''): h.update(chunk)
    return h.hexdigest()
def free(path): return shutil.disk_usage(path).free

def snapshot(pid):
    try:
        text=pathlib.Path(f'/proc/{pid}/stat').read_text(); a=text[text.rfind(')')+2:].split()
        io={k:int(v) for k,v in (x.split(':') for x in pathlib.Path(f'/proc/{pid}/io').read_text().splitlines())}
        status=pathlib.Path(f'/proc/{pid}/status').read_text().splitlines()
        vm={x.split(':')[0]:int(x.split()[1])*1024 for x in status if x.startswith(('VmRSS:','VmHWM:'))}
        return {'pid':pid,'rss_bytes':int(a[21])*os.sysconf('SC_PAGE_SIZE'),'vmrss_bytes':vm.get('VmRSS'),'vmhwm_bytes':vm.get('VmHWM'),'threads':int(a[17]),'cpu_user_ticks':int(a[11]),'cpu_system_ticks':int(a[12]),'cpu_ticks_per_second':os.sysconf('SC_CLK_TCK'),'io':io,'children':None,'descendant_listing':'unavailable_on_this_proc_mount','start_time_ticks':int(a[19])}
    except (OSError,ValueError): return None

def tree_snapshot(pid):
    todo=[pid];seen=set();rows=[];errors=[]
    while todo:
        current=todo.pop()
        if current in seen:continue
        seen.add(current);row=snapshot(current)
        if row:rows.append(row)
        else:errors.append({"pid":current,"reason":"stat/status/io unreadable or process exited"})
        try:
            tasks=list(pathlib.Path(f"/proc/{current}/task").iterdir())
            for task in tasks:
                try:todo.extend(int(x) for x in (task/"children").read_text().split())
                except OSError as exc:errors.append({"pid":current,"task":task.name,"reason":str(exc)})
        except OSError as exc:errors.append({"pid":current,"reason":str(exc)})
    return {"root_pid":pid,"observed_pids":sorted(seen),"snapshots":rows,"sampled_sum_rss_bytes":sum(x["rss_bytes"] for x in rows) if rows else None,"coverage_errors":errors,"method":"recursive every-thread children; non-atomic, transient descendants may be missed; shared pages counted per process"}

def mock(root):
    lock=threading.Lock(); sequence=0
    class Handler(http.server.BaseHTTPRequestHandler):
        def log_message(self,*_): pass
        def do_POST(self):
            nonlocal sequence
            assert self.client_address[0]=='127.0.0.1' and self.path=='/v1/embeddings'
            assert self.headers.get('Authorization')=='Bearer synthetic-resource-only'
            b=self.rfile.read(int(self.headers['Content-Length'])); body=json.loads(b)
            assert body['model']=='fake/resource-preparation' and body.get('dimensions') in (None,128)
            assert all('resource_' in x for x in body['input'])
            start=time.monotonic_ns()
            with lock:
                sequence+=1; seq=sequence
                with (root/'http.jsonl').open('a') as f: f.write(json.dumps({'event':'entered','sequence':seq,'input_count':len(body['input']),'body_bytes':len(b),'body_sha256':hashlib.sha256(b).hexdigest(),'time_ns':start})+'\n')
            (root/'http-entered').touch()
            deadline=time.monotonic()+120
            while not (root/'http-release').exists():
                if time.monotonic()>deadline: raise TimeoutError('owned fixture gate')
                time.sleep(.005)
            payload=json.dumps({'model':body['model'],'data':[{'index':i,'embedding':[1.0]+[0.0]*127} for i in range(len(body['input']))],'usage':{'prompt_tokens':len(body['input']),'total_tokens':len(body['input'])}}).encode()
            self.send_response(200); self.send_header('Content-Type','application/json'); self.send_header('Content-Length',str(len(payload))); self.end_headers()
            try: self.wfile.write(payload)
            except (BrokenPipeError,ConnectionResetError): pass
            with lock:
                with (root/'http.jsonl').open('a') as f:f.write(json.dumps({'event':'returned','sequence':seq,'input_count':len(body['input']),'time_ns':time.monotonic_ns(),'synthetic_usage_units':len(body['input'])})+'\n')
    server=http.server.ThreadingHTTPServer(('127.0.0.1',int(os.environ.get('P7_RESOURCE_PORT','0'))),Handler)
    write(root/'http-port.json',{'port':server.server_port,'pid':os.getpid()})
    server.serve_forever()

def wait_file(p,seconds=20):
    end=time.monotonic()+seconds
    while not p.exists():
        if time.monotonic()>end:raise TimeoutError(str(p))
        time.sleep(.01)
def totals(path):
    count=size=allocated=0
    if path.exists():
        for p in path.rglob('*'):
            if p.is_file():
                s=p.stat();count+=1;size+=s.st_size;allocated+=s.st_blocks*512
    return {'files':count,'logical_bytes':size,'allocated_bytes':allocated}
def db_report(root):
    paths=list(root.rglob('index.sqlite3'))
    if len(paths)!=1:return {'unavailable':True,'candidates':list(map(str,paths))}
    p=paths[0];c=sqlite3.connect(f'file:{p}?mode=ro',uri=True,timeout=0)
    try:
        counts={t:c.execute(f'SELECT COUNT(*) FROM {t}').fetchone()[0] for t in ['files','symbols','chunks','document_manifest','semantic_manifest']}
        counts['edge_tables']={t:c.execute('SELECT COUNT(*) FROM '+t).fetchone()[0] for (t,) in c.execute("SELECT name FROM sqlite_master WHERE type='table'").fetchall() if t.endswith('_edges') and t.replace('_','').isalnum()}
        counts['outbox_states']={s:n for s,n in c.execute('SELECT state,COUNT(*) FROM semantic_outbox GROUP BY state')}
        counts['attempt_count']=c.execute('SELECT COALESCE(SUM(attempt_count),0) FROM semantic_outbox').fetchone()[0]
        return {'path':str(p),'counts':counts,'integrity':c.execute('PRAGMA integrity_check').fetchone()[0],'foreign_key_errors':len(c.execute('PRAGMA foreign_key_check').fetchall()),'files':[{ 'path':str(q),'logical_bytes':q.stat().st_size,'allocated_bytes':q.stat().st_blocks*512} for q in p.parent.iterdir() if q.is_file() and q.name.startswith(p.name)]}
    finally:c.close()
def source(i,value=0,name=None):return f'pub fn {name or "resource_"+str(i).zfill(5)}() -> u32 {{ {i+value} }}\n'

if __name__ == "__main__":
    if len(sys.argv) != 3 or sys.argv[1] != "--mock":
        raise SystemExit("only explicit --mock DIR is supported")
    mock(pathlib.Path(sys.argv[2]))
