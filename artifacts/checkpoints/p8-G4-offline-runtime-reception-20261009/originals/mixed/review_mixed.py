"""Independent archive inspection and original Rust statistics replay; no product run."""
import collections, hashlib, json, os, re, sqlite3, subprocess, sys, time, zipfile
from pathlib import Path

ROOT=Path(__file__).resolve().parent
REPO=Path('/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source')
G='260f596582f2d82b8d7c707b61a6b8b6a43b069f'
sys.path.insert(0,str(REPO/'scripts'))
import p7_build_identity as identity
import p8_runtime as runtime
import p8_runtime_build as build

def load(p): return json.loads(p.read_text())
def sha(p): return identity.file_sha256(p)
def ensure(c,m):
    if not c: raise AssertionError(m)
def write(p,x): p.write_text(json.dumps(x,sort_keys=True,indent=2)+'\n')
def norm(x): return json.dumps(x,sort_keys=True,separators=(',',':'))

SOURCE=identity.source_snapshot(REPO)
OBSERVER=build.observer_snapshot(REPO)
ensure(SOURCE['source_commit']==G,'not exact G')
specs=load(ROOT/'expected-artifacts.json')
stats_hashes={load(ROOT/f"C{s['c']}/member-hashes.json")['members']['p8-build/p8-runtime-statistics']['sha256'] for s in specs}
ensure(len(stats_hashes)==1,'different statistics binaries')
binary=ROOT/'retained-statistics'
reports=[]
for s in sorted(specs,key=lambda x:x['c']):
    c=s['c'];home=ROOT/f'C{c}';orig=home/'originals';product=orig/'p8-build';run=orig/'p8-runtime'
    hashes=load(home/'member-hashes.json')['members']
    def member(name):return {k:v for k,v in hashes[name].items() if k in ('bytes','sha256')}
    for prefix in ('p8-build','p8-runtime'):
        seal=load(orig/prefix/'seal.json')
        actual={name[len(prefix)+1:]:member(name) for name in hashes if name.startswith(prefix+'/') and name!=prefix+'/seal.json'}
        ensure(seal['artifact_inventory']==actual,'complete raw seal differs')
    receipt=load(product/'build-receipt.json');plan=load(run/'plan.json');report=load(run/'report.json')
    ensure(receipt['source_before']==receipt['source_after']==SOURCE,'source before/after')
    ensure(receipt['observer_before']==receipt['observer_after']==OBSERVER,'observer before/after')
    ensure(receipt['status']=='passed' and receipt['build_exit_code']==0 and receipt['schema_version']==2,'build receipt')
    ensure(receipt['build_command']==build.command_for(Path(receipt['target_dir'])),'Cargo exact command')
    ensure(sha(product/'product-build.jsonl')==receipt['cargo_log_sha256'] and sha(product/'product-build.stderr')==receipt['stderr_sha256'],'Cargo logs')
    messages=[json.loads(line) for line in (product/'product-build.jsonl').read_text().splitlines()]
    ensure(messages[-1]=={'reason':'build-finished','success':True},'Cargo completion')
    artifact_evidence={}
    for name,(package,source,prefix,field) in build.TARGETS.items():
        rows=[x for x in messages if x.get('reason')=='compiler-artifact' and x.get('target',{}).get('name')==name and x.get('executable')]
        stored=receipt['artifacts'][name];a=stored['cargo_artifact'];ensure(rows==[a] and a==receipt[field],'unique original Cargo event')
        ensure(a['manifest_path']=='/home/runner/work/codecortex/codecortex/crates/'+package+'/Cargo.toml' and a['target']['src_path']=='/home/runner/work/codecortex/codecortex/crates/'+package+'/'+source,'exact original source path')
        ensure(a['executable']==receipt['target_dir']+'/release/'+name==stored['copy_source']['path'],'original target copy source')
        ensure(a['target']['kind']==['bin'] and a['features']==[] and a['profile']['opt_level']=='3' and a['profile']['debug_assertions'] is False and a['profile']['test'] is False,'actual release default profile')
        row=member('p8-build/'+name)
        ensure(row['sha256']==stored['binary_sha256']==stored['copy_source']['sha256']==receipt[prefix+'_sha256'] and row['bytes']==stored['binary_bytes']==stored['copy_source']['bytes'],'actual archived binary copy bytes')
        artifact_evidence[name]=row
    for rel,row in OBSERVER['files'].items():
        ensure(sha(product/'observer-source'/rel)==row['sha256'],'archived observer')
    for name,row in plan['retained_build_evidence'].items():
        ensure(member('p8-runtime/build-evidence/'+name)==row==member('p8-build/'+name),'original retained build proof copy')
    ensure(report['final_build_verification']==plan['build_identity'],'final receipt/source/observer recheck')
    ensure(plan['build_identity']['source']==SOURCE and plan['build_identity']['observer']==OBSERVER and plan['build_identity']['artifacts']==receipt['artifacts'],'initial exact identity')
    ensure(plan['build_identity']['build_seal_sha256']==sha(product/'seal.json'),'build seal binding')
    ensure(report['raw_sha256']==sha(run/'raw.jsonl') and report['plan_sha256']==sha(run/'plan.json'),'original raw/plan binding')
    ensure(plan['operations']==900 and plan['concurrency']==c and plan['files']==1000 and plan['profile']=='mixed' and plan['offer_interval_ms']==500,'fixed original plan')
    ensure(plan['queue_capacity']==128 and plan['request_timeout_seconds']==60,'original queue/timeouts')
    raw=[json.loads(line) for line in (run/'raw.jsonl').read_text().splitlines()]
    rows=[r for r in raw if r['kind']=='operation'];resources=[r for r in raw if r['kind']=='resources']
    ensure(len(rows)==900 and sorted(r['id'] for r in rows)==list(range(900)),'all offered IDs')
    outcomes=dict(collections.Counter(r['status'] for r in rows));ensure(outcomes==report['outcomes']=={'success':900},'full outcome denominator')
    origin=min(r['scheduled_ns'] for r in rows)
    for r in rows:
        ensure(r['scheduled_ns']==origin+(r['id']//c*c)*500_000_000 and r['offered_ns']>=r['scheduled_ns'],'fixed offered schedule')
        ensure(r['finished_ns']>=r['call_started_ns']>=r['started_ns']>=r['offered_ns'],'all timing stages')
        ensure(r['operation']==('build' if r['id']%3==0 else 'read'),'planned operation')
    ensure(collections.Counter(r['operation'] for r in rows)=={'read':600,'build':300},'primary N')
    mutations=sorted((r for r in rows if r['operation']=='build'),key=lambda x:x['mutation_ordinal'])
    ensure([r['mutation_ordinal'] for r in mutations]==list(range(300)),'complete serialized mutation admissions')
    actions=collections.Counter(r['mutation']['action'] for r in mutations)
    ensure(len(actions)==6 and set(actions.values())=={50},'six action strata N50')
    ensure(report['actual_concurrency']['maximum']<=c and (c==1 or report['actual_concurrency']['read_build_overlap']),'actual overlap')
    ensure(runtime.latency_summary(rows)==report['latency'],'all-attempt latency')
    ensure(runtime.rss_trend(resources)==report['rss'],'original native RSS rule')
    ensure(runtime.sample_coverage(resources,origin,report['resource_time_coverage']['work_end_ns'])==report['resource_time_coverage'],'original resource interval coverage')
    process=load(run/'product/process.json');full_process=load(run/'full-product/process.json')
    for proc in (process,full_process):ensure(proc['exit_code']==0 and proc['cleanup']=='completed' and proc['tool_count']==14 and proc['binary_sha256']==receipt['binary_sha256'],'actual product exit/identity')
    ensure(all(r['server']['pid']==process['pid'] and r['runner']['pid']!=process['pid'] and r['server']['resident_bytes']>0 for r in resources),'native server vs runner attribution')
    for key in ('user_cpu_ns','system_cpu_ns'):
        values=[r['server'][key] for r in resources];ensure(values==sorted(values),'native CPU monotonic')
    for key in ('rchar','wchar','read_bytes','write_bytes','read_syscalls','write_syscalls'):
        values=[r['server']['io'][key] for r in resources];ensure(values==sorted(values),'native I/O monotonic')
    rpc=[json.loads(line) for line in (run/'product/rpc.jsonl').read_text().splitlines()]
    requests=[r['payload'] for r in rpc if r['event']=='request'];responses=[r['payload'] for r in rpc if r['event']=='response']
    ensure(len({r['id'] for r in requests})==len(requests) and len({r['id'] for r in responses})==len(responses),'unique stdio IDs')
    ensure({r['id'] for r in requests}=={r['id'] for r in responses},'no missing terminal responses')
    index=[r for r in requests if r['method']=='tools/call' and r['params']['name']=='index']
    ensure(len(index)==301 and index[0]['params']['arguments']['full'] is True and all(r['params']['arguments']['full'] is False for r in index[1:]),'incremental side not repaired at endpoint')
    pending={};peak=0;overlap=False
    for r in rpc:
        if r['event']=='request':
            p=r['payload'];name=p.get('params',{}).get('name')
            if p['method']=='tools/call' and name in ('index','search'):
                pending[p['id']]=name;peak=max(peak,len(pending));overlap|={'index','search'}<=set(pending.values())
        elif r['event']=='response':pending.pop(r['payload']['id'],None)
    ensure(not pending and (c==1 or overlap),'wire-level actual overlap')
    parity=load(run/'parity.json');tables=parity['comparison']['tables']
    ensure(len(parity['tables'])==15 and [t['table'] for t in tables]==parity['tables'] and parity['exit_code']==0 and parity['error'] is None and parity['comparison']['equal'] is True and not parity['comparison']['different_tables'],'full original fifteen table oracle')
    ensure(all(t['equal'] and t['different_row_count']==0 and t['full_rows']==t['incremental_rows'] and t['full_digest']==t['incremental_digest'] for t in tables),'every oracle table equal')
    ensure(sha(run/'parity.json')==report['parity_sha256'],'parity original bytes')
    oracle_event=[r for r in raw if r['kind']=='oracle_process'];ensure(len(oracle_event)==1 and oracle_event[0]['exit_code']==0,'actual original oracle execution')
    # The copied final authored fixture and Git state must be byte-for-byte the same.
    def authored(prefix):return {name[len(prefix):]:member(name) for name in hashes if name.startswith(prefix) and not name[len(prefix):].startswith('.codecortex/')}
    ensure(authored('p8-runtime/project/')==authored('p8-runtime/fresh-full/'),'endpoint source/Git copy differed')
    stats=load(run/'statistics.json');ensure((run/'statistics.json').read_bytes()==(run/'statistics-replay.json').read_bytes(),'original two statistics outputs')
    ensure(stats['expected_samples']==stats['recorded_samples']==900 and stats['missing_samples']==stats['unexpected_samples']==0 and stats['configured_concurrency']==c,'statistics denominator')
    ensure({x['operation']:x['recorded_samples'] for x in stats['by_operation']}=={'read':600,'build':300} and all(x['recorded_samples']==50 for x in stats['by_build_mutation']),'unpooled strata')
    ensure(stats['plan_sha256']==sha(run/'plan.json') and stats['raw_sha256']==sha(run/'raw.jsonl') and stats['replay_binary_sha256']==receipt['statistics_sha256'],'stats exact inputs and binary')
    free=os.statvfs(ROOT).f_bavail*os.statvfs(ROOT).f_frsize
    if not binary.exists():
        with zipfile.ZipFile(ROOT/f"C{s['c']}-artifact-{s['id']}.zip") as z: data=z.read('p8-build/p8-runtime-statistics')
        ensure(hashlib.sha256(data).hexdigest()==receipt['statistics_sha256'] and len(data)==receipt['artifacts']['p8-runtime-statistics']['binary_bytes'],'retained stats bytes')
        with binary.open('xb') as f:f.write(data)
        binary.chmod(0o555)
    ensure(sha(binary)==receipt['statistics_sha256'],'same original replay executable')
    replay=home/'independent-statistics-replay';replay.mkdir()
    command=[str(binary),'--plan',str(run/'plan.json'),'--raw',str(run/'raw.jsonl'),'--output',str(replay/'statistics.json')]
    record=dict(argv=command,source_commit=G,free_bytes_before=free,input_sha256={'plan':sha(run/'plan.json'),'raw':sha(run/'raw.jsonl')},binary_sha256=sha(binary))
    write(replay/'command.json',record);start=time.monotonic()
    with (replay/'stdout.log').open('xb') as stdout,(replay/'stderr.log').open('xb') as stderr:completed=subprocess.run(command,stdout=stdout,stderr=stderr,timeout=120)
    record.update(exit_code=completed.returncode,wall_seconds=time.monotonic()-start,byte_identical=(replay/'statistics.json').read_bytes()==(run/'statistics.json').read_bytes(),output_sha256=sha(replay/'statistics.json'))
    write(replay/'result.json',record);ensure(record['exit_code']==0 and record['byte_identical'],'actual Rust statistics replay differs')
    ensure(record['input_sha256']=={'plan':sha(run/'plan.json'),'raw':sha(run/'raw.jsonl')} and sha(binary)==record['binary_sha256'],'replay changed inputs/executable')
    summary=dict(configured_concurrency=c,offered=900,outcomes=outcomes,primary_N={'read':600,'build':300},mutation_N=dict(actions),observed_driver_peak=report['actual_concurrency']['maximum'],wire_peak=peak,read_build_overlap=overlap,resource_samples=len(resources),server_pid=process['pid'],runner_pid=resources[0]['runner']['pid'],native_RSS_positive=len(resources),cpu_and_io_monotonic=True,process_tree_measurement='unavailable; no aggregate tree claim',original_oracle_tables={t['table']:t['full_rows'] for t in tables},incremental_index_calls=300,initial_full_index_calls=1,repair_index_calls=0,branch_switches=report['real_branch_switches'],catalog_compactions=report['observed_catalog_compactions'],source=SOURCE['source_commit'],artifacts=artifact_evidence,statistics_replay=record)
    write(home/'independent-review.json',summary);reports.append(summary)
ensure(identity.source_snapshot(REPO)==SOURCE and build.observer_snapshot(REPO)==OBSERVER,'source/observer changed during audit')
for s in specs:
    home=ROOT/f"C{s['c']}";hashes=load(home/'member-hashes.json')['members']
    ensure(all(sha(home/'originals'/n)==row['sha256'] for n,row in hashes.items() if row['extracted']),'raw originals changed')
write(ROOT/'independent-review.json',dict(status='accepted_scoped_mixed_raw',source_commit=G,cells=reports,limits=['C identifies configured offered concurrency; observed peaks are reported exactly, not relabeled as C.','Each read group N600 and build group N300 is separate by C; each mutation action N50 remains separate. No pooling or new performance threshold.','Existing IID confidence intervals are descriptive; mixed mutations and serial correlation do not certify homogeneous tails.','This is mixed read/build only, not one-hour soak or enabled semantic worker throughput.','Current native RSS, CPU and I/O belong to server self; runner getrusage is separate; process tree aggregate remains unavailable.','No product, oracle executable or Cargo invoked by the reviewer. Only the retained source-bound statistics binary was replayed on original plan/raw.'],TODO_closed=0))
print(json.dumps({'status':'accepted_scoped_mixed_raw','report_sha256':sha(ROOT/'independent-review.json'),'cells':[{k:r[k] for k in ('configured_concurrency','offered','outcomes','observed_driver_peak','wire_peak','read_build_overlap','resource_samples')} for r in reports]},indent=2))
