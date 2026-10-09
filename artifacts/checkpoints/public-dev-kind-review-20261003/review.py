#!/usr/bin/env python3
"""Frozen public DEV kind audit. Emits hashes/coordinates, never source/query/gold bodies.

Product output is not read. No evaluation, imports of author tools, or defaults changed.
"""
import ast
import copy
from collections import Counter
import hashlib
import json
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
ADMISSION = '5385f5a7a2a875c6d5cbd049bdde039bf71bbf32'
PRODUCT = '88f2cf099c8b81f3acef485fd5ac9b01c63ce790'
PREFIX = 'crates/cc-eval/benchmarks/public-v19/'
ADMISSION_PATH = PREFIX+'protocol/global-dev-review/typescript-extension/admission.json'
DECODER = json.JSONDecoder()

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()

def blob(entry):
    return subprocess.check_output(['git', 'show', entry], cwd=REPO)

def write(name, value):
    (HERE/name).write_text(json.dumps(value, indent=2, sort_keys=True)+'\n')

def raw_array(text):
    """Exact JSON element tokens; no reconstructed raw-hash claims."""
    i = text.index('[')+1
    result = []
    while True:
        while text[i].isspace() or text[i] == ',': i += 1
        if text[i] == ']': return result
        value, end = DECODER.raw_decode(text, i)
        result.append((value, text[i:end]))
        i = end

def raw_field(text, wanted):
    i = text.index('{')+1
    while True:
        while text[i].isspace() or text[i] == ',': i += 1
        if text[i] == '}': raise KeyError(wanted)
        key, i = DECODER.raw_decode(text, i)
        while text[i].isspace() or text[i] == ':': i += 1
        start = i
        _, i = DECODER.raw_decode(text, i)
        if key == wanted: return text[start:i]

def python_definitions(body):
    lines = body.splitlines(keepends=True)
    offsets = [0]
    for line in lines: offsets.append(offsets[-1]+len(line))
    result = []
    def visit(node, scope=(), owner=None):
        scoped = isinstance(node, (ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef))
        if scoped:
            decorators = [ast.unparse(d) for d in node.decorator_list]
            kind = 'class' if isinstance(node, ast.ClassDef) else 'method' if owner == 'class' else 'function'
            subtype = ('static_method' if 'staticmethod' in decorators else
                       'class_method' if 'classmethod' in decorators else
                       'property_accessor' if any(d == 'property' or d.endswith('.setter') or d.endswith('.deleter') for d in decorators) else
                       'instance_method' if kind == 'method' else kind)
            result.append(dict(name=node.name, scope='.'.join(scope+(node.name,)), kind=kind,
                               subtype=subtype, decorators=decorators,
                               start=offsets[node.lineno-1]+node.col_offset,
                               end=offsets[node.end_lineno-1]+node.end_col_offset))
            scope = scope+(node.name,)
            owner = 'class' if isinstance(node, ast.ClassDef) else 'function'
        for child in ast.iter_child_nodes(node): visit(child, scope, owner)
    visit(ast.parse(body))
    return result

def go_definitions(body):
    return json.loads(subprocess.check_output([str(HERE/'.scratch/go-taxonomy')], input=json.dumps(body.decode()).encode()))

def project(q):
    groups = sorted(q['answers'], key=lambda g: not g['primary'])
    return list(dict.fromkeys(a['path'] for g in groups for a in g['alternatives']))

