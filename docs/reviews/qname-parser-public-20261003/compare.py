"""Compare actual full outputs; no field injection or score fabrication."""
import json
from pathlib import Path
root = Path(__file__).resolve().parent
load = lambda mode, name: json.loads((root / mode / name).read_text())
for api in ['engine', 'mcp']:
    old = load('baseline', api + '.json')['machine_pack']['hits']
    new = load('current', api + '.json')['machine_pack']['hits']
    assert len(old) == len(new)
    for x, y in zip(old, new):
        for key, value in x.items():
            if key != 'metadata':
                assert value == y[key], (api, key)
        for key, value in x['metadata'].items():
            assert value == y['metadata'][key], (api, key)
    print(f'PASS {api}: all {len(old)} hits preserve all old fields/metadata, scores/traces/source/document')
for mode in ['baseline', 'current']:
    scores = load(mode, 'scores.json')
    for api in ['engine', 'mcp']:
        assert scores[api]['correct']['recall10'] == (0 if mode == 'baseline' else 1)
        assert scores[api]['wrong']['recall10'] == 0
    print(f'PASS {mode}: real engine/MCP native micro scores')
for mode in ['baseline', 'current']:
    data = load(mode, 'decorated-class-public.json')
    for query, payload in data.items():
        for api, envelope in payload.items():
            hits = envelope['machine_pack']['hits']
            if mode == 'baseline':
                assert any(h.get('symbol_name') == 'Inner' and h.get('symbol_kind') == 'class' for h in hits)
            else:
                assert len(hits) == 1
                assert hits[0]['symbol_kind'] == 'function'
                assert hits[0]['metadata']['qname'] == 'Outer'
            print(mode, query, api, [(h.get('symbol_name'),h.get('symbol_kind'),h.get('metadata', {}).get('qname')) for h in hits])
print('REJECT product: decorated classes become public Function identity, losing nested class/member results')
