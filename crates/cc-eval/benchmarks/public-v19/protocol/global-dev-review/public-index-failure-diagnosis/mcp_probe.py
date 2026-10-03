import subprocess,json,os,tempfile,pathlib,select,time

def index(files, binary, evidence):
 with tempfile.TemporaryDirectory(prefix='v19-index-diagnosis-') as t:
  root=pathlib.Path(t)
  for name,body in files.items():
   p=root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(body)
  env={k:os.environ[k] for k in ('PATH','LANG','LC_ALL','TMPDIR','TZ') if k in os.environ}
  env.update(HOME=t,XDG_CONFIG_HOME=t+'/.config',XDG_CACHE_HOME=t+'/.cache',CODECORTEX_PPID_POLL_MS='0')
  with open(root/'stderr.log','w') as err:
   p=subprocess.Popen([str(binary),'mcp','--project-path',t],cwd=t,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=err,text=True)
   def call(i,method,params):
    p.stdin.write(json.dumps(dict(jsonrpc='2.0',id=i,method=method,params=params))+'\n');p.stdin.flush()
    deadline=time.monotonic()+30
    while time.monotonic()<deadline:
     if not select.select([p.stdout],[],[],max(0,deadline-time.monotonic()))[0]:break
     l=p.stdout.readline()
     if not l:raise RuntimeError('closed')
     r=json.loads(l)
     if r.get('id')==i:return r
    raise TimeoutError(method)
   try:
    call(1,'initialize',dict(protocolVersion='2024-11-05',capabilities={},clientInfo=dict(name='public-index-failure-diagnosis',version='1')))
    p.stdin.write(json.dumps(dict(jsonrpc='2.0',method='notifications/initialized'))+'\n');p.stdin.flush()
    response=call(2,'tools/call',dict(name='index',arguments=dict(path=t,full=True)))
    evidence['rpc_response']=response
    return response
   finally:
    p.kill();p.wait(timeout=5)
    evidence['stderr']= (root/'stderr.log').read_text()
