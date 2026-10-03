"""Offline evidence integrity check; issues no RPCs."""
import hashlib,json,pathlib
b=pathlib.Path(__file__).resolve().parent
m=json.loads((b/'artifact-manifest.json').read_text())
assert all(hashlib.sha256((b/p).read_bytes()).hexdigest()==h for p,h in m['file_sha256'].items())
r=json.loads((b/'evidence/receipt.json').read_text());assert r['search_calls']==0 and len(r['tests'])==9
errors=0
for t in r['tests']:
 d=json.loads((b/'evidence'/(t['label']+'.json')).read_text())
 assert d['search_calls']==0 and d['rpc_methods'][-1]=='tools/call:index'
 if d['expected_error']:
  errors+=1;assert d['rpc_response']['error']['code']==-32602 and d['expected_error'] in d['actual_error']
 else:assert 'error' not in d['rpc_response'] and d['rpc_response']['result'].get('isError') is not True
 assert 'MODEL ellipsis_empty_leaf_reproduced=1 invalid_dependency_reproduced=1 valid_qualified_name_control=1' in d['component_stdout']
assert errors==5
assert 'calls=2 refs=2 duplicate_ids=1' in json.loads((b/'evidence/go_nested_selector.json').read_text())['component_stdout']
assert 'calls=256 refs=256 duplicate_ids=13' in json.loads((b/'evidence/gin_original_source.json').read_text())['component_stdout']
print(json.dumps({'hashes_verified':len(m['file_sha256']),'retained_tests_verified':9,'expected_errors':errors,'success_controls':4,'new_rpc_calls':0}))
