#!/usr/bin/env python3
"""Read tar members as bytes only; no SQLite connection, extraction or native run."""
import datetime, gzip, hashlib, json, stat, tarfile
from pathlib import Path, PurePosixPath

OUT=Path(__file__).resolve().parent
ROOT=OUT.parent.parent
INPUT=ROOT/'integration-validation/round12-M6-original-archive'
HEAD='e95fd750d2f5052d0308c970cd5032c48b765cde'
def sha(data):return hashlib.sha256(data).hexdigest()
def digest(path):
    h=hashlib.sha256()
    with path.open('rb') as f:
        for b in iter(lambda:f.read(1048576),b''):h.update(b)
    return h.hexdigest()
def fi(path):return {'path':str(path),'bytes':path.stat().st_size,'sha256':digest(path)}
def jb(v):return (json.dumps(v,ensure_ascii=False,sort_keys=True,indent=2)+'\n').encode()
checks=[]
def check(name,value,detail=None):
    checks.append({'name':name,'passed':bool(value),'detail':detail})
    assert value,name
prefix=ROOT/'runtime-review/round11-m6-soak-prefix/raw-complete-line-prefix.jsonl'
prefix_size=prefix.stat().st_size;prefix_digest=digest(prefix)
earlier=ROOT/'runtime-review/round12-m6-session-loss/raw-observed-prefix.jsonl.gz'
earlier_h=hashlib.sha256();earlier_bytes=0
with gzip.open(earlier,'rb') as f:
    for b in iter(lambda:f.read(1048576),b''):earlier_h.update(b);earlier_bytes+=len(b)
index=json.loads((INPUT/'archive-index.json').read_bytes())
check('original ledger and unresolved hour scope',index['original_todos_completed']==0 and index['remaining_todos']==29 and index['task_ledger_changed'] is False and index['hour_status']=='unknown_session_unavailable_no_terminal')
packages={};contents={}
for name,n,wanted_bytes,wanted_sha in [
    ('m6-completed-and-initial-failures',16488,219581310,'b90c8764bd3cd38b38285991639638ad8c2216516b7aeca818c535855dcb27e1'),
    ('m6-unresolved-hour-observation',2058,182008029,'20e0e0940831ac419fd2130dbe37133bb660efc1d635d03a3b256ae623ff6c03')]:
    manifest_path=INPUT/(name+'.json');m=json.loads(manifest_path.read_bytes());path=INPUT/(name+'.tar.xz')
    before=fi(path);s=path.stat();before_stat=(s.st_size,s.st_mtime_ns,s.st_ino)
    check(name+'/exact archive hash and size',before['bytes']==m['archive']['bytes'] and before['sha256']==m['archive']['sha256']==wanted_sha and m['archive'] in index['archives'])
    check(name+'/source and bounded scope',m['source_commit']==HEAD and m['file_count']==n and m['uncompressed_file_bytes']==wanted_bytes and m['native_execution'] is False and m['sqlite_connections_opened']==0 and m['original_receipts_modified'] is False and m['target_caches_included'] is False and m['status']=='preserved_exact_observed_bytes')
    actual={};retained={};special=[]
    with tarfile.open(path,'r|xz') as tar:
        for item in tar:
            if not item.isfile() or item.name in actual or item.name.startswith('/') or '\\' in item.name or str(PurePosixPath(item.name))!=item.name or any(p in ('','.','..') for p in item.name.split('/')):
                raise ValueError('unsafe/duplicate/nonregular member '+item.name)
            expected=m['files'][item.name]
            if item.size!=expected['bytes'] or item.mode!=expected['mode']:raise ValueError('size/mode mismatch '+item.name)
            stream=tar.extractfile(item);h=hashlib.sha256();read=0;israw=item.name=='soak/runtime/raw.jsonl';ph=hashlib.sha256()
            keep=(item.name in ['execution-receipt.json','runtime-parent-recovery/receipt.json','runtime-parent-recovery/original-execution-observation.json','build/build-receipt.json','backfill/receipt.json'] or item.name.endswith('/seal.json') or item.name in [c+'/runtime/report.json' for c in ['c1','c4','c8','c16']] or item.name.endswith('-run.log') or item.name.endswith('-verify.log')) and item.size<=1000000
            buf=[]
            while True:
                block=stream.read(1048576)
                if not block:break
                if read+len(block)>item.size:raise ValueError('expanded bytes beyond member size')
                h.update(block)
                if israw and read<prefix_size:ph.update(block[:prefix_size-read])
                if keep:buf.append(block)
                read+=len(block)
            if read!=item.size or h.hexdigest()!=expected['sha256']:raise ValueError('member hash mismatch '+item.name)
            actual[item.name]={'bytes':read,'sha256':h.hexdigest()}
            if keep:retained[item.name]=b''.join(buf)
            if item.name.endswith(('.sqlite3','-wal','-shm')):special.append({'path':item.name,**actual[item.name]})
            if israw:
                check('unknown hour exact prior 29202421-byte observation preserved',read==earlier_bytes and h.hexdigest()==earlier_h.hexdigest())
                check('known resource-gap prefix preserved byte-for-byte',read>=prefix_size and ph.hexdigest()==prefix_digest)
    check(name+'/all members and full inventory',len(actual)==n and sum(v['bytes'] for v in actual.values())==wanted_bytes and actual=={k:{'bytes':v['bytes'],'sha256':v['sha256']} for k,v in m['files'].items()})
    check(name+'/no source checkout or target caches',not any(p.split('/')[0] in ('source','cargo-home','runtime-target','backfill-target') for p in actual))
    check(name+'/unchanged archive during review',before==fi(path) and before_stat==(path.stat().st_size,path.stat().st_mtime_ns,path.stat().st_ino))
    packages[name]={'archive':before,'manifest':fi(manifest_path),'file_count':len(actual),'file_bytes':sum(v['bytes'] for v in actual.values()),'database_related_bytes_only':special,
       'producer_before_after_observation':{'reported_equal':m['source_before_after_equal'],'scope':'Producer historical observation from frozen manifest/script; reviewer did not re-open unknown live original directory or claim global quiescence.'},'member_inventory':actual}
    contents[name]=retained

