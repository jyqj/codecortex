"""Offline binding of retained compound reads to exact original stdio RPCs."""
import hashlib
import json
from collections import Counter

def require(ok, message):
    if not ok: raise ValueError(message)

def tool_value(payload):
    require('error' not in payload and isinstance(payload.get('result'), dict), 'failed/missing original RPC result')
    response = payload['result']
    require(response.get('isError') is not True and isinstance(response.get('structuredContent'), dict), 'missing original tool structured content')
    value = response['structuredContent']
    return value.get('result', value)

def transport(events):
    kinds=Counter();requests={};responses={};wire={};times={};eof=exit0=False
    for event in events:
        kind=event.get('kind') if event.get('event')=='terminal' else event.get('event')
        kinds[kind]+=1
        if kind in ('request','response'):
            payload=event['payload'];mapping=requests if kind=='request' else responses
            require(payload['id'] not in mapping,'duplicate transport ID')
            mapping[payload['id']]=payload
            times.setdefault(payload['id'],{})[kind]=event['time_ns']
        if kind=='stdout_wire':
            data=event['text'].encode()
            require(len(data)==event['wire_bytes'] and hashlib.sha256(data).hexdigest()==event['wire_sha256'],'raw stdout wire bytes')
            payload=json.loads(data)
            if 'id' in payload:
                require(payload['id'] not in wire,'duplicate wire response')
                wire[payload['id']]=payload
        if kind=='process_exit':exit0=event.get('exit_code',event.get('code'))==0
        if kind in ('stdout_eof','eof'):eof=True
    require(eof and exit0 and set(requests)==set(responses)==set(wire) and responses==wire,'owned transport stopped with exact complete responses')
    require(all(v['request']<=v['response'] for v in times.values()),'RPC response precedes request')
    return requests,responses,times,dict(kinds)

def bind_compound_reads(rows,requests,responses,times):
    """Search order anchors the shared clock offset; status samplers stay separate.

    No relation between raw operation IDs and RPC IDs is assumed. Original
    per-call relative intervals must contain exact wire request/response times
    under one common monotonic origin. Exact payloads and responses must agree.
    """
    reads=sorted((r for r in rows if r['operation']=='read'),key=lambda r:r['call_started_ns'])
    search_ids=sorted((i for i,p in requests.items() if p.get('method')=='tools/call' and p.get('params',{}).get('name')=='search'),key=lambda i:times[i]['request'])
    calls=[(r,c) for r in reads for c in r['cache_probe']['requests'] if c['name']=='search']
    require(len(search_ids)==len(calls)==2*len(reads),'all original symbol/hybrid RPCs accounted')
    origin_low=None;origin_high=None;used=set();mapping=[]
    def candidate(call,i):
        p=requests[i]
        if p.get('method')!='tools/call' or p.get('params')!={'name':call['name'],'arguments':call['arguments']}:return None
        if tool_value(responses[i])!=call['response']:return None
        low=times[i]['response']-call['finished_ns'];high=times[i]['request']-call['started_ns']
        if low>high:return None
        return low,high
    def bind(row,call,i,bounds):
        nonlocal origin_low,origin_high
        require(i not in used,'one wire RPC reused for multiple cache calls')
        low,high=bounds
        origin_low=low if origin_low is None else max(origin_low,low)
        origin_high=high if origin_high is None else min(origin_high,high)
        require(origin_low<=origin_high,'no common raw/wire monotonic clock origin')
        used.add(i);mapping.append({'operation_id':row['id'],'role':call['role'],'rpc_id':i})
    for (row,call),i in zip(calls,search_ids):
        bounds=candidate(call,i);require(bounds is not None,'original ordered search RPC payload/response differs')
        bind(row,call,i,bounds)
    status_ids=[i for i,p in requests.items() if p.get('method')=='tools/call' and p.get('params',{}).get('name')=='status']
    def response_key(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
    status_by_response={}
    for i in status_ids:status_by_response.setdefault(response_key(tool_value(responses[i])),[]).append(i)
    for row in reads:
        for call in row['cache_probe']['requests']:
            if call['name']!='status':continue
            candidates=[]
            for i in status_by_response.get(response_key(call['response']),[]):
                if i in used:continue
                bounds=candidate(call,i)
                if bounds is not None and max(origin_low,bounds[0])<=min(origin_high,bounds[1]):candidates.append((i,bounds))
            require(len(candidates)==1,'cache status lacks a unique exact original wire response/timing match')
            i,bounds=candidates[0];bind(row,call,i,bounds)
    require(len(used)==4*len(reads),'full four-RPC read denominator')
    return {'compound_reads':len(reads),'bound_RPCs':len(used),'role_counts':dict(Counter(x['role'] for x in mapping)),
            'mapping':mapping,'unbound_status_RPCs':len(set(status_ids)-used),
            'common_monotonic_origin_interval_ns':[origin_low,origin_high],
            'scope':'Exact request arguments and full response bytes/JSON bound to original wire IDs and common clock; sampler status RPCs retained separately, no contiguous ID assumption.'}
