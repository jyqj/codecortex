#!/usr/bin/env python3
"""Offline, byte-exact gold overlay replay. No product output or evaluator inputs."""
import argparse
import contextlib
import copy
import hashlib
import io
import json
from pathlib import Path
import subprocess
import types

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BASE = '6c1416109003bcff0c1911307a4af5bd48870517'
REVIEW = HERE.parent / 'public-dev-kind-review-20261003'
VERSION = 'public-dev-declaration-kind-v2-candidate'
STATUS = 'not_admitted'
DECODER = json.JSONDecoder()
GENERATED = ['change-manifest.json', 'candidate-gold.json', 'aggregate.json',
             'input-manifest.json', 'license-retention-manifest.json']

def require(condition, label):
    if not condition:
        raise ValueError(label)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=False)+'\n').encode()

def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':')).encode()

def blob(entry):
    return subprocess.check_output(['git', 'show', entry], cwd=REPO)

def load_review():
    """Verify the entire fixed review before importing its standalone oracle."""
    m = json.loads((REVIEW/'artifact-manifest.json').read_bytes())
    require((REVIEW/'artifact-manifest.json').read_bytes() ==
            blob(BASE+':'+str((REVIEW/'artifact-manifest.json').relative_to(REPO))), 'review manifest pin')
    for p, receipt in m['files'].items():
        raw = (REVIEW/p).read_bytes()
        require(sha(raw) == receipt['sha256'] and len(raw) == receipt['bytes'], 'review artifact pin')
        require(raw == blob(BASE+':'+str((REVIEW/p).relative_to(REPO))), 'review base pin')
    # Execute only verified standalone oracle bytes; do not emit a pyc in REVIEW.
    audit = types.ModuleType('frozen_kind_review')
    audit.__file__ = str(REVIEW/'review.py')
    exec(compile((REVIEW/'review.py').read_bytes(), audit.__file__, 'exec'), audit.__dict__)
    return audit

def replay_review(audit):
    """Recheck all 268 alternatives, source hashes, owners and controls in scratch."""
    out = HERE/'.scratch/review-replay'
    out.mkdir(parents=True, exist_ok=True)
    audit.HERE = out
    helper = HERE/'.scratch/go-taxonomy'
    audit.go_definitions = lambda body: json.loads(subprocess.check_output(
        [str(helper)], input=json.dumps(body.decode()).encode()))
    with contextlib.redirect_stdout(io.StringIO()):
        audit.main()
    for p in ['item-review.json', 'input-manifest.json', 'aggregate.json',
              'versioned-proposal.json', 'license-retention-manifest.json']:
        require((out/p).read_bytes() == (REVIEW/p).read_bytes(), 'source review replay drift')
    for p, receipt in json.loads((REVIEW/'license-retention-manifest.json').read_bytes()).items():
        require((out/p).read_bytes() == blob(receipt['entry']) == (REVIEW/p).read_bytes(), 'legal replay drift')

def spaces(text, pos):
    while pos < len(text) and text[pos].isspace():
        pos += 1
    return pos

def fields(text, offset=0):
    """Exact field ranges in character offsets; duplicate JSON keys are rejected."""
    pos = spaces(text, 0)
    require(text[pos] == '{', 'object expected')
    pos += 1
    result = {}
    while True:
        pos = spaces(text, pos)
        if text[pos] == '}':
            return result
        key, pos = DECODER.raw_decode(text, pos)
        require(key not in result, 'duplicate field')
        pos = spaces(text, pos)
        require(text[pos] == ':', 'colon expected')
        start = spaces(text, pos+1)
        _, end = DECODER.raw_decode(text, start)
        result[key] = (offset+start, offset+end)
        pos = spaces(text, end)
        require(text[pos] in ',}', 'object delimiter expected')
        if text[pos] == ',':
            pos += 1

def elements(text, offset=0):
    pos = spaces(text, 0)
    require(text[pos] == '[', 'array expected')
    pos += 1
    result = []
    while True:
        start = spaces(text, pos)
        if text[start] == ']':
            return result
        _, end = DECODER.raw_decode(text, start)
        result.append((offset+start, offset+end))
        pos = spaces(text, end)
        require(text[pos] in ',]', 'array delimiter expected')
        if text[pos] == ',':
            pos += 1

def field(text, interval, key):
    start, end = interval
    return fields(text[start:end], start)[key]

