"""Validate independent observations; reject the no-dependency-loss requirement."""
import ast, hashlib, json, pathlib
root = pathlib.Path(__file__).resolve().parent
old = json.loads((root/'old-controls/results.json').read_text())
fixed = json.loads((root/'fixed-controls/results.json').read_text())
for record in old['fixtures']:
    if record['name'] in ('old-negative', 'named-variadic'):
        assert 'invalid dependency key' in record['result']['error']
    else:
        assert record['result']['report']['files_parsed'] == 1
        assert not record['result']['report']['parse_errors']
for record in fixed['fixtures']:
    r = record['result']
    assert r['report']['files_parsed'] == 1
    assert not r['report']['parse_errors']
    assert r['empty_dependencies'] == 0
assert all(x['passed'] for x in fixed['atom_controls'])
assert 'invalid dependency key' in fixed['strict_validator_error']
before = json.loads((root/'legal-old/legal-identifiers.json').read_text())
after = json.loads((root/'legal-fixed/legal-identifiers.json').read_text())
for a,b in zip(before,after):
    assert a['name'] == b['name']
    assert a['uses_type_edges'] == 1
    assert b['uses_type_edges'] == 0
    assert a['atoms'] == [a['type_name']] and b['atoms'] == []
    assert not a['result']['report']['parse_errors']
    assert not b['result']['report']['parse_errors']
    if a['name'].startswith('python'):
        assert a['name_dependency'] and not b['name_dependency']
        path = root/'legal-fixed'/b['name']/'sample.py'
        ast.parse(path.read_text())
        assert b['type_name'].isidentifier()
requests = json.loads((root/'requests-fixed/full-index.json').read_text())
assert requests['report']['files_parsed'] == requests['report']['files_scanned'] == 19
assert not requests['report']['parse_errors']
assert requests['dependency_count'] == 2420
assert requests['empty_dependencies'] == 0
assert len(requests['validated_manifests']) == 19
negative = json.loads((root/'requests-old/full-index.json').read_text())
assert 'exceptions.py: invalid resolution manifest: invalid dependency key' == negative['error']
evidence = {}
for p in root.rglob('*'):
    if p.is_file() and not any(x in p.parts for x in ('snapshots','public-requests','requests-main-source','harness-old','harness-fixed')):
        if p.suffix in ('.json','.rs','.py','.log') and p.name not in ('evidence-sha256.json',):
            evidence[str(p.relative_to(root))] = hashlib.sha256(p.read_bytes()).hexdigest()
(root/'evidence-sha256.json').write_text(json.dumps(evidence,indent=2))
print('Independent evidence verified. BOUNDED_REJECT: four legal type names lose uses_type edges; three lose name dependencies.')
