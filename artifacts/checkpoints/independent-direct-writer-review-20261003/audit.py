"""Check executed evidence and fixed-source identity; no test-result inference."""
import collections, hashlib, json, pathlib, sqlite3, subprocess, tomllib
ROOT=pathlib.Path(__file__).resolve().parent
REPO=ROOT.parents[2]
TOOL=pathlib.Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def git(*argv):return subprocess.check_output(['git','-C',str(REPO),*argv])
entries=[json.loads(s) for s in (ROOT/'commands.jsonl').read_text().splitlines()]
for row in entries:
    assert sha(ROOT/row['log'])==row['log_sha256'],row['log']
def last(label):return next(x for x in reversed(entries) if x['label']==label)
cases=['canonical','helpers','synthetic','invalid','nul','failures']
final=[last('candidate-'+c) for c in cases]
driver_hash=sha(ROOT/'driver/src/main.rs')
for x in final:
    assert x['exit_code']==0,x
    assert x['driver_sha256']==driver_hash,x
    assert 'PASS ' in (ROOT/x['log']).read_text(),x
binary_hash=sha(ROOT/'local/binaries/candidate')
assert all(x['binary_sha256']==binary_hash for x in final)
source=json.loads((ROOT/'candidate-source.json').read_text())
assert source['built_writer_sha256']==source['original_sha256']['crates/cc-db/src/direct_writer.rs']
assert all(x['writer_sha256']==source['built_writer_sha256'] for x in final)
for f,h in source['original_sha256'].items():
    if f=='Cargo.toml':continue # explicitly recorded members-only adaptation
    assert sha(ROOT/'local/source'/f)==h,f
negative=[]
for variant,case,needle in [
    ('base','canonical','schema tables: no such column: new.rowid'),
    ('mutant-no-triggers','canonical','callback schema BEFORE any reopen'),
    ('mutant-split','helpers','!drop.contains("forged")'),
    ('mutant-no-nul','nul','NUL silently accepted: ()')
]:
    build=last(variant+'-build');run=last(variant+'-'+case)
    assert build['exit_code']==0
    assert run['exit_code']==101
    assert run['driver_sha256']==driver_hash
    assert sha(ROOT/'local/binaries'/variant)==run['binary_sha256']
    assert needle in (ROOT/run['log']).read_text(),variant
    negative.append(dict(variant=variant,run=run,detected=needle))
# Driver lock retains every selected registry package/version/checksum from fixed input.
upstream=tomllib.loads(git('show',source['commit']+':Cargo.lock').decode())
lock=tomllib.loads((ROOT/'driver/Cargo.lock').read_text())
identity=lambda p:(p['name'],p['version'],p.get('source'),p.get('checksum'))
upstream_registry={identity(p) for p in upstream['package'] if 'source' in p}
selected_registry={identity(p) for p in lock['package'] if 'source' in p}
assert selected_registry<=upstream_registry
assert git('diff','--name-only').decode()=='' # original checkout untouched
changed=git('diff','--name-only','ee4c4fc0b41e298bf38b8269310053fa4b355c61',source['commit']).decode().splitlines()
assert [p for p in changed if p.startswith('crates/') and not p.endswith('_tests.rs')]==['crates/cc-db/src/direct_writer.rs']
canonical_path=pathlib.Path(final[0]['argv'][2])/'standalone-canonical.sqlite3'
c=sqlite3.connect('file:'+str(canonical_path)+'?mode=ro',uri=True)
catalog=c.execute('SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name').fetchall()
assert len(catalog)==162
count=collections.Counter(x[0] for x in catalog)
explicit=sum(1 for t,_,_,sql in catalog if t=='index' and sql is not None)
automatic=sum(1 for t,_,_,sql in catalog if t=='index' and sql is None)
(ROOT/'canonical-catalog.json').write_text(json.dumps(catalog,ensure_ascii=False,indent=2)+'\n')
checks=[]
for label,argv in [
    ('driver-format',[str(TOOL/'rustfmt'),'--edition','2021','--check',str(ROOT/'driver/src/main.rs')]),
    ('candidate-writer-format',[str(TOOL/'rustfmt'),'--edition','2021','--check',str(ROOT/'local/source/crates/cc-db/src/direct_writer.rs')]),
    ('toolchain-rustc',[str(TOOL/'rustc'),'-Vv']),('toolchain-cargo',[str(TOOL/'cargo'),'-V'])
]:
    r=subprocess.run(argv,capture_output=True,text=True)
    p=ROOT/(label+'.log');p.write_text(r.stdout+r.stderr)
    assert r.returncode==0,(argv,r.stdout,r.stderr)
    checks.append(dict(argv=argv,exit_code=r.returncode,log=p.name,sha256=sha(p)))
failed_driver=[]
for e in entries:
    if e['label'].startswith('candidate-') and e['exit_code']:
        copies=[p.name for p in ROOT.glob('driver-*-failed.rs.txt') if sha(p)==e['driver_sha256']]
        assert copies,e
        failed_driver.append(dict(log=e['log'],source=copies,exit_code=e['exit_code']))
result=dict(outcome='boundedpass',candidate=source['commit'],base='ee4c4fc0b41e298bf38b8269310053fa4b355c61',
            driver_sha256=driver_hash,binary_sha256=binary_hash,driver_lock_sha256=sha(ROOT/'driver/Cargo.lock'),
            original_source=source,selected_registry_packages=len(selected_registry),registry_versions_match_fixed_lock=True,
            candidate_case_groups=6,positive_runs=final,negative_controls=negative,checks=checks,
            catalog=dict(total=len(catalog),types=dict(count),explicit_indexes=explicit,automatic_indexes=automatic),
            failed_driver_attempts=failed_driver,
            compiler_binary_sha256=sha(TOOL/'rustc'),cargo_binary_sha256=sha(TOOL/'cargo'),
            runtime_sqlite_version='3.53.2',catalog_export_reader_sqlite_version=sqlite3.sqlite_version,
            original_checkout_head=git('rev-parse','HEAD').decode().strip(),original_tracked_diff_empty=True,
            not_run=['FIFO','GC/WAL kill/crash/fault','general swap/crash guarantees','100k/performance',
                     'real provider','DEV/heldout','full workspace/remote CI'])
(ROOT/'review-receipt.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print(json.dumps({k:result[k] for k in ['outcome','candidate_case_groups','catalog','binary_sha256','driver_sha256','failed_driver_attempts']},ensure_ascii=False))
