"""Compare real envelopes and unchanged native micro scoring; no injected fields."""
import hashlib, json
from pathlib import Path
root = Path(__file__).resolve().parent
oldroot = root.parent/'qname-parser-public-20261003'
load = lambda p: json.loads(p.read_text())
for api in ['engine','mcp']:
    old = load(oldroot/'current'/f'{api}.json')['machine_pack']['hits']
    new = load(root/'fixed'/f'{api}.json')['machine_pack']['hits']
    assert len(old) == len(new) == 5
    for x,y in zip(old,new):
        assert x == y, (api, x['chunk_id'], [(k,x.get(k),y.get(k)) for k in set(x)|set(y) if x.get(k)!=y.get(k)])
    print(f'PASS {api}: all 5 normal-method hits byte-value-identical in every field including scores/traces/source/document/qname')
    scores = load(root/'fixed'/'scores.json')
    assert scores[api]['correct']['recall10'] == 1
    assert scores[api]['wrong']['recall10'] == 0
    for name in ['micro-gold.json','wrong-gold.json']:
        assert (root/'fixed'/name).read_bytes() == (oldroot/'current'/name).read_bytes()
    print(f'PASS {api}: unchanged native evaluator, original micro gold, correct=1 / wrong=0; original missing baseline=0 retained')
source = b'@decorate\nclass Outer:\n    @decorate\n    class Inner:\n        def pulse(self): return 1\n'
expected = {('Outer','class','Outer'),('Inner','class','Outer.Inner'),('pulse','method','Outer.Inner.pulse')}
data = load(root/'fixed'/'decorated-class-public.json')
for query,payload in data.items():
    for api,envelope in payload.items():
        hits = envelope['machine_pack']['hits']
        actual = {(h['symbol_name'], h['symbol_kind'], h['metadata']['qname']) for h in hits}
        assert actual == expected, (query,api,actual)
        for h in hits:
            p = h['metadata']['source_evidence']
            assert p['source']['byte_len'] == len(source)
            assert source[p['span']['start']:p['span']['end']].decode() == h['text']
            owner = source[p['owner']['start']:p['owner']['end']].decode()
            assert owner.startswith('@decorate' if h['symbol_kind']=='class' else 'def pulse')
        print(f'PASS {query} {api}: {sorted(actual)}; exact original source slices and canonical wrapper owners')
print('PASS decorated-class regression repaired: no false Function class identity; independent Inner and pulse results remain')
