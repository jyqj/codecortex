from pathlib import Path
import sys,json,hashlib,collections,datetime
sys.dont_write_bytecode=True
aid=int(sys.argv[1]);root=Path.cwd();base=root/'raw-pr178-9f7b16f'/str(aid)/'extracted/p8-runtime';review=root/'review-pr/pr178-ci-observation/raw-review'
read=lambda p:json.loads(p.read_text())
key=lambda value:hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':')).encode()).hexdigest()
errors=[];checks=collections.Counter()
def check(value,name):
 checks[name]+=1
 if not value and len(errors)<100:errors.append(name)
original=read(base/'report.json');plan=read(base/'plan.json')
prior=read(review/('runtime-'+str(aid)+'-review-v3.json'))
check(prior['review_status']=='raw_and_receipt_review_passed' and prior['errors']==[],'primary_independent_original_gate_review')
check(plan['profile']=='soak' and plan['operations']==3601 and prior['source_sha']=='9f7b16f0758eb79f306cf44605b84550f02de441','fixed_original_source_and_soak_profile')
raw=[json.loads(x) for x in (base/'raw.jsonl').open()]
ops=[x for x in raw if x['kind']=='operation']
reads=sorted([x for x in ops if x['operation']=='read'],key=lambda x:x['call_started_ns'])
resources=[x for x in raw if x['kind']=='resources']
events=[json.loads(x) for x in (base/'product/rpc.jsonl').open()]
requests=[x for x in events if x.get('event')=='request' and isinstance(x.get('payload'),dict) and x['payload'].get('method')=='tools/call']
responses=[x for x in events if x.get('event')=='response' and isinstance(x.get('payload'),dict) and 'id' in x['payload']]
reply={x['payload']['id']:x for x in responses}
check(len({x['payload']['id'] for x in requests})==len(requests),'unique_original_tool_request_ids')
check(len(reply)==len(responses),'unique_original_response_ids')
actual=collections.defaultdict(list)
for req in requests:
 p=req['payload'];params=p['params'];res=reply[p['id']]['payload']
 check('error' not in res and res['result'].get('isError') is not True,'successful_original_rpc_envelope')
 value=res['result']['structuredContent'];value=value.get('result',value)
 group=params['name']+(':'+params['arguments'].get('mode','') if params['name']=='search' else '')
 actual[group].append({'id':p['id'],'arguments':params['arguments'],'value':value,'request_time_ns':req['time_ns'],'response_time_ns':reply[p['id']]['time_ns']})
 check(reply[p['id']]['time_ns']>=req['time_ns'],'original_transport_request_response_time_order')
all_calls=[call for row in reads for call in row['cache_probe']['requests']]
check(len(reads)==2400 and len(all_calls)==9600,'all_original_compound_reads_and_rpc_roles')
by_role={role:[x for x in all_calls if x['role']==role] for role in ['before_status','symbol','hybrid','after_status']}
for role,calls in by_role.items():check(len(calls)==2400 and all(x['status']=='success' for x in calls),'complete_original_'+role+'_population')
endpoint=next(x for x in raw if x['kind']=='endpoint_public')
wanted_symbol=[x['response'] for x in by_role['symbol']]+[endpoint['incremental']]
check(collections.Counter(key(x['value']) for x in actual['search:symbol'])==collections.Counter(map(key,wanted_symbol)),'every_original_symbol_response_and_endpoint_exactly_bound')
check([key(x['value']) for x in sorted(actual['search:hybrid'],key=lambda x:x['request_time_ns'])]==[key(x['response']) for x in by_role['hybrid']],'every_original_hybrid_response_bound_in_admitted_order')
check([key(x['value']) for x in sorted(actual['search:symbol'],key=lambda x:x['request_time_ns'])[:-1]]==[key(x['response']) for x in by_role['symbol']],'every_original_symbol_response_bound_in_admitted_order')
for group,role in [('search:symbol','symbol'),('search:hybrid','hybrid')]:
 expected_args=by_role[role][0]['arguments']
 check(all(x['arguments']==expected_args for x in actual[group]),'actual_original_'+role+'_request_arguments')
status=actual['status'];check(all(x['arguments']=={'aspect':'index'} for x in status),'original_status_request_arguments')
remaining=collections.Counter(key(x['value']) for x in status)
objects={key(x['value']):x['value'] for x in status}
for call in by_role['before_status']+by_role['after_status']:
 k=key(call['response']);check(remaining[k]>0,'original_full_status_probe_response_bound')
 if remaining[k]>0:remaining[k]-=1
background=[]
for k,count in remaining.items():
 if count>0:background.extend([objects[k]]*count)
diagnostics=[x.get('diagnostics',x) for x in background]
endpoint_state=next(x for x in raw if x['kind']=='endpoint_status')['response']
dcounts=collections.Counter(map(key,diagnostics));last=key(endpoint_state)
check(dcounts[last]>0,'original_final_endpoint_diagnostics_bound')
if dcounts[last]>0:dcounts[last]-=1
diag_objects={key(x):x for x in diagnostics}
projection=lambda x:{'server':x.get('process_resources',{}),'query_execution':x.get('query_execution'),'search_cache':x.get('search_cache')}
retained_projection=collections.Counter()
for k,count in dcounts.items():
 if count>0:retained_projection[key(projection(diag_objects[k]))]+=count
raw_projection=collections.Counter(key({'server':x.get('server',{}),'query_execution':x.get('query_execution'),'search_cache':x.get('search_cache')}) for x in resources)
check(retained_projection==raw_projection,'every_native_resource_snapshot_exactly_bound_after_removing_compound_and_endpoint_status')
check(len(status)==len(resources)+4801,'all_status_requests_accounted_for_without_reusing_responses')
out={'schema_version':1,'artifact_id':aid,'source_sha':prior['source_sha'],'reviewed_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'review_status':'original_stdio_binding_passed' if not errors else 'review_failed','errors':errors,'checks':dict(checks),'compound_reads':len(reads),'compound_rpc_roles':len(all_calls),'status_probe_responses':len(by_role['before_status'])+len(by_role['after_status']),'background_resource_snapshots':len(resources),'all_status_rpcs':len(status),'actual_tool_counts':{k:len(v) for k,v in actual.items()},'scope':'Read-only supplemental binding of original soak raw four-role payloads and native resource projections to retained real stdio request/response events. Uses original source protocol and retains the original gate verdict; introduces no new primary experiment or product acceptance threshold. Absolute RPC clocks are not relabeled as relative workload clocks.'}
path=review/('soak-'+str(aid)+'-stdio-binding-review.json');path.write_text(json.dumps(out,ensure_ascii=False,sort_keys=True,indent=2)+'\n')
print(json.dumps(out,ensure_ascii=False));print(path)
