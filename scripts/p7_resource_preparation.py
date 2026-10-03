#!/usr/bin/env python3
"""Bounded real-stdio resource preparation. Synthetic/loopback only; no acceptance inference."""
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
        return {'pid':pid,'rss_bytes':int(a[21])*os.sysconf('SC_PAGE_SIZE'),'vmrss_bytes':vm.get('VmRSS'),'vmhwm_bytes':vm.get('VmHWM'),'threads':int(a[17]),'cpu_user_ticks':int(a[11]),'cpu_system_ticks':int(a[12]),'cpu_ticks_per_second':os.sysconf('SC_CLK_TCK'),'io':io,'children':pathlib.Path(f'/proc/{pid}/task/{pid}/children').read_text().split()}
    except (OSError,ValueError): return None

def tree_snapshot(pid):
    seen=set();todo=[pid];rows=[]
    while todo:
        current=todo.pop()
        if current in seen:continue
        seen.add(current);row=snapshot(current)
        if row:rows.append(row)
        try:
            for p in pathlib.Path(f'/proc/{current}/task').glob('*/children'):
                todo.extend(int(x) for x in p.read_text().split())
        except OSError:pass
    return {'root_pid':pid,'observed_pids':sorted(seen),'rss_bytes':sum(x['rss_bytes'] for x in rows),'threads':sum(x['threads'] for x in rows),'snapshots':rows,'method':'20ms PID-tree samples from owned parent relationships; short-lived child may escape sampling'}

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
    server=http.server.ThreadingHTTPServer(('127.0.0.1',0),Handler)
    write(root/'http-port.json',{'port':server.server_port,'pid':os.getpid()})
    server.serve_forever()

