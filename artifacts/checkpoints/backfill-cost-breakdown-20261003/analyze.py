#!/usr/bin/env python3
"""Read immutable evidence git blobs; write only beside this script. No product run."""
import bisect, collections, gzip, hashlib, json, pathlib, statistics, subprocess

OUT = pathlib.Path(__file__).resolve().parent
ROOT = OUT.parents[2]
SOURCE = '574f7598662334c63e020da136c87f4f7281554d'
EVIDENCE = 'a8452be903e1a88c935e267f5d76879ce7ee2716'
BASE = 'artifacts/checkpoints/independent-100k-release-20261003/'
manifest = {}

def blob(path, rev=EVIDENCE):
    data = subprocess.check_output(['git', 'show', rev + ':' + path], cwd=ROOT)
    manifest[rev + ':' + path] = {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}
    return data

def rows(name):
    return [json.loads(x) for x in gzip.decompress(blob(BASE + 'live/n100000/' + name + '.jsonl.gz')).splitlines()]

def stats(values):
    a = sorted(values)
    if not a:
        return {'n': 0}
    def q(p):
        x = (len(a)-1)*p
        lo = int(x)
        return a[lo] + (a[min(lo+1, len(a)-1)]-a[lo])*(x-lo)
    return {'n':len(a), 'sum':sum(a), 'mean':statistics.mean(a), 'p50':q(.5), 'p90':q(.9), 'p95':q(.95), 'p99':q(.99), 'max':max(a)}

http, rpc, resource = rows('http'), rows('rpc'), rows('resources')
entered = {x['sequence']:x for x in http if x['event']=='entered'}
returned = {x['sequence']:x for x in http if x['event']=='returned'}
assert set(entered)==set(returned)==set(range(1,29697))
assert all(x['input_count']==1 for x in http)
req = {x['payload']['id']:x for x in rpc if x['event']=='request'}
resp = {x['payload']['id']:x for x in rpc if x['event']=='response'}
polls = []
for i,x in req.items():
    if x['payload'].get('params',{}).get('name')=='status':
        y=resp[i]
        data=y['payload']['result']['structuredContent']
        data=data.get('result',data)
        polls.append({'id':i,'start_ns':x['sent_ns'],'end_ns':y['time_ns'],'wire_ms':(y['time_ns']-x['sent_ns'])/1e6, 'retrieval':data['retrieval']})
polls.sort(key=lambda x:x['start_ns'])
start, end = polls[0]['start_ns'], polls[-1]['end_ns']
def count_until(t):
    return bisect.bisect_right(rt,t)
rt=sorted(x['time_ns'] for x in returned.values())
et=sorted(x['time_ns'] for x in entered.values())
def per_window(lo,hi):
    num=count_until(hi)-count_until(lo)
    ss=[x for x in resource if lo<=x['time_ns']<=hi and x['product']]
    r={'start_ns':lo,'end_ns':hi,'wall_s':(hi-lo)/1e9,'returned':num,'returned_per_s':num/((hi-lo)/1e9),'pending_proxy_start':100000-count_until(lo),'pending_proxy_end':100000-count_until(hi)}
    if len(ss)>1:
        a,b=ss[0]['product'],ss[-1]['product']
        r['sample_wall_s']=(ss[-1]['time_ns']-ss[0]['time_ns'])/1e9
        r['cpu_s']=(b['cpu_user_ticks']+b['cpu_system_ticks']-a['cpu_user_ticks']-a['cpu_system_ticks'])/a['cpu_ticks_per_second']
        r['cpu_cores']=r['cpu_s']/r['sample_wall_s']
        r['io_delta']={k:b['io'][k]-a['io'][k] for k in a['io']}
    return r

duration=[(returned[i]['time_ns']-entered[i]['time_ns'])/1e6 for i in entered]
gaps=[(entered[i+1]['time_ns']-returned[i]['time_ns'])/1e6 for i in range(1,29696)]
cycles=[(entered[i+1]['time_ns']-entered[i]['time_ns'])/1e6 for i in range(2,29696)]
events=sorted((x['time_ns'],1 if x['event']=='entered' else -1) for x in http)
active=peak=0
for _,v in events:
    active+=v;peak=max(peak,active)
assert active==0
poll_ns=sum(x['end_ns']-x['start_ns'] for x in polls)
poll_returned=sum(count_until(x['end_ns'])-count_until(x['start_ns']) for x in polls)
postgate_span=(returned[29696]['time_ns']-returned[1]['time_ns'])/1e9
boundaries={}
for label,predicate in [('within_16',lambda i:i%16!=0),('round_16',lambda i:i%16==0),('job_1024',lambda i:i%1024==0),('before_cursor_exhaustion',lambda i:i<25000),('after_cursor_exhaustion',lambda i:i>25024)]:
    boundaries[label]=stats([gaps[i-1] for i in range(1,29696) if predicate(i)])
