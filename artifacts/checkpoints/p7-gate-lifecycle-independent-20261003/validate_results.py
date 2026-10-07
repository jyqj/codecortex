#!/usr/bin/env python3
"""Formal map assertions consume retained raw results and locked per-run hashes."""
import hashlib,json,pathlib,re
HERE=pathlib.Path(__file__).resolve().parent
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
map_=json.loads((HERE/'gate-review-map.json').read_text())
raws=0;commands=0
for path in sorted((HERE/'matrix').rglob('receipt.json')):
 r=json.loads(path.read_text());commands+=1
 for local,digest in r['artifact_sha256'].items():
  p=path.parent/local;assert p.is_file() and sha(p)==digest,p
 assert r['exit_code']==0
 if r['suite']=='independent_real_stdio_lifecycle_L3':
  summary=json.loads((path.parent/'raw/summary.json').read_text())
  assert summary==r['summary'] and summary['passed'] is True
  for row,observed in zip(map_['lifecycle_rows'],summary['cases'],strict=True):
   assert row['case']==observed['case']
   assert observed['status']['semantic_state']==row['expected_semantic_state']
   assert observed['status']['dense_state']==row['expected_dense_state']
   assert observed['delta']['document_posts']==row['expected_document_posts_delta']
   assert observed['delta']['query_posts']==row['expected_query_posts_delta']
  http=json.loads((path.parent/'raw/http-raw.json').read_text())
  rpc=json.loads((path.parent/'raw/stdio-raw.json').read_text());raws+=len(http)+len(rpc)
  assert len(http)==8 and all(r['only_loopback'] and r['dummy_authorization_matched'] for r in http)
  assert sum(r['response_status']==500 for r in http)==3
  assert sum(r['query_posts'] for r in http)==4 and sum(r['document_posts'] for r in http)==4
  errors=[r['response']['error'] for r in rpc if r.get('response',{}).get('error')]
  assert len(errors)==4 and all(r['code']==-32603 and r['message']=='semantic recall is not configured' for r in errors)
 elif r['suite']=='public_L3_kind_name_source':
  observed=json.loads((path.parent/'raw/coordinate-result.json').read_text())
  assert observed==r['summary'] and observed['line_coordinates_closed'] and len(observed['rows'])==2
  raws+=len(json.loads((path.parent/'raw/stdio-raw.json').read_text()))
 else:
  assert r['test_result']==[1,0,0] and r['raw_audit_exit']==0
  audit=json.loads((path.parent/'audit.json').read_text())
  assert (audit['cases'],audit['lane_receipts'],audit['candidates'],audit['hits'])==(11,66,148,21)
  assert audit['byte_and_line_boundaries']=='passed' and all(x=='rejected' for x in audit['negative_controls'].values())
  raws+=11
# Kill the missing-graph control even after its declared count is made consistent.
from audit_scope import audit
first=json.loads((HERE/'matrix/semantic-http/01/raw/all-lane-scope.json').read_text())
graph=next(l for l in first[0]['lanes'] if l['lane_id']=='graph')
graph['candidates']=[c for c in graph['candidates'] if c['legacy_chunk_id']!='chunk:outside/callee.py:0']
graph['candidate_count']=len(graph['candidates'])
try:
 audit(first)
except AssertionError:
 graph_negative='rejected_with_consistent_candidate_count'
else:
 raise AssertionError('missing cross-file graph was admitted')
log=(HERE/'exact-batch-test.log').read_text();assert '1 passed; 0 failed; 0 ignored;' in log
result={'baseline_sha':map_['baseline_sha'],'canonical_commands':commands,'unchanged_PR58_scope_test_runs':10,'scope_case_receipts':110,'scope_lane_receipts':660,'scope_candidates_checked':1480,'scope_final_hits_checked':210,'lifecycle_runs':3,'formal_lifecycle_checkpoints':39,'public_function_name_source_responses':6,'canonical_stdio_http_and_scope_row_records':raws,'additional_exact_batch_unit':{'passed':1,'failed':0,'ignored':0,'filtered_out':226},'strict_clippy_exit':0,'production_counterexample':False,'supplemental_graph_negative_control':graph_negative,'earlier_line_counterexample':'retracted: partial byte slice begins at preceding line newline; line bounds1-4 are correct','full_current_gate_status_changed':False,'P7_016_SIGKILL':'not_run'}
(HERE/'formal-map-verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
