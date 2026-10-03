"""Final bounded independent comparisons; uses actual outputs, no injection."""
import json
from pathlib import Path
p=Path(__file__).resolve().parent
previous=p.parent/'repair-f4df9a8'
load=lambda path: json.loads(path.read_text())
for file in ['cpp-micro-gold.json','micro-gold.json','wrong-gold.json']:
    assert (p/'fixed'/file).read_bytes()==(previous/'current'/file).read_bytes(),file
print('PASS original C++ and Python gold bytes unchanged')
for api in ['engine','mcp']:
    assert load(p/'fixed'/f'{api}.json')['machine_pack']['hits']==load(previous/'current'/f'{api}.json')['machine_pack']['hits']
    print('PASS',api,'ordinary Python complete hit fields/metadata/scores/traces/proofs unchanged')
    for q,payload in load(p/'fixed'/'decorated-class-public.json').items():
        actual=sorted((h['symbol_name'],h['symbol_kind'],h['metadata']['qname']) for h in payload[api]['machine_pack']['hits'])
        assert actual==sorted([('Outer','class','Outer'),('Inner','class','Outer.Inner'),('pulse','method','Outer.Inner.pulse')])
        print('PASS',api,q,'Python Class/Class/Method proof preserved')
    hits=load(p/'fixed'/'cpp-public.json')['leaf'][api]['machine_pack']['hits']
    assert not any(h.get('symbol_name')=='T' for h in hits)
    leaf=[h for h in hits if h.get('symbol_name')=='leaf']
    assert len(leaf)==1
    h=leaf[0]
    assert h['symbol_name']=='leaf' and h['symbol_kind']=='function'
    assert 'qname' not in h['metadata']
    old=load(previous/'current'/'cpp-public.json')['leaf'][api]['machine_pack']['hits'][0]
    assert h['metadata']['source_evidence']==old['metadata']['source_evidence']
    assert load(p/'fixed'/'cpp-native-scores.json')[api]['recall10']==1
    assert load(previous/'current'/'cpp-native-scores.json')[api]['recall10']==0
    scores=load(p/'fixed'/'scores.json')[api]
    assert scores['correct']['recall10']==1 and scores['wrong']['recall10']==0
    print('P2 CLOSED',api,'leaf / function / absent qname, identical exact source proof; native recall10 restored 0 → 1')
