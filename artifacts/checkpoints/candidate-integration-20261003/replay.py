#!/usr/bin/env python3
"""Limited authorized fixtures only. Never selects the broad runtime group."""
import hashlib, json, os, pathlib, re, subprocess, sys
ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = pathlib.Path(sys.argv[1]).resolve() if len(sys.argv)>1 else pathlib.Path(__file__).resolve().parent / 'verified'
OUT.mkdir(parents=True, exist_ok=True)
TOOL = pathlib.Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
CACHE = OUT / 'owned-cache'
ENV = dict(os.environ, PATH=f'{TOOL}:' + os.environ['PATH'], CARGO_HOME='/workspace/.cargo', RUSTC=str(TOOL/'rustc'), RUSTDOC=str(TOOL/'rustdoc'), CARGO_TARGET_DIR=str(ROOT/'target/candidate-integration'), CODECORTEX_SEMANTIC_CACHE_ROOT=str(CACHE))
receipts=[]
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def command(name,args, test=False):
    result=subprocess.run(list(map(str,args)), cwd=ROOT, env=ENV, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    text=result.stdout.decode(errors='replace')
    (OUT/f'{name}.log').write_text(text)
    ran=(not test or any(int(n)>0 for n in re.findall(r'test result: ok\. (\d+) passed;', text)))
    receipts.append(dict(name=name,argv=list(map(str,args)),exit_code=result.returncode, executed_nonzero_tests=ran if test else None, log=f'{name}.log'))
    (OUT/'receipts.json').write_text(json.dumps(receipts,indent=2)+'\n')
    if result.returncode or not ran: raise SystemExit(f'{name} failed: retained in {OUT}')
    return text
source_paths = sorted([p for p in ROOT.glob('crates/**/*.rs') if 'target' not in p.parts] + [ROOT/'Cargo.lock',ROOT/'Cargo.toml',ROOT/'.github/workflows/ci.yml'])
identity=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(), dirty=subprocess.check_output(['git','status','--short'],cwd=ROOT,text=True), source_sha256={str(p.relative_to(ROOT)):sha(p) for p in source_paths}, toolchain=command('rustc-version',[TOOL/'rustc','-Vv']), binaries={}, env={k:ENV[k] for k in ['CARGO_HOME','RUSTC','RUSTDOC','CARGO_TARGET_DIR','CODECORTEX_SEMANTIC_CACHE_ROOT']})
(OUT/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
def build(name,args):
    text=command('build-'+name,[TOOL/'cargo','test',*args,'--locked','--offline','--no-run','--message-format=json'])
    bins={}
    for line in text.splitlines():
        try: msg=json.loads(line)
        except ValueError: continue
        if msg.get('reason')=='compiler-artifact' and msg.get('executable') and msg['profile']['test']:
            path=pathlib.Path(msg['executable']); bins[msg['target']['name']]=path
            identity['binaries'][str(path)]={'sha256':sha(path),'target':msg['target']['name'],'features':msg['features'],'package_id':msg['package_id']}
    (OUT/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
    return bins
server=build('server',['-p','cc-server','--features','semantic-http','--lib'])['cc_server']
for name in ['shared_provider_gate_tests::shared_provider_gate_cases_in_fresh_processes', 'tests::recoverable_retry_drains_other_ready_documents','tests::recoverable_retry_width_zero_drains_other_ready_documents','tests::bounded_parallel_pins_running_and_factory_survive_until_physical_join']:
    command(name.split('::')[-1],[server,'--exact','semantic_runtime::'+name,'--nocapture'],True)
semantic=build('queues',['-p','cc-semantic','--test','bounded_parallel','--test','queue_worker'])
for name,path in semantic.items(): command(name,[path,'--nocapture'],True)
db=build('db',['-p','cc-db','--lib','--test','semantic_outbox'])
command('fifo',[db['semantic_outbox'],'--nocapture'],True)
for name in ['direct_rebuild_canonical_schema_matches_normal_temp_path','direct_rebuild_callback_and_index_errors_leave_live_database_unchanged']:
    command(name,[db['cc_db'],'--exact','index_db::tests::'+name,'--nocapture'],True)
command('strict-clippy',[TOOL/'cargo-clippy','clippy','-p','cc-semantic','-p','cc-server','--all-targets','--features','semantic-http','--locked','--offline','--','-D','warnings'])
command('fmt',[TOOL/'rustfmt','--edition','2021','--check',*[ROOT/p for p in ['crates/cc-semantic/src/admission.rs','crates/cc-semantic/src/queue.rs','crates/cc-semantic/tests/bounded_parallel.rs','crates/cc-server/src/service_factory.rs','crates/cc-server/src/semantic_wiring.rs','crates/cc-server/src/semantic_runtime.rs','crates/cc-server/src/shared_provider_gate_tests.rs']]])
command('diff-check',['git','diff','--check'])
identity['binaries_unchanged_after_tests']=all(sha(pathlib.Path(p))==v['sha256'] for p,v in identity['binaries'].items())
assert identity['binaries_unchanged_after_tests']
(OUT/'identity.json').write_text(json.dumps(identity,indent=2)+'\n')
print(f'Limited checks completed; receipts and source/binary identities: {OUT}')