def array(text, interval):
    start, end = interval
    return elements(text[start:end], start)

def alt_ranges(text, gi, ai):
    answers = fields(text)['answers']
    group = array(text, answers)[gi]
    alt = array(text, field(text, group, 'alternatives'))[ai]
    symbol = field(text, alt, 'symbol')
    return answers, alt, field(text, symbol, 'kind')

def locator(row):
    return tuple(row[k] for k in ['repo', 'query_ordinal', 'group_ordinal', 'alternative_ordinal'])

def check_proposals(proposals, expected):
    """Require the exact fixed item set and every original binding, not counts alone."""
    expected_map = {locator(r):r for r in expected}
    require(len(expected_map) == len(expected), 'expected duplicate')
    actual = {}
    for r in proposals:
        key = locator(r)
        require(key not in actual, 'duplicate proposal')
        require(key in expected_map, 'extra proposal')
        require(r == expected_map[key], 'proposal binding drift')
        actual[key] = r
    require(set(actual) == set(expected_map), 'missing proposal')

def apply_line(line, patches):
    """Only exact, nonoverlapping literal function-to-method replacements."""
    text = line.decode()
    positions = []
    for p in patches:
        gi, ai = p['group_ordinal'], p['alternative_ordinal']
        _, alt, kind = alt_ranges(text, gi, ai)
        require(sha(text[alt[0]:alt[1]].encode()) == p['original_alternative_raw_sha256'], 'alternative binding')
        start, end = kind
        require(text[start:end] == '"function"', 'false correction')
        require(json.loads(text)['answers'][gi]['alternatives'][ai]['symbol']['kind'] == 'function', 'kind binding')
        positions.append((start, end))
    require(len(set(positions)) == len(positions), 'duplicate delta')
    ordered = sorted(positions)
    require(all(a[1] <= b[0] for a, b in zip(ordered, ordered[1:])), 'overlapping delta')
    result = text
    for start, end in reversed(ordered):
        result = result[:start]+'"method"'+result[end:]
    original, candidate = json.loads(text), json.loads(result)
    restored = copy.deepcopy(candidate)
    for p in patches:
        gi, ai = p['group_ordinal'], p['alternative_ordinal']
        restored['answers'][gi]['alternatives'][ai]['symbol']['kind'] = 'function'
    require(restored == original, 'non-kind structural drift')
    # Restore exact original tokens at shifted character positions as a byte proof.
    restored_text = result
    for n in range(len(ordered)-1, -1, -1):
        start, _ = ordered[n]
        shifted = start - 2*n
        require(restored_text[shifted:shifted+8] == '"method"', 'delta placement drift')
        restored_text = restored_text[:shifted]+'"function"'+restored_text[shifted+8:]
    require(restored_text.encode() == line, 'non-kind byte drift')
    return result.encode()

def check_candidate_outputs(actual, expected):
    require(set(actual) == set(expected), 'output set drift')
    for name in expected:
        require(actual[name] == expected[name], 'candidate output drift: '+name)