c=packages['m6-completed-and-initial-failures']['member_inventory'];h=packages['m6-unresolved-hour-observation']['member_inventory'];ct=contents['m6-completed-and-initial-failures'];ht=contents['m6-unresolved-hour-observation']
for prefix in ['build/','backfill/','c1/runtime/','c4/runtime/','c8/runtime/','c16/runtime/']:
    seal=json.loads(ct[prefix+'seal.json'])
    actual={k[len(prefix):]:v for k,v in c.items() if k.startswith(prefix) and k!=prefix+'seal.json'}
    check('original sealed output completely preserved '+prefix,actual==seal['artifact_inventory'],{'files':len(actual)})
initial=json.loads(ct['execution-receipt.json']);recovery=json.loads(ct['runtime-parent-recovery/receipt.json'])
check('initial failure is never rewritten to all-six pass',initial['status']=='failed' and initial['cell_outcomes']=={'c1':False,'c4':False,'c8':False,'c16':False,'soak':False,'backfill':True} and sum(p.get('exit_code')==2 for p in initial['phases'])==10)
check('initial source stable e95',initial['source_before']==initial['source_after'] and initial['source_before']['commit']==HEAD)
check('four recovered mixed pairs exact exit0 and hour remains running',recovery['status']=='running' and recovery['cell_outcomes']=={x:True for x in ['c1','c4','c8','c16']} and all(next(p for p in recovery['phases'] if p['name']==x+'-'+phase)['exit_code']==0 for x in ['c1','c4','c8','c16'] for phase in ['run','verify']) and next(p for p in recovery['phases'] if p['name']=='soak-run')['status']=='running' and 'exit_code' not in next(p for p in recovery['phases'] if p['name']=='soak-run'))
check('original pre-recovery observation retained',sha(ct['runtime-parent-recovery/original-execution-observation.json'])==recovery['original_observation_sha256'])
for cell in ['c1','c4','c8','c16']:
    report=json.loads(ct[cell+'/runtime/report.json'])
    check('mixed original report and seal equal prior reviewer snapshot '+cell,all(c[cell+'/runtime/'+name]['sha256']==digest(ROOT/'runtime-review/round11-m6-mixed/original-cell-text'/cell/name) for name in ['report.json','seal.json']))
    check('mixed pass does not erase false resource coverage '+cell,report['status']=='passed_observation' and report['exit_code']==0 and report['outcomes']=={'success':900} and report['resource_time_coverage']['passed'] is False)
