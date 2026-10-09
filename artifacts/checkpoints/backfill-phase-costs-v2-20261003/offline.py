import pathlib,gzip,json,statistics
O=pathlib.Path(__file__).resolve().parent
P=O/'source/artifacts/checkpoints/independent-v2-100k-release-20261003'
s=json.load(gzip.open(P/'summary.json.gz','rt'));start=s['drain_start_ns'];end=start+300_000_000_000
h=[json.loads(l) for l in gzip.open(P/'live/n100000/http.jsonl.gz','rt')];ent={r['sequence']:r for r in h if r['event']=='entered'};ret={r['sequence']:r for r in h if r['event']=='returned'}
def stat(a):
 a=sorted(a);return {'n':len(a),'sum_ms':sum(a),'mean_ms':statistics.mean(a),'p50_ms':a[len(a)//2],'p95_ms':a[int(len(a)*.95)],'max_ms':a[-1]}
serv=[];gap=[];overlap=0
for seq,e in ent.items():
 if seq in ret and start<=e['time_ns']<ret[seq]['time_ns']<=end:serv.append((ret[seq]['time_ns']-e['time_ns'])/1e6)
 if seq+1 in ent and seq in ret and start<=ret[seq]['time_ns']<ent[seq+1]['time_ns']<=end:gap.append((ent[seq+1]['time_ns']-ret[seq]['time_ns'])/1e6)
 if seq+1 in ent and seq in ret and ent[seq+1]['time_ns']<ret[seq]['time_ns']:overlap+=1
windows=[]
for i in range(6):
 lo=start+i*50_000_000_000;hi=lo+50_000_000_000
 ids=[q for q,r in ret.items() if lo<=r['time_ns']<hi]
 windows.append({'window_s':[i*50,(i+1)*50],'returned':len(ids),'server_ms':stat([(ret[q]['time_ns']-ent[q]['time_ns'])/1e6 for q in ids if ent[q]['time_ns']>=start]) if ids else None,'gap_ms':stat([(ent[q+1]['time_ns']-ret[q]['time_ns'])/1e6 for q in ids if q+1 in ent and ent[q+1]['time_ns']<=hi]) if ids else None})
rpc=[json.loads(l) for l in gzip.open(P/'live/n100000/rpc.jsonl.gz','rt')];req={r['payload']['id']:r for r in rpc if r['event']=='request'};resp={r['payload']['id']:r for r in rpc if r['event']=='response' and 'id'in r['payload']}
lat=[(resp[q]['time_ns']-r['sent_ns'])/1e6 for q,r in req.items() if q in resp and r['payload'].get('params',{}).get('name')=='status']
o={'source':'29b03a0fec6bfde92aac9c41dce996ebb2d08cc7','pr108_head':'99973e7d6faf3809add4a12cd444f8e69f04d93c','drain_start_ns':start,'http_service_excluding_initial_fixture_hold':stat(serv),'serial_inter_http_gap':stat(gap),'adjacent_request_overlaps':overlap,'status_client_span':stat(lat),'windows':windows,'note':'HTTP entered-to-returned measures mock handler+fixture logging, excludes client transport/decode. Initial fixture-held request excluded from service sample. Gaps include production work, client transport edges, scheduling, rounds and status contention; cannot attribute directly to SQL.'}
(O/'offline-http-rpc.json').write_text(json.dumps(o,indent=2)+'\n');print(json.dumps(o,indent=2))