class Product:
    def __init__(self,binary,root,out,cache):
        env={k:v for k,v in os.environ.items() if not k.startswith('CODECORTEX_') and k not in ('OPENAI_API_KEY',)}
        env.update(CODECORTEX_SEMANTIC_CACHE_ROOT=str(cache),CODECORTEX_PPID_POLL_MS='0',P7_RESOURCE_DUMMY='synthetic-resource-only')
        self.stderr=(out/'product-stderr.log').open('wb')
        self.p=subprocess.Popen([str(binary),'mcp','--project-path',str(root)],cwd=root,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.stderr,text=True)
        self.lock=threading.Lock();self.responses={};self.n=0;self.raw=(out/'rpc.jsonl').open('w');self.active=0;self.peak=0;self.phase='startup';self.stop=threading.Event();self.resources=(out/'resources.jsonl').open('w');self.model_pid=None
        def reader():
            for line in self.p.stdout:
                value=json.loads(line)
                with self.lock:
                    self.raw.write(json.dumps({'event':'response','phase':self.phase,'time_ns':time.monotonic_ns(),'wire_bytes':len(line.encode()),'wire_sha256':hashlib.sha256(line.encode()).hexdigest(),'payload':value})+'\n');self.raw.flush()
                    q=self.responses.get(value.get('id'))
                if q:q.put(value)
        threading.Thread(target=reader,daemon=True).start()
        def sampler():
            while not self.stop.is_set():
                self.resources.write(json.dumps({'time_ns':time.monotonic_ns(),'phase':self.phase,'product':snapshot(self.p.pid),'product_tree':tree_snapshot(self.p.pid),'runner':snapshot(os.getpid()),'model':snapshot(self.model_pid) if self.model_pid else None,'tmp_free_bytes':free(out),'cgroup_memory_current':int(pathlib.Path('/sys/fs/cgroup/memory.current').read_text())})+'\n');self.resources.flush()
                self.stop.wait(.02)
        self.sampler=threading.Thread(target=sampler,daemon=True);self.sampler.start()
        self.rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'resource-preparation','version':'1'}})
        with self.lock:self.p.stdin.write(json.dumps({'jsonrpc':'2.0','method':'notifications/initialized'})+'\n');self.p.stdin.flush()
        tools=self.rpc('tools/list',{})['tools'];assert len(tools)==14;write(out/'tools.json',tools)
    def rpc(self,method,params,offered=None,timeout=300):
        offered=offered or time.monotonic_ns();q=queue.Queue()
        with self.lock:
            self.n+=1;i=self.n;self.responses[i]=q;self.active+=1;self.peak=max(self.peak,self.active)
            request={'jsonrpc':'2.0','id':i,'method':method,'params':params};sent=time.monotonic_ns();self.raw.write(json.dumps({'event':'request','phase':self.phase,'offered_ns':offered,'sent_ns':sent,'payload':request})+'\n');self.raw.flush();self.p.stdin.write(json.dumps(request)+'\n');self.p.stdin.flush()
        try:
            value=q.get(timeout=timeout);finish=time.monotonic_ns()
            if 'error' in value:raise RuntimeError(value['error'])
            self.last_timing={'offered_ns':offered,'sent_ns':sent,'finish_ns':finish,'client_queue_ms':(sent-offered)/1e6,'wire_to_response_ms':(finish-sent)/1e6,'offered_to_response_ms':(finish-offered)/1e6,'backend_queue_ms':None,'backend_service_ms':None}
            return value['result']
        finally:
            with self.lock:self.active-=1;self.responses.pop(i,None)
    def tool(self,name,args,**kw):
        value=self.rpc('tools/call',{'name':name,'arguments':args},**kw)
        if value.get('isError'):raise RuntimeError(value)
        data=value['structuredContent'];return data.get('result',data)
    def close(self):
        if self.p.poll() is None:
            self.p.stdin.close()
            try:self.p.wait(timeout=15)
            except subprocess.TimeoutExpired:self.p.terminate();self.p.wait(timeout=15)
        self.stop.set();self.sampler.join();self.raw.close();self.resources.close();self.stderr.close()
        return self.p.returncode

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
def run(args):
    out=args.output.resolve();out.mkdir() # refuse overwriting evidence
    binary=args.binary.resolve();summary={'status':'running_prepared','source_sha':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'binary':str(binary),'binary_sha256':digest(binary),'profile':'debug','cells':[],'scales':[],'not_run':[],'full_P7_015':False,'full_V20':False}
    environment={'cpu_max':pathlib.Path('/sys/fs/cgroup/cpu.max').read_text().strip(),'memory_max':pathlib.Path('/sys/fs/cgroup/memory.max').read_text().strip(),'memory_stat':pathlib.Path('/sys/fs/cgroup/memory.stat').read_text(),'memory_events':pathlib.Path('/sys/fs/cgroup/memory.events').read_text(),'affinity':sorted(os.sched_getaffinity(0)),'tmp_available_bytes':free(out),'workspace_available_bytes':free(pathlib.Path('/workspace')),'device':out.stat().st_dev,'platform':dict(zip(("sysname","nodename","release","version","machine"),os.uname())),'page_size':os.sysconf('SC_PAGE_SIZE'),'clock_ticks':os.sysconf('SC_CLK_TCK'),'sample_interval_ms':20,'reclaimable_file_cache_not_free_RAM':True};write(out/'environment.json',environment)
    for n in args.scales:
        if free(out)<1024**3:
            summary['not_run'].append({'scale':n,'reason':'tmp_preflight_less_than_1GiB_reserve','actual_free':free(out)});continue
        case=out/f'n{n}';case.mkdir();root=case/'repo';root.mkdir();(root/'src').mkdir();cache=case/'semantic-cache';mock_process=None;product=None
        try:
            for i in range(n):(root/'src'/f'file_{i:05}.rs').write_text(source(i))
            input_manifest=[{'path':str(p.relative_to(root)),'bytes':p.stat().st_size,'sha256':digest(p)} for p in sorted((root/'src').glob('*.rs'))];write(case/'source-inputs.json',input_manifest)
            mocklog=(case/'model-stderr.log').open('wb');mock_process=subprocess.Popen([sys.executable,str(pathlib.Path(__file__).resolve()),'--mock',str(case)],stdout=mocklog,stderr=mocklog)
            wait_file(case/'http-port.json');port=json.loads((case/'http-port.json').read_text())['port']
            config={'auto_index':{'enabled':False},'indexing':{'max_concurrent_parse':4},'query':{'strategy':'local','deadline_ms':30000,'lane_timeout_ms':20000,'semantic_timeout_ms':5000,'semantic_top_k':24},'semantic':{'enabled':True,'network_opt_in':True,'allow_query_network':False,'model_id':'fake/resource-preparation','dimensions':128,'max_input_tokens':8192,'max_batch_items':16,'endpoint':f'http://127.0.0.1:{port}/v1','allow_http':True,'api_key_ref':'env:P7_RESOURCE_DUMMY','max_concurrent':4,'max_concurrent_per_project':2,'retry_max_attempts':1,'retry_total_deadline_ms':120000,'breaker_failure_threshold':100}}
            write(root/'.codecortex.json',config);write(case/'config.json',config)
            product=Product(binary,root,case,cache);product.model_pid=mock_process.pid;builds=[]
            product.phase='cold-empty-index-build';t=time.monotonic_ns();data=product.tool('index',{'path':str(root)});builds.append({'profile':product.phase,'duration_ms':(time.monotonic_ns()-t)/1e6,'result':data})
            wait_file(case/'http-entered');held=product.tool('status',{'aspect':'capabilities'})['retrieval'];assert held['semantic_state']=='backfilling'
            write(case/'held-status.json',held);write(case/'held-db.json',db_report(root))
            product.phase='warmup-unmeasured';product.tool('search',{'query':f'resource_{n-1:05}','retrieval_strategy':'local','top_k':5})
            for ci,c in enumerate(args.concurrency):
                for mode in ['warm_distinct_query','warm_repeated_query']:
                    product.phase=f'{mode}-C{c}';product.peak=0;offered_start=time.monotonic_ns();errors=[]
                    # Fixed open-loop burst schedule; queue admission is visible, not a closed-loop throughput generator.
                    with concurrent.futures.ThreadPoolExecutor(max_workers=c) as pool:
                        def job(j):
                            offered=offered_start+(j//c)*100_000_000
                            query=f'resource_{((ci*args.samples+j)%n) if mode=="warm_distinct_query" else 0:05}'
                            value=product.tool('search',{'query':query,'retrieval_strategy':'local','top_k':5},offered=offered)
                            assert value['machine_pack']['hits'],query
                            assert any(query in h['text'] for h in value['machine_pack']['hits']),query
                            return {'query':query,'hits':len(value['machine_pack']['hits']),'packing':value['evidence_summary'].get('packing')}
                        futures=[]
                        for j in range(args.samples):
                            offered=offered_start+(j//c)*100_000_000
                            delay=(offered-time.monotonic_ns())/1e9
                            if delay>0:time.sleep(delay)
                            futures.append(pool.submit(job,j))
                        for f in futures:
                            try:f.result()
                            except Exception as e:errors.append(str(e))
                    summary['cells'].append({'scale':n,'concurrency_cap':c,'profile':mode,'offered_samples':args.samples,'peak_client_inflight':product.peak,'duration_ms':(time.monotonic_ns()-offered_start)/1e6,'errors':errors,'tail_certified':False})
                    write(out/'summary.json',summary)
            for label,count in [('no-op',0),('single-body',1),('batch-10',min(n,10)),('batch-100',min(n,100)),('batch-1000',min(n,1000))]:
                product.phase=label;paths=[]
                for i in range(count):(root/'src'/f'file_{i:05}.rs').write_text(source(i,10+count));paths.append(f'src/file_{i:05}.rs')
                kw={'path':str(root)}
                if count:kw['changed_paths']=paths
                t=time.monotonic_ns();data=product.tool('index',kw);builds.append({'profile':label,'files_mutated':count,'duration_ms':(time.monotonic_ns()-t)/1e6,'result':data})
            write(case/'builds.json',builds)
            # Release only the owned loopback model; then measure actual backfill completion.
            product.phase='backfill-drain';(case/'http-release').touch();start=time.monotonic();status=None
            while time.monotonic()-start<args.drain_timeout:
                status=product.tool('status',{'aspect':'capabilities'})['retrieval']
                if status['semantic_state']=='ready':break
                if free(out)<512*1024**2:raise RuntimeError('tmp runtime reserve reached; preserve partial artifacts')
                time.sleep(.1)
            write(case/'final-status.json',status);write(case/'final-db.json',db_report(root));assert status['semantic_state']=='ready',status
            product.phase='ready';value=product.tool('search',{'query':'resource_00000','retrieval_strategy':'local','top_k':5});assert value['machine_pack']['hits']
            exit_code=product.close();assert exit_code==0;product=None
            summary['scales'].append({'scale':n,'status':'measured_debug_prepared','fixture_source_bytes':sum(a['bytes'] for a in input_manifest),'source_files':n,'repo':totals(root),'cache':totals(cache),'backfill_drain_ms':(time.monotonic()-start)*1000,'product_exit_code':exit_code,'device':root.stat().st_dev,'db':json.loads((case/'final-db.json').read_text())})
        except Exception as e:
            summary['scales'].append({'scale':n,'status':'failed_or_incomplete','error':str(e),'traceback':traceback.format_exc(),'tmp_available':free(out)})
            write(out/'summary.json',summary)
            raise
        finally:
            (case/'http-release').touch()
            if product:product.close()
            if mock_process and mock_process.poll() is None:mock_process.terminate();mock_process.wait(timeout=10)
            if mock_process:mocklog.close()
            write(out/'summary.json',summary)
    summary['status']='completed_prepared';write(out/'summary.json',summary)

if __name__=='__main__':
    if len(sys.argv)==3 and sys.argv[1]=='--mock':mock(pathlib.Path(sys.argv[2]));sys.exit()
    p=argparse.ArgumentParser();p.add_argument('--binary',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--scales',type=int,nargs='+',default=[1000]);p.add_argument('--concurrency',type=int,nargs='+',default=[1,4,8,16]);p.add_argument('--samples',type=int,default=32);p.add_argument('--drain-timeout',type=int,default=180);run(p.parse_args())