def main():
    admission_raw = blob(ADMISSION+':'+ADMISSION_PATH)
    assert sha(admission_raw) == 'b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425'
    admission = json.loads(admission_raw)
    locks = admission['inputs_sha256']
    input_hashes = {ADMISSION+':'+ADMISSION_PATH:sha(admission_raw)}
    component_entry = ADMISSION+':'+PREFIX+'protocol/global-dev-review/typescript-extension/global-components.json'
    component_raw = blob(component_entry)
    input_hashes[component_entry] = sha(component_raw)
    global_components = json.loads(component_raw)['components']
    def locked(entry):
        raw = blob(entry)
        assert sha(raw) == locks[entry], entry
        input_hashes[entry] = sha(raw)
        return raw
    protocol_entry = ADMISSION+':'+PREFIX+'protocol/PREREGISTRATION.md'
    protocol = blob(protocol_entry)
    input_hashes[protocol_entry] = sha(protocol)
    assert b'all supplied symbol identity fields' in protocol
    # Historical author/owner instructions are corroboration, never imported/executed.
    for entry in [
        admission['repo_results']['requests']['author_sha']+':'+PREFIX+'requests/scripts/author.py',
        admission['repo_results']['requests']['author_sha']+':'+PREFIX+'requests/provenance/protocol/PREREGISTRATION.md',
        admission['repo_results']['requests']['review_sha']+':'+PREFIX+'reviews/requests/review_dev20.py']:
        input_hashes[entry] = sha(blob(entry))
    rows_out, repo_counts = [], {}
    licenses = {}
    for repo in ['requests', 'gin']:
        receipt = admission['repo_results'][repo]
        author = receipt['author_sha']
        base = author+':'+PREFIX+repo+'/'
        native_suite = json.loads(locked(receipt['native_dev_suite_entry']))
        compat_suite = json.loads(locked(receipt['compat_dev_suite_entry']))
        raw_queries = locked(base+native_suite['queries'])
        compat_raw = locked(base+compat_suite['queries'])
        queries = [json.loads(line) for line in raw_queries.splitlines()]
        hypothetical_queries = copy.deepcopy(queries)
        compat = {q['query_family']:q for q in map(json.loads, compat_raw.splitlines())}
        assert len(queries) == receipt['native_dev_rows']
        assert all(q['split'] == 'dev' for q in queries)
        # Read only frozen public DEV gold/source maps and owner review receipts.
        if repo == 'requests':
            gold_entry = base+'gold/dev.json'
            gold_raw = blob(gold_entry)
            input_hashes[gold_entry] = sha(gold_raw)
            gold = {g['family_id']:(g, token) for g, token in raw_array(gold_raw.decode())}
            files = ['dev20-review.json','block02-review.json','block03-review.json','block04-review.json','delta-dev-review.json']
            review_entries = [receipt['review_sha']+':'+PREFIX+'reviews/requests/'+f for f in files]
        else:
            gold_entry = base+'gold/evidence.json'
            gold_raw = blob(gold_entry)
            input_hashes[gold_entry] = sha(gold_raw)
            gold = {g['family']:(g, token) for g, token in raw_array(gold_raw.decode())}
            assert set(gold) == {q['query_family'] for q in queries}, 'gold must be DEV-only'
            review_entries = [receipt['source_review_sha']+':'+PREFIX+'reviews/gin/dev-067-review.json',
                              receipt['review_sha']+':'+PREFIX+'reviews/gin/repair-v2/dev-004-repair-review.json']
        reviewers = []
        for entry in review_entries:
            raw = blob(entry)
            input_hashes[entry] = sha(raw)
            for r in json.loads(raw)['rows']:
                reviewers.append((entry, r))
        cache, counts, affected_queries, affected_groups = {}, Counter(), set(), set()
        for qi, (q, line) in enumerate(zip(queries, raw_queries.splitlines(keepends=True))):
            family = q['query_family']
            family_hash = sha(family.encode())
            owner = [(e,r) for e,r in reviewers if r.get('family_id') == family or r.get('family_id_sha256') == family_hash]
            assert owner
            # Repair/current receipts override prior content decisions; prior hashes retained.
            assert owner[-1][1]['decision'] == 'accept'
            g, gold_token = gold[family]
            current_owner = owner[-1][1]
            if repo == 'requests':
                assert current_owner['question_sha256'] == sha(canonical(q))
                assert current_owner['gold_record_sha256'] == sha(canonical(g))
            else:
                assert current_owner.get('new_native_row_sha256', current_owner.get('row_sha256')) == sha(line)
            evidence = g['evidence'] if repo == 'requests' else q['annotations']['v19']['gold_evidence']
            if repo == 'gin': assert g == dict(family=family, **q['annotations']['v19'])
            counts['native_rows'] += 1
            if q['answers']:
                assert project(q) == compat[family]['expected_files']
                counts['compat_projections_verified'] += 1
            answers_raw = raw_field(line.decode(), 'answers')
            for gi, (group, group_token) in enumerate(raw_array(answers_raw)):
                for ai, (alt, alt_token) in enumerate(raw_array(raw_field(group_token, 'alternatives'))):
                    counts['all_alternatives'] += 1
                    path = alt['path']
                    entry = base+'source/'+path
                    if path not in cache:
                        body = locked(entry)
                        definitions = python_definitions(body) if repo == 'requests' else go_definitions(body)
                        cache[path] = (body, definitions)
                    body, definitions = cache[path]
                    span, symbol = alt['span'], alt['symbol']
                    start, end = span['start'], span['end']
                    assert 0 <= start < end <= len(body)
                    body[:start].decode(); body[start:end].decode()
                    witnesses = [e for e in evidence if e['path'] == path and
                                 (e.get('start_byte',e.get('span',{}).get('start')), e.get('end_byte',e.get('span',{}).get('end'))) == (start,end)]
                    assert len(witnesses) == 1
                    witness = witnesses[0]
                    assert sha(body[start:end]) == witness.get('span_sha256',witness.get('sha256'))
                    assert witness['symbol'].split('.')[-1] == symbol['name']
                    counts['source_map_bindings_verified'] += 1
                    if symbol['kind'] != 'function':
                        counts['non_function_controls_source_map_verified'] += 1
                        continue
                    matches = [d for d in definitions if d['name'] == symbol['name'] and d['start'] == start and
                               d['end'] <= end and not body[d['end']:end].strip()]
                    assert len(matches) == 1, (repo, qi, gi, ai, 'ambiguous source binding')
                    declaration = matches[0]
                    if repo == 'requests':
                        assert symbol['qname'] == 'requests.'+Path(path).stem+'.'+declaration['scope']
                        assert witness['symbol'] == declaration['scope']
                    elif declaration['receiver']:
                        assert witness['symbol'] == declaration['receiver']+'.'+declaration['name']
                    counts['taxonomy:'+symbol['kind']+'->'+declaration['kind']] += 1
                    if symbol['kind'] != 'function' or declaration['kind'] != 'method': continue
                    affected_queries.add(qi); affected_groups.add((qi,gi))
                    hypothetical_queries[qi]['answers'][gi]['alternatives'][ai]['symbol']['kind'] = 'method'
                    subtype = declaration.get('subtype','receiver_method')
                    counts['affected_subtype:'+subtype] += 1
                    counts['affected_primary_groups_alternatives' if group['primary'] else 'affected_supporting_groups_alternatives'] += 1
                    rows_out.append(dict(repo=repo, query_ordinal=qi, group_ordinal=gi, alternative_ordinal=ai,
                        query_id_sha256=sha(q['id'].encode()), family_id_sha256=family_hash,
                        original_query_line_raw_sha256=sha(line), raw_line_includes_line_ending=True,
                        original_query_text_json_token_sha256=sha(raw_field(line.decode(),'query').encode()),
                        original_gold_record_raw_sha256=sha(gold_token.encode()),
                        original_answers_raw_sha256=sha(answers_raw.encode()),
                        original_alternative_raw_sha256=sha(alt_token.encode()),
                        original_qname_sha256=sha(canonical(symbol.get('qname'))),
                        source_entry=entry, source_sha256=sha(body), sourcecoords=span,
                        source_fragment_sha256=sha(body[start:end]), declarationcoords={'start':declaration['start'],'end':declaration['end']},
                        source_map_entry=gold_entry, source_map_record_sha256=sha(canonical(witness)),
                        owner_reviews=[dict(entry=e, row_sha256=sha(canonical(r)), decision=r['decision']) for e,r in owner],
                        original_kind='function', declaration_kind='method', declaration_subtype=subtype,
                        classification='documented_broad_function_convention' if repo == 'requests' else 'unable_to_determine_original_kind_convention',
                        protocol_resolution='open_versioned_proposal_required',
                        proposed_kind_if_declaration_taxonomy_approved='method', applied=False,
                        primary=group['primary'], required_facet_links=sum(f.get('group_id') == group['id'] for f in q['annotations']['v19']['facets'])))
        counts['source_files_read'] = len(cache)
        counts['affected_queries'] = len(affected_queries)
        counts['affected_groups'] = len(affected_groups)
        counts['affected_alternatives'] = sum(r['repo']==repo for r in rows_out)
        affected_families = {queries[qi]['query_family'] for qi in affected_queries}
        counts['affected_global_correlation_components'] = sum(bool(affected_families.intersection(c['members'])) for c in global_components)
        relations_entry = base+'relations.json'
        relations = json.loads(locked(relations_entry))
        parent = {q['query_family']:q['query_family'] for q in queries}
        def find(x):
            while parent[x] != x: x = parent[x]
            return x
        for c in relations['components']:
            members = [m for m in c['members'] if m in parent]
            for m in members[1:]: parent[find(m)] = find(members[0])
        counts['affected_local_correlation_components'] = len({find(f) for f in affected_families})
        counts['original_local_correlation_components'] = len({find(f) for f in parent})
        assert counts['original_local_correlation_components'] == receipt['local_components']
        counts['affected_required_facet_links'] = sum(r['required_facet_links'] for r in rows_out if r['repo']==repo)
        # Hypothetical proposal simulation only: exact structural identity after undoing kind.
        for original, candidate in zip(queries, hypothetical_queries):
            assert project(original) == project(candidate)
            restored = copy.deepcopy(candidate)
            for gi, group in enumerate(restored['answers']):
                for ai, alt in enumerate(group['alternatives']):
                    alt['symbol']['kind'] = original['answers'][gi]['alternatives'][ai]['symbol']['kind']
            assert restored == original
        counts['hypothetical_non_kind_field_changes'] = 0
        repo_counts[repo] = dict(counts)
        # Legal text only, no source/lineage/source-containing patches.
        legal_paths = subprocess.check_output(['git','ls-tree','-r','--name-only',author,PREFIX+repo+'/license'],cwd=REPO).decode().splitlines()
        for path in legal_paths:
            if Path(path).name not in ['LICENSE','NOTICE','AUTHORS','THIRD-PARTY-NOTICES.md','requests-introduction-LICENSE','requests-introduction-NOTICE','requests-introduction-AUTHORS']: continue
            raw = blob(author+':'+path)
            target = HERE/'retained-licenses'/repo/Path(path).relative_to(PREFIX+repo+'/license')
            target.parent.mkdir(parents=True,exist_ok=True); target.write_bytes(raw)
            licenses[str(target.relative_to(HERE))] = {'entry':author+':'+path,'sha256':sha(raw)}
    assert repo_counts['requests']['affected_alternatives'] == 85
    assert repo_counts['gin']['affected_alternatives'] == 81
    write('item-review.json', rows_out)
    write('input-manifest.json', input_hashes)
    write('license-retention-manifest.json', licenses)
    write('aggregate.json', dict(scope='public_DEV_annotation_review_no_rescoring', product=PRODUCT,
        admission_commit=ADMISSION, admission_sha256=sha(admission_raw), repos=repo_counts,
        classifications=dict(Counter(r['classification'] for r in rows_out)),
        confirmed_original_gold_errors=0, confirmed_parser_errors=0,
        normative_resolution_open=len(rows_out), applied_kind_changes=0,
        compat_projection_changed_rows=0, count_or_component_changes=0,
        original_raw_or_score_writes=0, default_selector_changes=0, live_provider_calls=0))
    write('versioned-proposal.json', dict(version='public-dev-declaration-kind-proposal-v1',
        status='needs_taxonomy_owner_review_not_admitted', previous_admission_sha256=sha(admission_raw),
        normative_rule='Class-owned Python definitions (including static/class/property accessors) and Go receiver declarations use method; free/nested-in-function definitions use function. This describes declarations, not runtime callable types.',
        unresolved='Original preregistration supplies exact matching but no declaration-versus-callable kind definition. Requests author and source owner explicitly preserve broad function. Gin original owner acceptance does not specify that distinction.',
        conditional_item_proposals='item-review.json', confirmed_error_patch_items=[],
        potential_alternative_kind_changes=166, applied_changes=0,
        forbidden_changes=['query','qname','span','primary','facets','difficulty','thresholds','parser','scorer','ranking','default_selector'],
        adoption_requires=['explicit taxonomy-owner decision','new independently reviewed candidate manifest/admission version','retain original admission and raw scores'],
        compatibility='Kind-only proposals leave frozen path-only compat projections, denominator, group counts and correlation components identical. No scores computed.'))
    print(json.dumps({'repos':repo_counts,'classifications':dict(Counter(r['classification'] for r in rows_out)), 'applied_changes':0}))

if __name__ == '__main__': main()
