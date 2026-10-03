import subprocess,os,json,time,hashlib,re
from pathlib import Path
R=Path(__file__).resolve().parent
T='/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin'
env=os.environ|{'PATH':T+':'+os.environ['PATH'],'CARGO_HOME':'/workspace/.cargo','CARGO_TARGET_DIR':str(R/'target'),'TMPDIR':str(R)}
def run(name,args,cwd,timeout=600):
 start=time.monotonic()
 with (R/(name+'.log')).open('w') as f:
  try: p=subprocess.run(args,cwd=cwd,env=env,stdout=f,stderr=subprocess.STDOUT,timeout=timeout); code=p.returncode
  except subprocess.TimeoutExpired:code=124
 row={'name':name,'argv':args,'cwd':str(cwd.relative_to(R)),'exit_code':code,'seconds':time.monotonic()-start,'log':name+'.log'}
 text=(R/(name+'.log')).read_text()
 paths=re.findall(r'Running .*?\((/[^)]+)\)',text)
 row['binaries']=[{'path':str(Path(x).relative_to(R)),'sha256':hashlib.sha256(Path(x).read_bytes()).hexdigest()} for x in paths if Path(x).is_file()]
 row['test_results']=re.findall(r'test result: (.*)',text)
 return row
if __name__=="__main__":
 runs=[]
 for name,args,cwd in [
  ('independent-db',[T+'/cargo','test','-p','cc-db','--lib','independent_v2','--offline','--','--nocapture','--test-threads=1'],R/'harness'),
  ('independent-default',[T+'/cargo','test','-p','cc-server','--lib','independent_v2','--offline','--','--nocapture','--test-threads=1'],R/'harness'),
  ('independent-http',[T+'/cargo','test','-p','cc-server','--features','semantic-http','--lib','independent_v2','--offline','--','--nocapture','--test-threads=1'],R/'harness'),
 ]:
  row=run(name,args,cwd);runs.append(row);(R/'independent-runs.json').write_text(json.dumps(runs,indent=2)+'\n');print(name,row['exit_code'],flush=True)
  if row['exit_code']:break
