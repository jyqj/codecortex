"""Two counterbalanced pairs per size, finite sequential local FakeProvider AB."""
from pathlib import Path
import hashlib,json,os,platform,resource,subprocess,time
repo=Path(__file__).resolve().parents[3];out=Path(__file__).resolve().parent
binary=repo/'target/release/capability-snapshot-ab'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
assert len(json.loads((out/'checks.json').read_text()))==11
assert all(x['exit_code']==0 for x in json.loads((out/'checks.json').read_text()))
source=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()
paths=['crates/cc-server/src/capability_status.rs','crates/cc-db/src/capability_read.rs','crates/cc-db/src/freshness_store.rs','crates/cc-db/src/lib.rs','driver/src/main.rs','driver/src/baseline_status.rs','driver/Cargo.toml','driver/Cargo.lock']
manifest={p:sha((repo/p) if p.startswith('crates/') else (out/p)) for p in paths}
protocol={'source_sha':source,'binary_sha256':sha(binary),'source_files_sha256':manifest,
  'compiler':subprocess.check_output(['/workspace/.cargo/bin/rustc','--version'],env={**os.environ,'RUSTUP_HOME':'/workspace/.rustup'},text=True).strip(),
  'platform':platform.platform(),'profile':'release opt-level=3 thin LTO codegen-units=1; same binary/mode switch',
  'baseline':'exact final6db4d396e5d994388ada1e97c3d28947ffdb9c81 capability_status.rs, all other production source shared with candidate; freshness extraction preserves old SQL/transaction',
  'protocol':{'sizes':[1000,5000],'pairs_per_size':2,'orders':['baseline,candidate','candidate,baseline'],
    'poll_gap_ms':200,'deadline_seconds':120,'dimensions':2,'pool_size':1,'batch':16,'steady_status_calls':128,
    'generator':'file_{i:05}.rs: pub fn resource_{i:05}() -> u32 { i } newline',
    'cache':'independent cold tempfile per process','provider':'real SemanticRuntime with local Gated FakeProvider, no HTTP',
    'polling':'same initial-release/serial call/sleep200ms logic in both arms; actual start-to-start intervals include status service time',
    'level':'in-process actual production status and worker; not MCP transport/100k/network provider'},
  'generator_sha256':{str(n):hashlib.sha256(b''.join(f'pub fn resource_{i:05}() -> u32 {{ {i} }}\n'.encode() for i in range(n))).hexdigest() for n in [1000,5000]},
  'real_provider_calls':0,'kill_fault_gc_wal':'not run','heldout_read':False}
(out/'AB-PROTOCOL.json').write_text(json.dumps(protocol,indent=2)+'\n')
rows=[]
for n in [1000,5000]:
    for pair,order in enumerate([['baseline','candidate'],['candidate','baseline']],1):
        for mode in order:
            name=f'{n}-pair{pair}-{mode}';case=out/'ab'/name
            case.mkdir(parents=True,exist_ok=True)
            print('START '+name,flush=True);start=time.monotonic();before=resource.getrusage(resource.RUSAGE_CHILDREN)
            with (case/'stdout.log').open('wb') as stdout,(case/'stderr.log').open('wb') as stderr:
                result=subprocess.run([str(binary),mode,str(n),str(case)],cwd=repo,stdout=stdout,stderr=stderr)
            after=resource.getrusage(resource.RUSAGE_CHILDREN)
            row={'case':name,'files':n,'pair':pair,'mode':mode,'exit_code':result.returncode,'wall_seconds':time.monotonic()-start,
                 'child_cpu_seconds':after.ru_utime+after.ru_stime-before.ru_utime-before.ru_stime}
            if (case/'receipt.json').exists():
                receipt=json.loads((case/'receipt.json').read_text());row.update({k:receipt[k] for k in ['ready','ready_ms','polls','latest_errors','status_total_ms','fake_inputs']})
            rows.append(row);(out/'AB-RUNS.json').write_text(json.dumps(rows,indent=2)+'\n')
            print(json.dumps(row),flush=True)
            if result.returncode:raise SystemExit(result.returncode)
assert sha(binary)==protocol['binary_sha256']
for p,h in manifest.items():assert sha((repo/p) if p.startswith('crates/') else(out/p))==h,p
print('FINITE AB COMPLETE; SOURCES/BINARY UNCHANGED',flush=True)
