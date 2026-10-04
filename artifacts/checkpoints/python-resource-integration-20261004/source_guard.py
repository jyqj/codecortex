"""Exact bounded delta, immutable evidence, and explicit test migration guard."""
import hashlib, json, pathlib, subprocess
ROOT=pathlib.Path(__file__).resolve().parent
BASE='84d5d57978cfaa3a0dd7f63e39d7c7288b614d95'
IMPORTS=[('08733fa26504e9792586513ebe81075f058b586e','120af8f'),('a9050fa7123a671dc0d9e2c1b61533cfbee8612d','1302ea3'),('c5091acf45fcd001f17e9822555e1db503ec7ed9','53d1803'),('6ed481e22960244a78c2706e4866a71f2a6278a4','b832e23'),('7d49beb6c220b6992d8e8e6001342f7a2fa213f6','7b4904f'),('c2b2a22e0536e163d1f9f7353f10ddf400dfd854','003abc1'),('16d1344a72f749f995670c995c537330f2d9403a','3f435e6')]
def git(*args): return subprocess.check_output(['git',*args])
def sha(b): return hashlib.sha256(b).hexdigest()
expected={}; imports=[]
for source, copied in IMPORTS:
 paths=git('diff-tree','--no-commit-id','--name-only','-r',source).decode().splitlines()
 for p in paths:
  original=git('show',source+':'+p)
  assert original==git('show',copied+':'+p),(p,source,copied)
  expected[p]=source
 imports.append(dict(source=source,integrated=git('rev-parse',copied).decode().strip(),paths=paths))
migration=json.loads((ROOT/'migration.json').read_text())
p=migration['path']; old=git('show',migration['historical_integrated_commit']+':'+p)
assert old==git('show',migration['historical_source_commit']+':'+p)==(ROOT/'python_identity_independent.rs.original').read_bytes()
assert sha(old)==migration['original_sha256']
assert sha(pathlib.Path(p).read_bytes())==migration['current_sha256']
assert git('diff',migration['historical_integrated_commit'],'--',p)==(ROOT/'r1-migration.patch').read_bytes()
for p, source in expected.items():
 if p==migration['path']: continue
 assert pathlib.Path(p).read_bytes()==git('show',source+':'+p),('import drift',p)
# All pre-existing paths outside the accepted imports and documented TODO change
# remain byte-identical to integration head. This includes every old freeze/log.
new_test='crates/cc-index/tests/python_identity_resource_integration.rs'
assert pathlib.Path(new_test).read_bytes()==git('show','50a4933e48ef20b16401ac8f75c386a660aa8e5c:'+new_test)
exceptions=set(expected)|{new_test,'docs/roadmap/code-index-v2/05-TODO.md'}
assert not git('diff','--name-only','--diff-filter=D',BASE).strip(), 'unexpected deletion'
for p in git('diff','--name-only','--diff-filter=M',BASE).decode().splitlines():
 if p in exceptions: continue
 assert pathlib.Path(p).read_bytes()==git('show',BASE+':'+p),('pre-existing drift',p)
production=git('diff','--name-only',BASE,'--',':(glob)crates/*/src/**').decode().splitlines()
assert set(production)=={'crates/cc-model/src/declaration_identity.rs','crates/cc-parsers/src/python_identity.rs','crates/cc-parsers/src/lib.rs'},production
assert pathlib.Path('crates/cc-parsers/src/lib.rs').read_bytes()==git('show','7d49beb:crates/cc-parsers/src/lib.rs')
refs=subprocess.check_output(['rg','-n','declaration_identity|python_identity','crates','-g','*.rs','-g','!**/tests/**','-g','!declaration_identity.rs','-g','!python_identity.rs']).decode().splitlines()
assert {(line.split(':',2)[0],line.split(':',2)[2]) for line in refs}=={('crates/cc-model/src/lib.rs','pub mod declaration_identity;'),('crates/cc-parsers/src/lib.rs','pub mod python_identity;')}, refs
report=dict(integration_base=BASE,production_anchor='e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696',imports=imports,exact_imports_except_recorded_r1_migration=True,immutable_old_evidence=True,existing_vec_to_array_fix_preserved=True,production_paths=production,identity_unwired=True,lock_and_query_and_extraction_unchanged=True,migration=migration)
(ROOT/'source-guard.json').write_text(json.dumps(report,indent=2)+'\n')
print('PASS: exact imported sources/evidence; old tree preserved; explicit R1 migration; identity unwired')
