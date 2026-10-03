"""Index-only repair verification at an exact compiled source; no query/gold reads."""
import hashlib,json,pathlib,subprocess,argparse
from mcp_probe import index
HERE=pathlib.Path(__file__).resolve().parent
SOURCE='ee1988521e2125f86d2ff0aff8559dccc2a417b0'
def sha(p):return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
def main():
 p=argparse.ArgumentParser();p.add_argument('--output',type=pathlib.Path,required=True);p.add_argument('--build',type=pathlib.Path,default=pathlib.Path('/tmp/v19-requests-fix-build-receipt/build-receipt.json'));p.add_argument('--inputs',type=pathlib.Path,default=pathlib.Path('/tmp/v19-development-inputs'));a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
 receipt=json.loads(a.build.read_text());assert receipt['source_sha']==SOURCE and receipt['build_exit_code']==0
 binary=pathlib.Path(receipt['artifacts']['codecortex']['copied_binary']);assert sha(binary)==receipt['artifacts']['codecortex']['binary_sha256']
 source=pathlib.Path(receipt['source_worktree']);assert all(sha(source/f)==h for f,h in receipt['source_file_sha256'].items())
 # Frozen input receipt only: hashes/metadata, never query or gold bodies.
 plan=json.loads(subprocess.check_output(['git','show','5d3d9e101198cebdfa1a959f74ad6ef5da4a064c:crates/cc-eval/benchmarks/public-v19/protocol/global-dev-review/development-baseline/preregistration-receipt.json']))
 hashes={f:h for f,h in plan['input_file_sha256'].items() if f.startswith('requests/source/')}
 assert len(hashes)==20
 files={}
 for f,h in hashes.items():
  input_path=a.inputs/f;assert sha(input_path)==h;files[f.removeprefix('requests/source/')]=input_path.read_text()
 cases=[('variadic_tuple_min',{'case.py':'def f() -> tuple[int, ...]:\n    return ()\n'}),('variadic_named_type',{'case.py':'class Widget: pass\ndef f() -> tuple[Widget, ...]:\n    return ()\n'}),('nonvariadic_control',{'case.py':'def f() -> tuple[int, str]:\n    return ()\n'}),('original_exceptions_only',{'src/requests/exceptions.py':files['src/requests/exceptions.py']}),('full_requests_source',files)]
 results=[]
 for label,case in cases:
  e={'label':label,'input_sha256':{f:hashlib.sha256(body.encode()).hexdigest() for f,body in case.items()},'search_calls':0}
  r=index(case,binary,e);assert 'error' not in r,r.get('error');assert r['result'].get('isError') is not True
  report=r['result']['structuredContent']['result'];assert not report['parse_errors'],report['parse_errors'];assert report['files_parsed']==len(case)
  assert len(e['database_audits'])==1,e['database_audits'];audit=e['database_audits'][0];assert audit['empty_keys']==audit['punctuation_type_records']==audit['duplicate_sites']==0;assert audit['manifest_count']==len(case)
  (a.output/(label+'.json')).write_text(json.dumps(e,indent=2,sort_keys=True)+'\n');results.append({'label':label,'input_files':len(case),'files_parsed':report['files_parsed'],'db_audit':audit,'status':'pass'})
 assert all(sha(source/f)==h for f,h in receipt['source_file_sha256'].items())
 summary={'source_sha':SOURCE,'base_sha':'ace2bc7983be2955831c9384e44d1bdd0749c909','binary_sha256':sha(binary),'build_receipt_sha256':sha(a.build),'source_files_verified':len(receipt['source_file_sha256']),'requests_source_sha256':hashes,'tests':results,'index_calls':len(cases),'search_calls':0,'provider_calls':0}
 (a.output/'index-receipt.json').write_text(json.dumps(summary,indent=2,sort_keys=True)+'\n');print(json.dumps({'tests_passed':len(cases),'full_requests_source_files':len(files),'search_calls':0,'source_sha':SOURCE,'binary_sha256':sha(binary)}))
if __name__=='__main__':main()
