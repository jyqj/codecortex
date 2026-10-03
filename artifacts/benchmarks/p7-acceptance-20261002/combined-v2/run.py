#!/usr/bin/env python3
"""Record exact local/offline acceptance commands; never call live providers."""
import hashlib,json,os,pathlib,re,subprocess
ROOT=next(p for p in pathlib.Path(__file__).resolve().parents if (p/'Cargo.toml').is_file())
OUT=pathlib.Path(__file__).resolve().parent
ENV=dict(os.environ,CARGO_HOME='/workspace/.cargo',RUSTUP_HOME='/workspace/.rustup',CARGO_BUILD_JOBS='5')
ENV['PATH']='/workspace/.cargo/bin:'+ENV['PATH']
TEST=ROOT/'crates/cc-server/tests/p7_acceptance_matrix.rs'
def capture(argv):return subprocess.check_output(argv,cwd=ROOT,env=ENV,text=True).strip()
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()
receipt={'source_commit_sha':capture(['git','rev-parse','HEAD']),'integration_base_sha':'d6462c163abe6ca92d2ae0a2cf967b1ad2433e22','source_tree_sha':capture(['git','rev-parse','HEAD^{tree}']),'base_branch':'codex/cloud-p7-integration','test_source_sha256':digest(TEST),'toolchain':capture(['rustc','--version']),'dirty':capture(['git','status','--short']),'commands':[],'repeats':[],'boundary':'L1/L2 only; synthetic vectors and in-memory HTTP seam, zero sockets; not live, stdio, holdout, release performance or real costs'}
commands=[
 ('matrix-semantic',['cargo','test','-p','cc-server','--features','semantic','--test','p7_acceptance_matrix','--locked','--offline']),
 ('matrix-default',['cargo','test','-p','cc-server','--test','p7_acceptance_matrix','--locked','--offline']),
 ('search-lib',['cargo','test','-p','cc-search','--lib','--locked','--offline']),
 ('vector-exact',['cargo','test','-p','cc-semantic','--lib','vector::exact::tests','--locked','--offline']),
 ('lock-span',['cargo','test','-p','cc-eval','--test','p5b_execution','slow_fake_does_not_hold_codeindex_lock_or_the_only_sql_connection','--locked','--offline']),
 ('new-test-only-clippy',['cargo','clippy','--no-deps','-p','cc-server','--features','semantic','--test','p7_acceptance_matrix','--locked','--offline','--','-D','warnings']),
 ('fake-provider',['cargo','test','-p','cc-semantic','--lib','providers::fake','--locked','--offline']),
 ('partial-budget',['cargo','test','-p','cc-eval','--test','p7_partial_budget','--locked','--offline']),
 ('partial-budget-semantic',['cargo','test','-p','cc-eval','--features','cc-server/semantic','--test','p7_partial_budget','--locked','--offline']),
 ('fusion',['cargo','test','-p','cc-search','--lib','fusion::tests','--locked','--offline']),
 ('hydration',['cargo','test','-p','cc-search','--lib','semantic_hydrate_guard::tests','--locked','--offline']),
 ('execution',['cargo','test','-p','cc-search','--lib','execution::tests','--locked','--offline']),
 ('scope',['cargo','test','-p','cc-server','--features','semantic','--lib','semantic_scope_guard::tests','--locked','--offline']),
 ('wiring',['cargo','test','-p','cc-server','--features','semantic','--lib','semantic_wiring::tests','--locked','--offline']),
 ('exact-manifest',['cargo','test','-p','cc-semantic','--test','manifest_exact_integration','--locked','--offline']),
 ('query-cache',['cargo','test','-p','cc-semantic','--test','query_cache','--locked','--offline']),
 ('provider',['cargo','test','-p','cc-semantic','--lib','providers::openai_compatible','--locked','--offline']),
 ('admission',['cargo','test','-p','cc-semantic','--lib','admission::','--locked','--offline']),
 ('new-test-clippy',['cargo','clippy','-p','cc-server','--features','semantic','--test','p7_acceptance_matrix','--locked','--offline','--','-D','warnings']),
 ('new-test-default-clippy',['cargo','clippy','-p','cc-server','--test','p7_acceptance_matrix','--locked','--offline','--','-D','warnings']),
 ('new-test-format',['rustfmt','--check','--edition','2021',str(TEST.relative_to(ROOT))]),
]
for name,argv in commands:
 log=OUT/(name+'.log');assert not log.exists(),str(log)
 p=subprocess.run(argv,cwd=ROOT,env=ENV,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
 log.write_text(p.stdout)
 counts=[list(map(int,m)) for m in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',p.stdout)]
 item={'name':name,'argv':argv,'exit':p.returncode,'log':log.name,'log_sha256':digest(log),'counts':counts}
 receipt['commands'].append(item);print(name,p.returncode,counts,flush=True)
 if name.startswith('matrix-') and p.returncode==0:
  binary=re.search(r'Running tests/p7_acceptance_matrix.rs \(([^)]+)\)',p.stdout).group(1)
  results=[]
  for iteration in range(20):
   run=subprocess.run([str(ROOT/binary)],cwd=ROOT,env=ENV,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
   raw=OUT/f'{name}-repeat-{iteration+1:02}.log';raw.write_text(run.stdout)
   results.append({'iteration':iteration+1,'exit':run.returncode,'log':raw.name,'sha256':digest(raw)})
   if run.returncode:print(run.stdout,flush=True);break
  receipt['repeats'].append({'variant':name,'binary':binary,'binary_sha256':digest(ROOT/binary),'results':results})
 (OUT/'receipt.json').write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+'\n')