report={'source_sha':SOURCE,'evidence_sha':EVIDENCE,'scope':'final isolated failed 100k run only; no product execution; no heldout/private/provider reads',
        'binary_receipt':json.loads(blob(BASE+'build-receipt.json')),
        'source_identity':json.loads(blob(BASE+'source-identity.json')),
        'http':{'requests':len(entered),'input_count':1,'observed_peak_inflight':peak,'first_gated_request_ms':duration[0],'ungated_server_span_ms':stats(duration[1:]),'returned_to_next_entered_ms':stats(gaps),'entered_to_next_entered_ms':stats(cycles),'postgate_span_s':postgate_span,'postgate_returned_per_s':29695/postgate_span,'ungated_server_fraction':sum(duration[1:])/1000/postgate_span,'boundaries_ms':boundaries},
        'status':{'requests':len(polls),'window_s':(end-start)/1e9,'wire_ms':stats([x['wire_ms'] for x in polls]),'union_fraction':poll_ns/(end-start),'returned_during_rpc':poll_returned,'during_rpc_returned_per_s':poll_returned/(poll_ns/1e9),'outside_rpc_returned_per_s':(count_until(end)-count_until(start)-poll_returned)/((end-start-poll_ns)/1e9),'state_counts':dict(collections.Counter(x['retrieval']['semantic_state'] for x in polls)), 'error_counts':dict(collections.Counter(x['retrieval'].get('error',{}).get('message') for x in polls)), 'poll_sleep_ms':stats([(polls[i+1]['start_ns']-polls[i]['end_ns'])/1e6 for i in range(len(polls)-1)])},
        'windows_30s':[per_window(lo,min(lo+30_000_000_000,end)) for lo in range(start,end,30_000_000_000)],
        'measurement_window':per_window(start,end),
        'cleanup_tail':per_window(end,returned[29696]['time_ns']),
        'limitations':['HTTP entered follows request body read; returned follows response write and log lock acquisition: not provider client RTT.', 'returned-to-entered includes provider response decode/admission/cache put + verify/CAS/claim/renew/input and occasional reconciliation/job turnover; no per-phase hooks.', 'No fsync duration/syscall trace, SQL profile or no-poll counterfactual; wall fractions are not causal CPU cost shares.', 'pending proxy = 100000 - HTTP returns; publication slightly lags HTTP and claims are separate. Final done=29696 is cleanup-after-failure count.', 'clock is shared monotonic_ns; resources sampled ~20ms, non-atomic root PID only; read_bytes is attributed storage reads, rchar includes cached reads.', 'Single failed run: descriptive quantiles, no replicate confidence interval; adjacent requests autocorrelated.']}
report['source_files']={}
def exposure(lo, hi):
    return sum(max(0,min(hi,p['end_ns'])-max(lo,p['start_ns'])) for p in polls)
for w in report['windows_30s']:
    lo,hi=w['start_ns'],w['end_ns']
    ns=exposure(lo,hi)
    inside=sum(count_until(min(hi,p['end_ns']))-count_until(max(lo,p['start_ns'])) for p in polls if max(lo,p['start_ns'])<min(hi,p['end_ns']))
    w['poll_fraction']=ns/(hi-lo)
    w['inside_poll_per_s']=inside/(ns/1e9) if ns else None
    w['outside_poll_per_s']=(w['returned']-inside)/((hi-lo-ns)/1e9) if ns<hi-lo else None
# Keep only sample deltas wholly inside/outside a poll; discard mixed intervals.
sample_buckets={'poll':collections.Counter(),'gap':collections.Counter(),'mixed':collections.Counter()}
for a,b in zip(resource,resource[1:]):
    lo,hi=a['time_ns'],b['time_ns']
    if lo<start or hi>end or not a['product'] or not b['product']:
        continue
    ns=exposure(lo,hi)
    bucket=sample_buckets['poll' if ns==hi-lo else 'gap' if ns==0 else 'mixed']
    bucket['wall_s']+=(hi-lo)/1e9
    bucket['cpu_s']+=(b['product']['cpu_user_ticks']+b['product']['cpu_system_ticks']-a['product']['cpu_user_ticks']-a['product']['cpu_system_ticks'])/a['product']['cpu_ticks_per_second']
    for k in a['product']['io']:
        bucket[k]+=b['product']['io'][k]-a['product']['io'][k]
report['resource_by_poll_exposure']={k:dict(v) for k,v in sample_buckets.items()}
for p in ['crates/cc-server/src/semantic_runtime.rs','crates/cc-server/src/capability_status.rs','crates/cc-server/src/semantic_wiring.rs','crates/cc-semantic/src/queue.rs','crates/cc-semantic/src/cache.rs','crates/cc-semantic/src/publish.rs','crates/cc-db/src/document_store.rs','crates/cc-db/src/semantic_queue.rs','crates/cc-db/src/semantic_outbox.rs','crates/cc-db/src/sql/index_v1.sql','crates/cc-db/src/semantic_coverage.rs','crates/cc-db/src/index_db.rs']:
    blob(p,SOURCE)
    report['source_files'][p]=manifest[SOURCE+':'+p]
(OUT/'analysis.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
(OUT/'inputs-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
(OUT/'polls.json').write_text(json.dumps(polls,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['http','status','measurement_window','cleanup_tail']},ensure_ascii=False,indent=2))
