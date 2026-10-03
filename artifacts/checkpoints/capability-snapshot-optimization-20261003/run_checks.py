"""Bounded current-source checks, excluding heldout and kill/GC/WAL fault suites."""
from pathlib import Path
import os,subprocess,json,time,sys
repo=Path(__file__).resolve().parents[3]
out=Path(__file__).resolve().parent
cargo='/workspace/.cargo/bin/cargo'
env=os.environ.copy();env.update(RUSTUP_HOME='/workspace/.rustup',CARGO_HOME='/workspace/.cargo',CARGO_BUILD_JOBS='4')
cases=[
('v2-format',[cargo,'fmt','--all','--','--check']),
('v2-db-tests',[cargo,'test','-p','cc-db','--lib','capability_read','--locked','--offline']),
('v2-original-coverage-tests',[cargo,'test','-p','cc-db','--test','semantic_coverage','--locked','--offline']),
('v2-default-status-tests',[cargo,'test','-p','cc-server','--lib','capability_status::tests','--locked','--offline']),
('v2-production-status-tests',[cargo,'test','-p','cc-server','--features','semantic-http','--lib','capability_status::tests','--locked','--offline']),
('v2-current-oracles',[cargo,'test','-p','cc-server','--features','semantic-http','--test','p7_status_snapshot_independent_review','--locked','--offline','--','--nocapture']),
('v2-worker-failure-priority',[cargo,'test','-p','cc-server','--features','semantic-http','--lib','semantic_runtime::tests::lazy_factory_failure_is_visible_and_retired_state_cannot_override_replacement','--locked','--offline']),
('v2-public-parameters',[cargo,'test','-p','cc-server','--features','semantic-http','--test','p7_v18_parameter_contract','--locked','--offline']),
('v2-public-query-fence',[cargo,'test','-p','cc-server','--features','semantic-http','--test','p7_v11_ready_epoch_independent_review','--locked','--offline']),
('v2-db-clippy',[cargo,'clippy','-p','cc-db','--lib','--locked','--offline','--','-D','warnings']),
('v2-server-clippy',[cargo,'clippy','-p','cc-server','--features','semantic-http','--lib','--test','p7_status_snapshot_independent_review','--test','p7_v18_parameter_contract','--locked','--offline','--','-D','warnings']),
]
start_at=int(sys.argv[1]) if len(sys.argv)>1 else 0
rows=json.loads((out/'checks.json').read_text())[:start_at] if start_at else []
for name,argv in cases[start_at:]:
    print(name,flush=True);start=time.monotonic()
    with (out/(name+'.log')).open('wb') as f:
        result=subprocess.run(argv,cwd=repo,env=env,stdout=f,stderr=subprocess.STDOUT)
    rows.append({'name':name,'argv':argv,'exit_code':result.returncode,'seconds':time.monotonic()-start,'log':name+'.log'})
    (out/'checks.json').write_text(json.dumps(rows,indent=2)+'\n')
    if result.returncode:
        print('FAILED '+name,flush=True);raise SystemExit(result.returncode)
print('ALL CHECKS PASSED',flush=True)
