#!/usr/bin/env python3
"""Independent literal receipt interpretation; author expected/allowed fields unused."""
import copy, hashlib, json, pathlib, sys
DOCS = {
 'scope/needle.rs': 'pub fn needle() -> u32 { 731 }\n',
 'scope/sub/needle.rs': 'pub fn needle() -> u32 { 732 }\n',
 'scope/needle.py': 'from outside.callee import foreign\n\ndef needle():\n    return foreign() + 733\n',
 'scope/needle_class.py': 'class Needle:\n    pass\n',
 'scope_extra/needle.py': 'def needle():\n    return 998\n',
 'outside/needle.py': 'def needle():\n    return 999\n',
 'outside/callee.py': 'def foreign():\n    return 777\n'}
SCOPE = {'scope/needle.rs', 'scope/sub/needle.rs', 'scope/needle.py', 'scope/needle_class.py'}
CASES = {
 'all': (set(DOCS), set(DOCS)),
 'prefix': (SCOPE, SCOPE),
 'prefix-rust': ({'scope/needle.rs', 'scope/sub/needle.rs'}, {'scope/needle.rs', 'scope/sub/needle.rs'}),
 'explicit-empty-files': (set(), set()), 'explicit-empty-languages': (set(), set()),
 'dsl-conflict': (set(), set()), 'caller-dsl-language-conflict': (set(), set()),
 'files-intersection': ({'scope/needle.py'}, {'scope/needle.py'}),
 'kind-function': (SCOPE, {'scope/needle.rs', 'scope/sub/needle.rs', 'scope/needle.py'}),
 'kind-class': (SCOPE, {'scope/needle_class.py'}), 'absent-name': (SCOPE, set())}
LANES = {'exact_symbol', 'path', 'lexical', 'grep', 'graph', 'semantic'}


def audit(rows):
    assert {r['case'] for r in rows} == set(CASES) and len(rows) == 11
    identities, versions = {}, {}
    count = 0
    for row in rows:
        allowed, final = CASES[row['case']]
        assert {l['lane_id'] for l in row['lanes']} == LANES
        for lane in row['lanes']:
            assert lane['candidate_count'] == len(lane['candidates'])
            for candidate in lane['candidates']:
                count += 1
                chunk = candidate['legacy_chunk_id']
                assert chunk.startswith('chunk:')
                path, chunk_number = chunk[6:].rsplit(':', 1)
                assert chunk_number.isdigit()
                assert path in allowed
                doc = candidate['document']
                key, version = doc['doc_key'], doc['doc_version']
                assert len(key) == len(version) == 64
                assert identities.setdefault(key, path) == path
                assert versions.setdefault(chunk, version) == version
                span = candidate['source_span']
                assert 0 <= span['start'] < span['end'] <= len(DOCS[path].encode())
            if not allowed:
                assert not lane['candidates']
        hits = row['hits']
        assert {h['file_path'] for h in hits} == final
        expected_chunks = {'chunk:'+path+':0' for path in final}
        if 'scope/needle.py' in final:
            expected_chunks.add('chunk:scope/needle.py:1')
        if row['case'] == 'kind-function':
            expected_chunks.discard('chunk:scope/needle.py:0')
        assert {h['chunk_id'] for h in hits} == expected_chunks and len(hits) == len(expected_chunks)
        for hit in hits:
            text = hit['text']; path = hit['file_path']
            assert text and text in DOCS[path]
            lines = DOCS[path].splitlines(keepends=True)
            assert hit['start_line'] >= 1 and hit['end_line'] >= hit['start_line']
            span = hit['metadata']['source_evidence']['span']
            assert DOCS[path].encode()[span['start']:span['end']].decode() == text, 'literal byte-addressed source'
            raw = DOCS[path].encode()
            expected_lines = [raw[:span['start']].count(b'\n')+1, raw[:span['end']-1].count(b'\n')+1]
            assert [hit['start_line'], hit['end_line']] == expected_lines, 'byte-span line boundaries; slice may start at preceding line newline'
            if row['case'] == 'kind-function':
                assert hit['symbol_kind'] == 'function' and hit['symbol_name'].lower() == 'needle'
            if row['case'] == 'kind-class':
                assert hit['symbol_kind'] == 'class' and hit['symbol_name'] == 'Needle'
        if row['case'] == 'all':
            assert all(l['candidates'] for l in row['lanes'])
            graph = next(l for l in row['lanes'] if l['lane_id'] == 'graph')
            assert any(c['legacy_chunk_id'] == 'chunk:outside/callee.py:0' and c['exact_identity'] is False for c in graph['candidates'])
    assert set(identities.values()) == set(DOCS)
    return {'cases': len(rows), 'lane_receipts': len(rows)*6, 'candidates': count, 'hits': sum(len(r['hits']) for r in rows), 'real_cross_file_nonseed_graph': True, 'author_allowed_expected_consumed': False, 'production_predicates_consumed': False, 'doc_versions': versions, 'byte_and_line_boundaries': 'passed'}


if __name__ == '__main__':
    p = pathlib.Path(sys.argv[1]); rows = json.loads(p.read_text())
    result = audit(rows)
    mutants = {}
    for name, mutate in [
        ('omit_legal_final', lambda r: r[0]['hits'].pop()),
        ('empty_source', lambda r: r[0]['hits'][0].update(text='')),
        ('wrong_line', lambda r: r[0]['hits'][0].update(start_line=99, end_line=99)),
        ('wrong_byte_span', lambda r: r[0]['hits'][0]['metadata']['source_evidence'].update(span={'start': 0, 'end': 1})),
        ('erase_cross_file_graph', lambda r: next(l for l in r[0]['lanes'] if l['lane_id']=='graph').update(candidates=[c for c in next(l for l in r[0]['lanes'] if l['lane_id']=='graph')['candidates'] if c['legacy_chunk_id']!='chunk:outside/callee.py:0']))]:
        mutant = copy.deepcopy(rows); mutate(mutant)
        try:
            audit(mutant)
        except AssertionError:
            mutants[name] = 'rejected'
        else:
            raise AssertionError(name + ' unexpectedly admitted')
    result.update(raw_sha256=hashlib.sha256(p.read_bytes()).hexdigest(), negative_controls=mutants)
    print(json.dumps(result, indent=2))
