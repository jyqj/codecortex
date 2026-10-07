"""Exact source union and immutable evidence guard; no production overlays."""
import hashlib,json,subprocess
from pathlib import Path
BASE='37dd042eaa1209a86e0cafdcd92ae77e036e76f5'
PRODUCT='e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696'
SOURCES=['866cbed73f303463a80cee07462824d1816f2a44','a65c655f7822bc8d025d6c01b1aa86371bbd9de9','54b2b92cfca035ef1de2b4d6478f314231fd5511']
def git(*a):return subprocess.check_output(['git',*a])
expected={}
for rev in SOURCES:
 assert subprocess.run(['git','merge-base','--is-ancestor',rev,PRODUCT]).returncode==0
 for p in git('diff','--name-only',BASE,rev,'--','crates').decode().splitlines():
  assert p not in expected,('unexpected overlapping source',p)
  expected[p]=rev
actual=git('diff','--name-only',BASE,PRODUCT,'--','crates').decode().splitlines()
review_paths={}
for rev in ['8d5282acbc0ff630b37429c258968bb07090ba65','bc0d0a25','26becaaa5287a12c63c12c0aaac3f4b71f0a7e03','56fd54e2','537618bae7084f08ecdcf80b18dbbcbcc2735c7b']:
 for p in git('diff-tree','--no-commit-id','--name-only','-r',rev).decode().splitlines():review_paths[p]=rev
extras=[p for p in actual if p not in expected]
assert set(extras)=={p for p in review_paths if p.startswith('crates/')}|{'crates/cc-model/tests/provenance_compatibility.rs'}
for p,rev in (expected|review_paths).items():
 assert git('show',PRODUCT+':'+p)==git('show',rev+':'+p),(p,rev)
 original=git('show',PRODUCT+':'+p)
 if p == 'crates/cc-search/src/engine_lane_tests.rs':
  exception=json.loads(Path(__file__).with_name('test-only-lint-fix').joinpath('transformation.json').read_text())
  old=(exception['old_expression']+'\n').encode();new=(exception['new_expression']+'\n').encode()
  preserved=Path(exception['original_evidence']).read_bytes()
  assert preserved==original and original.count(old)==1,('original test bytes/unique expression',p)
  assert hashlib.sha256(original).hexdigest()==exception['original_sha256']
  assert Path(p).read_bytes()==original.replace(old,new),('test-only exception exceeded',p)
  assert hashlib.sha256(Path(p).read_bytes()).hexdigest()==exception['transformed_sha256']
 else:
  assert Path(p).read_bytes()==original,('working source/evidence drift',p)
assert Path('crates/cc-model/tests/provenance_compatibility.rs').read_bytes()==git('show','bc0d0a25:crates/cc-index/tests/provenance_review_support/prototype_compatibility.rs')
for rev in SOURCES:
 for p in git('diff','--name-only',BASE,rev,'--','artifacts').decode().splitlines():
  assert Path(p).read_bytes()==git('show',rev+':'+p),('author evidence drift',p)
for p in ['Cargo.lock','Cargo.toml']:
 assert Path(p).read_bytes()==git('show',BASE+':'+p),(p,'changed')
# Prove production identity/wire, qname, UID, gold, scorer, ranking/budget paths
# remain base by proving all changes equal this exact reviewed source union.
production=[p for p in actual if '/src/' in p and not p.endswith('_tests.rs')]
references=git('grep','-n','declaration_identity',PRODUCT,'--',':(glob)crates/*/src/**').decode().splitlines()
assert len(references)==1 and references[0].endswith('pub mod declaration_identity;'),references
report=dict(base=BASE,product=PRODUCT,exact_source_union=True,source_history_preserved=True,review_evidence_unchanged=True,original_author_controls_unchanged=True,lock_unchanged=True,identity_unwired=True,production_paths=production,source_files={p:dict(source=rev,sha256=hashlib.sha256(Path(p).read_bytes()).hexdigest()) for p,rev in expected.items()},review_imports=review_paths)
report['test_only_lint_exception']=exception
report['final_tree_head']=git('rev-parse','HEAD').decode().strip()
# The only changed crate path since the product anchor is the authorized test file.
assert git('diff','--name-only',PRODUCT,'--','crates').decode().splitlines()==[exception['path']]
Path(__file__).with_name('lint-fix-source-guard.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:v for k,v in report.items() if k not in ['source_files','review_imports']},indent=2))
