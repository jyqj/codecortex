"""Read-only checks of retained hashes and original acceptance results; no product launch."""
import gzip
import hashlib
import json
from pathlib import Path
BASE=Path(__file__).resolve().parent
E=BASE/'evidence/current-once'
def read(p):
    if p.exists():return json.loads(p.read_text())
    with gzip.open(str(p)+'.gz','rt') as f:return json.load(f)
def sha(data):return hashlib.sha256(data).hexdigest()
def check_run(directory,count):
    summary=read(directory/'summary.json');case=directory/f'live/n{count}'
    expected='passed_preflight_32' if count==32 else 'passed_declared_100k_local_scope'
    assert summary['status']==expected and not summary['failures'] and not summary['not_run'] and not summary.get('guard_stop')
    assert summary['timeouts']['ready_drain']==300 and summary['resource_limits']['cgroup_memory_guard_bytes']==12*1024**3
    cold=read(case/'cold-result.json')['result'];assert cold['files_scanned']==cold['files_parsed']==cold['files_added']==count
    assert cold['files_skipped']==0 and not cold['parse_errors']
    final=read(case/'final-db.json');assert final['counts']['files']==final['counts']['document_manifest']==final['counts']['semantic_manifest']==count
    assert final['integrity']=='ok' and final['foreign_key_errors']==0
    ready=next(c['result'] for c in summary['checks'] if c['name']=='ready_manifest_integrity_fk');assert ready['drain_wall_seconds']<300
    ref=read(case/'serial-reference.json');queries=[f'resource_{i:05}' for i in (0,1,count//2,count-1)];assert list(ref)==queries
    for mode,offered in [('repeated',[queries[0]]*4),('distinct',queries)]:
        rows=read(case/f'concurrent-{mode}.json');assert [r['query'] for r in rows]==offered
        for r in rows:assert r['hits']==ref[r['query']]['hits']
    reopened=read(case/'reopen-db.json');assert reopened['counts']==final['counts'] and reopened['integrity']=='ok' and reopened['foreign_key_errors']==0
    assert read(case/'reopen-status.json')['semantic_state']=='ready'
    for r in read(case/'reopen-queries.json'):assert r['hits']==ref[r['query']]['hits']
    assert len(summary['product_exits'])==2
    assert all(r['exit_code']==0 and r['forced'] is False for r in summary['product_exits'])
    aggregate=hashlib.sha256();n=0
    with gzip.open(case/'source-inputs.jsonl.gz','rt') as f:
        for line in f:
            r=json.loads(line);data=f'pub fn resource_{n:05}() -> u32 {{ {n} }}\n'.encode()
            assert r==dict(path=f'src/file_{n:05}.rs',bytes=len(data),sha256=sha(data))
            aggregate.update(line.encode());n+=1
    generation=next(c['result'] for c in summary['checks'] if c['name']=='generation')
    assert n==count and aggregate.hexdigest()==generation['ordered_manifest_sha256']
    return dict(status=expected,files=count,drain_wall_seconds=ready['drain_wall_seconds'],exact_C4_and_reopen_hits=True,EOF_exit0_forced_false_twice=True,source_manifest_sha256=aggregate.hexdigest())
def main():
    publication=read(BASE/'publication-manifest.json')
    for r in publication['retained']:
        p=BASE/r['archive'];assert sha(p.read_bytes())==r['archive_sha256']
        if r['archive'].endswith('.gz') and not r['source'].endswith('.gz'):
            with gzip.open(p,'rb') as f:assert sha(f.read())==r['source_sha256']
        else:assert sha(p.read_bytes())==r['source_sha256']
        assert not {'repo','cache','target','cargo','tmp'} & set(Path(r['archive']).parts)
    session=read(E/'session.json');assert session['formal_runs']==1 and session['status']=='passed_declared_100k_local_scope'
    bundle=read(BASE/'evidence/build-attempt-1/builds-ready.json');receipt=read(BASE/'evidence/build-attempt-1/candidate/build-receipt.json')
    assert receipt['driver_files']==bundle['driver_files']==session['driver_files']
    assert receipt['source_sha']=='90858afae647a513537bf118932a7ba5020ee98b'
    assert receipt['build_exit_code']==0 and receipt['guard_stop'] is None
    assert sha((BASE/'evidence/build-attempt-1/candidate/build-receipt.json').read_bytes())==bundle['receipts']['candidate']['sha256']
    smoke=check_run(E/'preflight-candidate',32);formal=check_run(E/'measurement',100000)
    held=read(E/'preflight-candidate/sharedgate-preflight.json');assert held['actual_http_held']==2 and held['actual_returned']==0
    result=dict(status='passed_readonly_evidence_verification',archived_files=len(publication['retained']),formal_runs=1,smoke=smoke,formal=formal,receipt_driver_source_hash_binding=True,historical_pass_inherited=False,strict_paired_causal_speedup=False,group_independence='unknown')
    (BASE/'evidence-verification.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
if __name__=='__main__':main()
