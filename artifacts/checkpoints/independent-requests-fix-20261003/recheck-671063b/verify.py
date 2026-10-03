import ast, hashlib, json, pathlib, sqlite3, subprocess
ROOT = pathlib.Path(__file__).resolve().parent
REPO = ROOT.parents[3]
LABELS = ('old','rejected','repaired')
def read(p): return json.loads((ROOT/p).read_text())
receipt = read('build-execution-receipt.json')
manifest = read('source-manifest.json')
controls = {k:read(f'{k}-controls/results.json') for k in LABELS}
legal = {k:read(f'{k}-legal-identifiers/legal-identifiers.json') for k in LABELS}
expected_shas = {'old':'b25723458a77edd2207ed1a52dceb0ea1009cb93',
                 'rejected':'da5b05ee08d84fd336d5a98f25da7cae176daebf',
                 'repaired':'671063b11af8cb40a0d526098de82e684dd24aca'}
for k in LABELS:
    r = receipt['versions'][k]
    assert r['source_sha'] == expected_shas[k] == manifest[k]['sha']
    for path,digest in manifest[k]['files'].items():
        assert hashlib.sha256((ROOT/'snapshots'/k/path).read_bytes()).hexdigest()==digest
    helper = (ROOT/f'snapshots/{k}/crates/cc-index/src/resolver/helpers.rs').read_text()
    start = helper.index('pub(in crate::resolver) fn type_atoms(')
    end = helper.find('\n#[cfg(test)]',start)
    expected_atoms = helper[start:end if end!=-1 else len(helper)].replace('pub(in crate::resolver)','pub')
    assert (ROOT/f'harness-{k}/src/atoms.rs').read_text()==expected_atoms
    assert r['target_absent_before_build'] and r['artifact_fresh'] is False
    assert r['build_exit_code'] == 0 and all(x['exit_code']==0 for x in r['runs'])
    binary = pathlib.Path(r['executable'])
    assert binary.resolve().is_relative_to(ROOT/'targets'/k)
    assert hashlib.sha256(binary.read_bytes()).hexdigest()==r['binary_sha256']
    assert hashlib.sha256((ROOT/f'harness-{k}/src/main.rs').read_bytes()).hexdigest()==r['harness_sha256']
    assert (ROOT/f'harness-{k}/src/main.rs').read_bytes()==(ROOT/'harness.rs').read_bytes()
    artifacts = [json.loads(x) for x in (ROOT/f'build-{k}.jsonl').read_text().splitlines()]
    for crate in ('cc-index','cc-model','cc-db','cc-parsers'):
        a = next(x for x in artifacts if x.get('reason')=='compiler-artifact' and x['target']['name']==crate.replace('-','_'))
        assert a['fresh'] is False
        assert pathlib.Path(a['manifest_path']).resolve()==ROOT/f'snapshots/{k}/crates/{crate}/Cargo.toml'
        assert all(pathlib.Path(p).resolve().is_relative_to(ROOT/'targets'/k) for p in a['filenames'])
    assert 'invalid dependency key' in controls[k]['strict_validator_error']
    for f in controls[k]['fixtures']:
        result = f['result']
        if k=='old' and f['name'] in ('old-negative','named-variadic'):
            assert 'invalid dependency key' in result['error']
            continue
        assert 'error' not in result
        assert result['report']['files_parsed']==result['report']['files_scanned']==1
        assert not result['report']['parse_errors'] and result['empty_dependencies']==0
        assert len(result['validated_manifests'])==1
        if k!='old':
            assert not any(d['key'] in ('','...') for m in result['validated_manifests'] for d in m['dependencies'])
    if k!='old':
        assert all(not x['actual'] for x in controls[k]['punctuation'])
        assert all('' not in x['keys'] for x in controls[k]['name_keys'])
        assert all(x['passed'] for x in controls[k]['atom_controls'])
rows=[]
for triplet in zip(*(legal[k] for k in LABELS)):
    a,b,c = triplet
    assert a['name']==b['name']==c['name']
    regression = a['name'] not in ('python-unicode-continuation','python-combining-continuation')
    expected_counts = [1,0 if regression else 1,1]
    assert [x['uses_type_edges'] for x in triplet]==expected_counts
    # The retained harness observation compares literal spelling. Name buckets
    # intentionally lowercase; independently inspect canonical persisted keys.
    canonical_key = c['type_name'].lower()
    canonical_dependency = any(d['kind']=='name_bucket' and d['key']==canonical_key
                               for m in c['result']['validated_manifests'] for d in m['dependencies'])
    assert c['atoms']==[c['type_name']] and canonical_dependency
    if regression and a['name']!='typescript-dollar': assert not b['name_dependency']
    for k,case in zip(LABELS,triplet):
        assert not case['result']['report']['parse_errors']
        if case['name'].startswith('python'):
            ast.parse((ROOT/f'{k}-legal-identifiers'/case['name']/'sample.py').read_text())
            assert case['type_name'].isidentifier()
        if k=='repaired':
            db=sqlite3.connect(ROOT/f'{k}-legal-identifiers'/f"{case['name']}.sqlite")
            assert db.execute("SELECT count(*) FROM semantic_edges WHERE relation_kind='uses_type' AND target_symbol=? AND target_symbol_uid IS NOT NULL",(case['type_name'],)).fetchone()[0]==1
            db.close()
    rows.append({'name':a['name'],'type_name':a['type_name'],'uses_type_edges':dict(zip(LABELS,expected_counts)),'repaired_name_dependency':canonical_dependency,'canonical_name_key':canonical_key})
def blob(sha,path): return subprocess.check_output(['git','-C',str(REPO),'rev-parse',f'{sha}:{path}']).decode().strip()
unchanged={}
for p in ('crates/cc-model/src/resolution.rs','crates/cc-db/src/resolution_dependency_store.rs'):
    before=blob(expected_shas['rejected'],p); after=blob(expected_shas['repaired'],p)
    assert before==after
    unchanged[p]=after
verdict={'outcome':'BOUNDED_PASS','reviewed_sha':expected_shas['repaired'],
         'prior_reject_sha':expected_shas['rejected'],'old_negative_sha':expected_shas['old'],
         'identifier_replay':rows,'validator_and_persistence_blobs_unchanged':unchanged,
         'build_identity':'three absent independent targets; exact cargo artifacts; binary/source/hash checks pass',
         'original_requests_corpus':'not_rechecked_owned_by_integration',
         'full_cache_migration':'not_run_owned_by_integration',
         'author_test_counts_used_as_evidence':False}
(ROOT/'verdict.json').write_text(json.dumps(verdict,ensure_ascii=False,indent=2))
print('BOUNDED_PASS: original negatives, seven legal identifier cases, strict validator and three-version build identity verified.')