def build(audit, proposals):
    expected = json.loads((REVIEW/'item-review.json').read_bytes())
    check_proposals(proposals, expected)
    proposal = json.loads((REVIEW/'versioned-proposal.json').read_bytes())
    inputs = json.loads((REVIEW/'input-manifest.json').read_bytes())
    for entry, digest in inputs.items():
        require(sha(blob(entry)) == digest, 'input binding')
    admission = json.loads(blob(audit.ADMISSION+':'+audit.ADMISSION_PATH))
    changes, overlay, row_hashes, files = [], [], [], {}
    for repo in ['requests', 'gin']:
        receipt = admission['repo_results'][repo]
        suite = json.loads(blob(receipt['native_dev_suite_entry']))
        entry = receipt['author_sha']+':'+audit.PREFIX+repo+'/'+suite['queries']
        original = blob(entry)
        require(sha(original) == inputs[entry] == admission['inputs_sha256'][entry], 'native pin')
        lines = original.splitlines(keepends=True)
        revised, offset = [], 0
        for qi, line in enumerate(lines):
            selected = [p for p in proposals if p['repo'] == repo and p['query_ordinal'] == qi]
            selected.sort(key=lambda p:(p['group_ordinal'],p['alternative_ordinal']))
            after = apply_line(line, selected)
            require(audit.project(json.loads(line)) == audit.project(json.loads(after)), 'compat projection drift')
            text, candidate = line.decode(), after.decode()
            for p in selected:
                require(sha(line) == p['original_query_line_raw_sha256'], 'raw row binding')
                require(sha(audit.raw_field(text, 'query').encode()) == p['original_query_text_json_token_sha256'], 'query token drift')
                gi, ai = p['group_ordinal'], p['alternative_ordinal']
                answers, alt, kind = alt_ranges(text, gi, ai)
                after_answers, after_alt, _ = alt_ranges(candidate, gi, ai)
                require(sha(text[answers[0]:answers[1]].encode()) == p['original_answers_raw_sha256'], 'answers pin')
                raw_alt_after = candidate[after_alt[0]:after_alt[1]]
                byte_start, byte_end = (len(text[:x].encode()) for x in kind)
                patch = dict(pointer=f'/answers/{gi}/alternatives/{ai}/symbol/kind',
                    line_byte_start=byte_start, line_byte_end=byte_end,
                    file_byte_start=offset+byte_start, file_byte_end=offset+byte_end,
                    before_raw_token='"function"', after_raw_token='"method"',
                    before_raw_token_sha256=sha(b'"function"'), after_raw_token_sha256=sha(b'"method"'),
                    after_alternative_raw_sha256=sha(raw_alt_after.encode()))
                source_binding = {k:p[k] for k in ['source_entry','source_sha256','sourcecoords',
                    'source_fragment_sha256','declarationcoords','source_map_entry',
                    'source_map_record_sha256','owner_reviews']}
                changes.append(dict(binding=p, candidate_kind='method', delta=patch,
                    overlay_applied_in_candidate=True, source_binding_before=source_binding,
                    source_binding_after=copy.deepcopy(source_binding),
                    before_gold_source_record_raw_sha256=p['original_gold_record_raw_sha256'],
                    after_gold_source_record_raw_sha256=p['original_gold_record_raw_sha256']))
                overlay.append(dict(repo=repo, query_ordinal=qi, group_ordinal=gi, alternative_ordinal=ai,
                    query_id_sha256=p['query_id_sha256'],
                    before_alternative_raw_sha256=p['original_alternative_raw_sha256'],
                    after_alternative_raw_sha256=patch['after_alternative_raw_sha256'],
                    alternative_json_token=raw_alt_after))
            if selected:
                before_answers = audit.raw_field(text, 'answers').encode()
                after_answers = audit.raw_field(candidate, 'answers').encode()
                row_hashes.append(dict(repo=repo, query_ordinal=qi, changes=len(selected),
                    before_line_raw_sha256=sha(line), after_line_raw_sha256=sha(after),
                    before_answers_raw_sha256=sha(before_answers), after_answers_raw_sha256=sha(after_answers),
                    query_raw_token_sha256=sha(audit.raw_field(text, 'query').encode()),
                    line_ending_preserved=True, non_kind_bytes_identical=True))
            revised.append(after)
            offset += len(line)
        derived = b''.join(revised)
        require(len(original)-len(derived) == 2*sum(p['repo'] == repo for p in proposals), 'file delta drift')
        files[repo] = dict(entry=entry, before_raw_sha256=sha(original), after_raw_sha256=sha(derived),
            before_bytes=len(original), after_bytes=len(derived), rows=len(lines),
            derived_in_memory_only=True, compat_projection_exact=True,
            source_map_entries_unchanged=sorted(set(p['source_map_entry'] for p in proposals if p['repo'] == repo)))
    require(len(changes) == 166 and len(row_hashes) == 101, 'candidate scope')
    manifest = dict(schema_version=1, version=VERSION, status=STATUS, base_commit=BASE,
        admission_commit=audit.ADMISSION, previous_admission_sha256=sha(blob(audit.ADMISSION+':'+audit.ADMISSION_PATH)),
        normative_rule=proposal['normative_rule'], taxonomy_owner_decision='Explicit delegated owner decision; applies to this new candidate only.',
        prior_review_item_sha256=sha((REVIEW/'item-review.json').read_bytes()),
        historical_confirmed_error_patch_items=[], changes=changes, rows=row_hashes, files=files,
        remaining_gate='Independent root review of every source/semantic binding, non-kind bytes, source notices and exact compat projection before separate admission.',
        unchanged_other_repo_taxonomy=['express', 'typescript'], scoring_permitted=False)
    aggregate = dict(version=VERSION, status=STATUS, base_commit=BASE, changes=166,
        changes_by_repo={'requests':85,'gin':81}, affected_rows_by_repo={'requests':58,'gin':43},
        native_rows_by_repo={'requests':91,'gin':67}, all_alternatives_source_bound=268,
        exact_non_kind_bytes_preserved=True, exact_compat_projections_preserved=True,
        unchanged_source_maps=True, retained_legal_files=9,
        historical_native_rows=301, historical_compat_rows=sum(r['compat_dev_rows'] for r in admission['repo_results'].values()),
        historical_global_correlation_components=admission['global_review']['conservative_global_correlation_components'],
        new_independent_samples=0, newly_admitted_rows=0, confirmed_historical_kind_errors=0,
        new_retrieval_runs=0, new_scores=0, live_provider_calls=0, persistent_changes_outside_owned_directory=0,
        default_selector_changes=0, product_main_chain_changes=0, four_repo_score_merge=False,
        derived_native_files=files)
    gold = dict(schema_version=1, version=VERSION, status=STATUS,
        representation='Minimal replacement alternatives; no query text or source corpus; apply only to hash-pinned native rows.',
        replacements=overlay)
    pins = dict(inputs)
    for name in ['item-review.json', 'versioned-proposal.json', 'artifact-manifest.json', 'review.py', 'go_taxonomy.go']:
        entry = BASE+':'+str((REVIEW/name).relative_to(REPO))
        pins[entry] = sha(blob(entry))
    legal = json.loads((REVIEW/'license-retention-manifest.json').read_bytes())
    outputs = {name:encoded(value) for name,value in zip(GENERATED,[manifest,gold,aggregate,pins,legal])}
    for p, receipt in legal.items():
        outputs[p] = blob(receipt['entry'])
        require(sha(outputs[p]) == receipt['sha256'], 'retained legal pin')
    return outputs

