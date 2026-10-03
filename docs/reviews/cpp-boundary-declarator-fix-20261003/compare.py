"""Compare retained PR134 evidence to fixed outputs, without changing gold/scorer."""
import json, subprocess
from pathlib import Path
p = Path(__file__).resolve().parent
prefix = '61390db1493718e1d1cb3c65e5061f91aaeedabb:docs/reviews/qname-parser-public-20261003/repair-f4df9a8/'
def original(name):
    return subprocess.check_output(['git', 'show', prefix+name])
def old(name):
    return json.loads(original(name))
def fixed(name):
    return json.loads((p/name).read_bytes())
for name in ['cpp-micro-gold.json', 'micro-gold.json', 'wrong-gold.json']:
    assert original('current/'+name) == (p/'fixed'/name).read_bytes()
    print('unchanged original gold bytes:', name)
assert original('native_driver.rs') == (p/'native_driver.rs').read_bytes()
print('original independent driver byte-identical; before argument chooses recall=1 assertion')
for api in ['engine', 'mcp']:
    assert old('current/'+api+'.json')['machine_pack']['hits'] == fixed('fixed/'+api+'.json')['machine_pack']['hits']
    print(api, 'all five ordinary Python hits exact: fields, scores, source and document proof')
    for query, payload in fixed('fixed/decorated-class-public.json').items():
        actual = sorted((h['symbol_name'], h['symbol_kind'], h['metadata']['qname']) for h in payload[api]['machine_pack']['hits'])
        assert actual == sorted([('Outer','class','Outer'),('Inner','class','Outer.Inner'),('pulse','method','Outer.Inner.pulse')])
        print(api, query, 'Class/Class/Method proofs preserved')
    hits = fixed('fixed/cpp-public.json')['leaf'][api]['machine_pack']['hits']
    assert any(h['symbol_name']=='leaf' and h['symbol_kind']=='function' and 'qname' not in h['metadata'] for h in hits)
    assert not any(h.get('symbol_name')=='T' and h.get('symbol_kind')=='function' for h in hits)
    assert old('current/cpp-native-scores.json')[api]['recall10'] == 0
    assert fixed('fixed/cpp-native-scores.json')[api]['recall10'] == 1
    print(api, 'leaf/function/qname omitted; unchanged native recall10: review current=0 fixed=1')
a, b = old('guard-current.json'), fixed('guard-fixed.json')
for path in a:
    assert a[path]['symbols'] == b[path]['symbols']
    if path != 'micro.cpp':
        assert a[path]['chunks'] == b[path]['chunks']
        assert a[path]['identities'] == b[path]['identities']
    else:
        def identity_without_document(rows):
            return [{k:v for k,v in row.items() if k != 'document'} for row in rows]
        assert identity_without_document(a[path]['identities']) == identity_without_document(b[path]['identities'])
        assert not any(row['name']=='leaf' for row in b[path]['identities'])
        print('C++ admitted identity tuples unchanged, no leaf identity invented; ancestor document versions reflect corrected structural name')
    print(path, 'native symbols unchanged; chunks and identities equal:', a[path]['chunks']==b[path]['chunks'], a[path]['identities']==b[path]['identities'])
print('Fresh independent DB envelope incarnation/timings/packing bytes are runtime information; no full-envelope equality claim.')