backfill=json.loads(ct['backfill/receipt.json']);build=json.loads(ct['build/build-receipt.json'])
check('original completed build and backfill receipts',build['build_exit_code']==0 and build['status']=='passed' and backfill['status']=='passed_observation' and backfill['exit_code']==backfill['execution_exit_code']==0)
check('build exact prior independent source and seal',c['build/build-receipt.json']['sha256']=='c9b372f74d726345f518732c856a65fdc13bc98c0261a65eb092bf9383736f26' and c['build/seal.json']['sha256']=='73a948da66979dfab18aa2522a67d481b2fb2b76549d2a0b2b3e49155c820ad0')
check('unknown hour has no manufactured terminal or seal', 'soak/runtime/report.json' not in h and 'soak/runtime/seal.json' not in h and 'soak/runtime/parity.json' not in h and 'soak/runtime/statistics.json' not in h)
check('unknown hour WAL and SHM retained without deletion',all('soak/runtime/project/.codecortex/'+n in h for n in ['index.sqlite3','index.sqlite3-wal','index.sqlite3-shm']))
check('unknown hour distinct archive root only',all(k.startswith('soak/') for k in h) and not any(k.startswith('soak/') for k in c))
wrappers={}
for name,want in [('run_exact_runtime.py',initial['wrapper_sha256']),('run_after_parent_preparation.py',recovery['wrapper_sha256'])]:
    path=ROOT/'runtime-review/m6-local-managed'/name
    check('separately retained exact wrapper '+name,digest(path)==want)
    wrappers[name]=fi(path)
check('recovery binds original wrapper',recovery['original_wrapper_sha256']==initial['wrapper_sha256'])
for name,pkg in packages.items():
    pkg['member_inventory_sha256']=sha(jb(pkg.pop('member_inventory')))
report={'schema_version':1,'kind':'independent_complete_M6_archive_preservation_review','created_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'verdict':'APPROVE_exact_original_evidence_preservation_with_scope_limits','source':HEAD,'archives':packages,'checks':checks,'checks_passed':len(checks),'checks_failed':0,
        'scope':{'native_invocations':0,'sqlite_connections':0,'tar_extractions':0,'live_unknown_source_files_read':False,'original_files_modified':False,
                 'completed':'Fresh build, four recovered mixed run/verify pairs and fake backfill; initial five parentless runs and ten exit2 results preserved.',
                 'unknown_hour':'Exact observed DB/WAL/SHM, raw and plan bytes only; not a consistent database backup or complete hour. No native terminal, final report, endpoint parity or seal.',
                 'wrappers':'Original and recovery wrapper scripts are retained separately; their exact hashes are bound. Include these two referenced files with a self-contained publication package.',
                 'stability':'Matching before/after producer fields describe file observations, never prove global process quiescence. This reviewer verified all archive member hashes and selected prior independent snapshots.'},
        'separate_wrapper_files':wrappers,'producer_script':fi(ROOT/'integration-validation/prepare_round12_m6_original_archive.py'),'archive_index':fi(INPUT/'archive-index.json'),
        'prior_unknown_session_report':fi(ROOT/'runtime-review/round12-m6-session-loss/interruption-observation.json'),
        'prior_resource_gap_report':fi(ROOT/'runtime-review/round11-m6-soak-prefix/resource-gap-prefix-review.json'),
        'ledger_unchanged':{'total':192,'completed':163,'remaining':29,'new_original_completed':0}}
(OUT/'independent-archive-review.json').write_bytes(jb(report))
print(json.dumps({'verdict':report['verdict'],'checks':len(checks),'report':fi(OUT/'independent-archive-review.json')}))