def seal():
    files = {}
    for p in sorted(HERE.rglob('*')):
        rel = p.relative_to(HERE)
        if not p.is_file() or rel.parts[0] in ['.scratch','__pycache__'] or p.name == 'artifact-manifest.json':
            continue
        raw = p.read_bytes()
        files[str(rel)] = dict(sha256=sha(raw),bytes=len(raw))
    (HERE/'artifact-manifest.json').write_bytes(encoded(dict(version=VERSION,status=STATUS,files=files)))

def verify_seal():
    m = json.loads((HERE/'artifact-manifest.json').read_bytes())
    actual = {str(p.relative_to(HERE)) for p in HERE.rglob('*') if p.is_file()
              and p.relative_to(HERE).parts[0] not in ['.scratch','__pycache__'] and p.name != 'artifact-manifest.json'}
    require(actual == set(m['files']), 'artifact set drift')
    for p, receipt in m['files'].items():
        raw = (HERE/p).read_bytes()
        require(sha(raw) == receipt['sha256'] and len(raw) == receipt['bytes'], 'artifact digest drift')
    require(m['version'] == VERSION and m['status'] == STATUS, 'admission state drift')

def check_ownership():
    paths = subprocess.check_output(['git','diff','--name-only',BASE],cwd=REPO).decode().splitlines()
    paths += subprocess.check_output(['git','ls-files','--others','--exclude-standard'],cwd=REPO).decode().splitlines()
    prefix = str(HERE.relative_to(REPO))+'/'
    require(all(p.startswith(prefix) for p in paths), 'changes outside owned directory')

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--write',action='store_true')
    parser.add_argument('--seal',action='store_true')
    args = parser.parse_args()
    if args.seal:
        seal()
        return
    audit = load_review()
    replay_review(audit)
    rows = json.loads((REVIEW/'item-review.json').read_bytes())
    outputs = build(audit, rows)
    if args.write:
        for p, raw in outputs.items():
            target = HERE/p
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
    else:
        check_candidate_outputs({p:(HERE/p).read_bytes() for p in outputs}, outputs)
        verify_seal()
    check_ownership()
    print(json.dumps(dict(version=VERSION,status=STATUS,changes=166,source_bound_alternatives=268,
        manifest_sha256=sha(outputs['change-manifest.json']), candidate_gold_sha256=sha(outputs['candidate-gold.json']),
        aggregate_sha256=sha(outputs['aggregate.json']), exact_replay=True, non_kind_byte_changes=0,
        compat_projection_changes=0, newly_admitted_rows=0, new_scores=0)))

if __name__ == '__main__':
    main()
