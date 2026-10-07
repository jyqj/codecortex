"""Independent comparisons of actual snapshots and real complete API outputs."""
import json
from pathlib import Path
p = Path(__file__).resolve().parent
load = lambda name: json.loads((p / name).read_text())
a, b = load('guard-before.json'), load('guard-current.json')
for path in a:
    x,y=a[path],b[path]
    assert x['symbols']==y['symbols'], path
    print(path,'parser symbols unchanged; chunks equal:',x['chunks']==y['chunks'],'identities equal:',x['identities']==y['identities'])
    for old,new in zip(x['structure']['boundaries'],y['structure']['boundaries']):
        if old!=new:
            print(' boundary:',{k:(v,new.get(k)) for k,v in old.items() if v!=new.get(k)})
for api in ['engine','mcp']:
    old=json.loads((p.parent/'current'/f'{api}.json').read_text())['machine_pack']['hits']
    new=load(f'current/{api}.json')['machine_pack']['hits']
    assert old==new
    print('PASS',api,'all five ordinary-method hits equal including complete metadata, scores/traces/proofs')
    for query,payload in load('current/decorated-class-public.json').items():
        hits=payload[api]['machine_pack']['hits']
        actual=sorted((h['symbol_name'],h['symbol_kind'],h['metadata']['qname']) for h in hits)
        assert actual==sorted([('Outer','class','Outer'),('Inner','class','Outer.Inner'),('pulse','method','Outer.Inner.pulse')])
        print('P1 CLOSED',query,api,actual)
    before=load('before/cpp-public.json')['leaf'][api]['machine_pack']['hits']
    current=load('current/cpp-public.json')['leaf'][api]['machine_pack']['hits']
    assert any(h.get('symbol_name')=='leaf' for h in before)
    assert all(h.get('symbol_name')!='leaf' for h in current)
    assert any(h.get('symbol_name')=='T' and h.get('symbol_kind')=='function' for h in current)
    assert load('before/cpp-native-scores.json')[api]['recall10']==1
    assert load('current/cpp-native-scores.json')[api]['recall10']==0
    print('P2 REJECT',api,'C++ leaf becomes T; actual native name-only recall10 1 → 0')
