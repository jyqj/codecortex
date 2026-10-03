from pathlib import Path
import hashlib,json,re,subprocess
R=Path(__file__).resolve().parent; S=R/'source'; H=R/'harness'
# Never rerun prepare here: it would replace the tested files and Cargo.lock.
ident=json.loads((R/'source-identity.json').read_text())
for row in ident['crate_files']:
 orig=(S/row['path']).read_bytes();copy=(H/row['path']).read_bytes()
 assert hashlib.sha256(orig).hexdigest()==row['sha256']
 assert hashlib.sha256(copy).hexdigest()==row['harness_sha256']
assert not subprocess.check_output(['git','status','--porcelain'],cwd=S)
mutants=json.loads((R/'mutants.json').read_text());assert len(mutants)==3 and all(x['killed'] for x in mutants)
restored=json.loads((R/'restored-runs.json').read_text());assert all(x['exit_code']==0 for x in restored)
regular=json.loads((R/'independent-runs.json').read_text());assert all(x['exit_code']==0 for x in regular)
query=(R/'query-fence.log').read_text();assert 'test result: ok. 1 passed' in query;assert query.count('index changed during retrieval after 1 attempts; retry the query')>=2
raw={}
for label in ['independent-http','restored-independent-http']:
 s=(R/(label+'.log')).read_text();m=re.search(r'NORMAL_PUBLISH (\{[^\n]+\})',s)
 assert m;raw[label]=json.loads(m.group(1));assert raw[label]['attempts']==1
(R/'normal-publish-observations.json').write_text(json.dumps(raw,indent=2)+'\n')
verdict={'verdict':'bounded_pass','production':'11af963c33cfa68cc9497e355464c1d6d058adac','final':'29b03a0fec6bfde92aac9c41dce996ebb2d08cc7','base':'6db4d396e5d994388ada1e97c3d28947ffdb9c81','contract':'approved retrieval-capabilities-v2 point_in_time; complete old/new observation valid; mixed old root/new coverage invalid','independent_unique_tests':7,'independent_positive_matrix_executions':9,'positive_matrix':'DB 3; default server 2; semantic-http server 4 (2 overlap); restore 7 executions not extra unique credit','actual_compiled_mutants_killed':3,'supplemental_unmodified_production_tests':{'db':6,'strict_query_stdio':1,'query_tools_crossed':['search','context'],'independent_unique_credit':0},'source_identity':'419 crate files verified; exact complete file prefix plus appended harness tests; historical author prefix guard verified EOF-only','platform':'Linux x86_64 bundled SQLite Unix VFS only','ci_gap':'default and semantic product current production compiled; migrated cfg semantic source oracle does not run in default workspace matrix; no explicit semantic-http CI command','production_or_ledger_writes':False,'author_checks_or_AB_independent_credit':0,'no_100k_run':True,'heldout_read_or_eval':False,'GC_WAL_kill_fault':False,'limits':['Finite synthetic operations only; not all concurrency schedules.','Identity is checked at observation boundaries, not response serialization/delivery.','No macOS/Windows/custom-VFS success certification.','No worker-throughput, general latency or 100k acceptance claim.'],'harness_failures_retained':['first SQL fixture pending/claimed doc key violated live unique constraint','first server helper supplied &IndexDb instead of Arc<IndexDb>','first no-active fixture assumed assembly had inserted a space row; fixed by ordinary explicit synthetic INSERT'],'auto_review':'empty process schema probe rejected for unconstrained execution; no empty request retried; no review work remains blocked by this rejection','remote_delivery':'pending separate connector branch commit/draft PR and verification'}
(R/'verdict.json').write_text(json.dumps(verdict,ensure_ascii=False,indent=2)+'\n')
print('bounded pass; source restored and clean; all independent final checks passed; 3 compiled mutants killed')
