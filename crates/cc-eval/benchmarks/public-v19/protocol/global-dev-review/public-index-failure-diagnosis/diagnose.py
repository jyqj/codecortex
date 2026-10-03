"""Fixed-version index-only diagnostics. Never sends search or reads query/gold bodies."""
import argparse,hashlib,json,pathlib,subprocess,os,tempfile
from mcp_probe import index
HERE=pathlib.Path(__file__).resolve().parent
BASE=HERE.parent/'development-baseline'
SOURCE='78b0ce52cb2ff4c53c53893b9f7dd469269806b8'
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def save(p,v):p.write_text(json.dumps(v,indent=2,sort_keys=True)+'\n')
def main():
 ap=argparse.ArgumentParser();ap.add_argument('--output',type=pathlib.Path,required=True);ap.add_argument('--binary',type=pathlib.Path,default=pathlib.Path('/tmp/v19-development-products/codecortex'));ap.add_argument('--source',type=pathlib.Path,default=pathlib.Path('/tmp/v19-dev-source-78b0'));ap.add_argument('--deps',type=pathlib.Path,default=pathlib.Path('/tmp/p7-017-build/debug/deps'));ap.add_argument('--inputs',type=pathlib.Path,default=pathlib.Path('/tmp/v19-development-inputs'));a=ap.parse_args();a.output.mkdir(parents=True,exist_ok=False)
 plan=json.loads((BASE/'preregistration-receipt.json').read_text());build=json.loads((BASE/'build/build-receipt.json').read_text())
 assert build['source_sha']==SOURCE==plan['execution_source_sha']
 assert sha(a.binary)=='4e2f101137591c5bbc3bdc6d660cd7c2989e0b72fbacc0bb89d1a79b9ba8fe77'
 assert subprocess.check_output(['git','-C',str(a.source),'rev-parse','HEAD'],text=True).strip()==SOURCE
 assert all(sha(a.source/p)==h for p,h in build['source_file_sha256'].items())
 cases=[
 ('python_variadic_tuple', 'case.py','def f() -> tuple[int, ...]:\n    return ()\n','invalid dependency key'),
 ('python_nonvariadic_control','case.py','def f() -> tuple[int, str]:\n    return ()\n',None),
 ('python_scalar_control','case.py','def f() -> int:\n    return 1\n',None),
 ('go_nested_selector','case.go','package p\nfunc f() { a.B().C() }\n','conflicting duplicate call site'),
 ('go_three_nested_selectors','case.go','package p\nfunc f() { a.B().C().D() }\n','conflicting duplicate call site'),
 ('go_separate_selector_control','case.go','package p\nfunc f() { a.B(); a.C() }\n',None),
 ('go_nested_arguments_control','case.go','package p\nfunc f() { B(C()) }\n',None)]
 # Only these two already-admitted source files; no query, gold or historical bodies.
 original=[]
 for repo,name in [('requests','src/requests/exceptions.py'),('gin','context.go')]:
  rel=f'{repo}/source/{name}';p=a.inputs/rel
  assert sha(p)==plan['input_file_sha256'][rel]
  body=p.read_text();error='invalid dependency key' if repo=='requests' else 'conflicting duplicate call site'
  cases.append((repo+'_original_source',name,body,error));original.append({'path':rel,'sha256':sha(p)})
 libs={}
 for l in (BASE/'build/cargo-build.jsonl').read_text().splitlines():
  try:d=json.loads(l)
  except ValueError:continue
  if d.get('reason')=='compiler-artifact' and d['target']['name'] in ('cc_model','cc_parsers'):
   libs[d['target']['name']]=a.deps/pathlib.Path(next(p for p in d['filenames'] if p.endswith('.rlib'))).name
 env={k:os.environ[k] for k in ('PATH','RUSTUP_HOME','CARGO_HOME','LANG') if k in os.environ}
 command=['rustc','--edition=2021',str(HERE/'component_probe.rs'),'-L','dependency='+str(a.deps)]
 for name,p in libs.items():command+=['--extern',name+'='+str(p)]
 driver=a.output/'component-probe';command+=['-o',str(driver)]
 r=subprocess.run(command,env=env,capture_output=True,text=True);(a.output/'compiler.stdout').write_text(r.stdout);(a.output/'compiler.stderr').write_text(r.stderr);assert r.returncode==0,r.stderr
 results=[]
 for label,name,body,error in cases:
  evidence={'label':label,'input_path':name,'input_sha256':hashlib.sha256(body.encode()).hexdigest(),'expected_error':error,'rpc_methods':['initialize','notifications/initialized','tools/call:index'],'search_calls':0}
  response=index({name:body},a.binary,evidence)
  actual=response.get('error',{}).get('message');evidence['actual_error']=actual
  if error:assert response['error']['code']==-32602 and error in actual
  else:assert 'error' not in response and response['result'].get('isError') is not True
  with tempfile.TemporaryDirectory(prefix='v19-component-diagnosis-') as tmp:
   p=pathlib.Path(tmp)/pathlib.Path(name).name;p.write_text(body)
   c=subprocess.run([str(driver),str(p),'python' if name.endswith('.py') else 'go',name],capture_output=True,text=True);assert c.returncode==0,c.stderr
   evidence['component_stdout']=c.stdout;evidence['component_stderr']=c.stderr
  save(a.output/(label+'.json'),evidence);results.append({'label':label,'error':actual,'expected_matched':True})
 assert all(sha(a.source/p)==h for p,h in build['source_file_sha256'].items())
 receipt={'source_sha':SOURCE,'binary_sha256':sha(a.binary),'baseline_build_receipt_sha256':sha(BASE/'build/build-receipt.json'),'baseline_plan_sha256':sha(BASE/'preregistration-receipt.json'),'original_sources':original,'production_source_files_verified':len(build['source_file_sha256']),'compiler_command':command,'compiler_exit_code':r.returncode,'component_binary_sha256':sha(driver),'rlib_sha256':{str(p):sha(p) for p in libs.values()},'tests':results,'index_calls':len(cases),'search_calls':0,'live_provider_calls':0,'production_changes':0,'gold_changes':0}
 driver.unlink();save(a.output/'receipt.json',receipt)
 print(json.dumps({'tests_passed':len(results),'index_calls':len(cases),'search_calls':0,'source_sha':SOURCE}))
if __name__=='__main__':main()
