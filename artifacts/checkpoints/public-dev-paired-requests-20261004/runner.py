#!/usr/bin/env python3
"""One-shot Requests paired public DEV; immutable product/scorer and admitted loader."""
import collections
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import datetime

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, '/tmp/paired-requests-python')
import blake3
PINS = {'base': '88f2cf099c8b81f3acef485fd5ac9b01c63ce790',
        'candidate': '37dd042eaa1209a86e0cafdcd92ae77e036e76f5'}
def sha(b): return hashlib.sha256(b).hexdigest()
def write(p, x): p.write_text(json.dumps(x, indent=2, sort_keys=True)+'\n')
def git(*a): return subprocess.check_output(['git', *a], cwd=ROOT)
def hashes(p): return {str(f.relative_to(p)):sha(f.read_bytes()) for f in sorted(p.rglob('*')) if f.is_file()}
def command(argv, cwd, logfile):
    with logfile.open('wb') as f:
        r = subprocess.run(argv, cwd=cwd, stdout=f, stderr=subprocess.STDOUT)
    return r.returncode
def main():
    spec=importlib.util.spec_from_file_location('admitted_selector',ROOT/'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003/selector.py')
    loader=importlib.util.module_from_spec(spec); spec.loader.exec_module(loader)
    receipt=json.loads((loader.HERE/'admission-receipt.json').read_text())
    history=json.loads(Path('/tmp/pygo-original/commands.json').read_text())
    history=[r for r in history if r['repo']=='requests']
    assert [r['profile'] for r in history]==['native','compat']
    records=[]; locks={}
    for side,commit in PINS.items():
        checkout=Path('/workspace/paired-requests-'+side)
        assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=checkout).decode().strip()==commit
        assert not subprocess.check_output(['git','status','--porcelain'],cwd=checkout)
        bins={n: checkout/'target/debug'/n for n in ('cc-eval','codecortex')}
        binding={'head':commit,'checkout':str(checkout),'target':str(checkout/'target'),
                 'binary_sha256':{n:sha(p.read_bytes()) for n,p in bins.items()},
                 'lock_sha256':sha((checkout/'Cargo.lock').read_bytes()),
                 'rustc':subprocess.check_output(['rustc','+1.95.0','--version']).decode().strip(),
                 'source_sha256':{p:sha(subprocess.check_output(['git','show',commit+':'+p],cwd=checkout))
                     for p in subprocess.check_output(['git','ls-tree','-r','--name-only',commit],cwd=checkout).decode().splitlines()
                     if p in ('Cargo.toml','Cargo.lock') or (p.startswith('crates/') and ('/src/' in p or p.endswith('/Cargo.toml')))}}
        write(HERE/('binding-'+side+'.json'),binding)
        scratch=Path('/workspace/paired-requests-'+side+'-inputs'); scratch.mkdir()
        actual={}
        # Read actual on-disk corpus map into loader, including Gin for strict admission;
        # only Requests is ever given to the evaluator.
        for entry,pin in receipt['source_bytes_sha256'].items():
            relative=entry.split('/public-v19/',1)[1]
            path=scratch/relative; path.parent.mkdir(parents=True,exist_ok=True)
            path.write_bytes(git('show',entry)); actual[entry]=path.read_bytes()
            assert sha(actual[entry])==pin
        packages=loader.load_version(version=loader.VERSION,repositories=['requests','gin'],source_bytes=actual)
        p=packages['requests']; locks[side]=p['input_lock']
        for mode in ('native','compat'):
            path=scratch/'requests'/('queries.'+mode+'.dev.jsonl'); path.write_bytes(p[mode])
            suite=json.loads(p['suites'][mode])
            original=json.loads((Path('/tmp/pygo-original')/('requests-.-'+mode)/'manifest.json').read_text())['suite']
            assert suite==original
            if mode=='native': suite['queries_digest']=blake3.blake3(p[mode]).hexdigest()
            suitepath=scratch/'requests'/('suite-'+mode+'-dev.json'); write(suitepath,suite)
            assert len(p[mode].splitlines()) in (91,83)
            out=Path('/workspace/paired-requests-'+side+'-runs')/mode
            validate=[str(bins['cc-eval']),'validate','--suite',str(suitepath)]
            assert command(validate,checkout,HERE/(side+'-'+mode+'-validate.log'))==0
            argv=[str(bins['cc-eval']),'run','--backend','mcp-stdio','--binary',str(bins['codecortex']),
                  '--suite',str(suitepath),'--output',str(out),'--profile','quality']
            started=datetime.datetime.now(datetime.timezone.utc).isoformat()
            rc=command(argv,checkout,HERE/(side+'-'+mode+'-run.log'))
            before=hashes(out)
            replay=[str(bins['cc-eval']),'replay','--run',str(out)]
            replay_rc=command(replay,checkout,HERE/(side+'-'+mode+'-replay.log'))
            after=hashes(out)
            rows=[json.loads(l) for l in (out/'normalized.jsonl').read_text().splitlines()]
            statuses=dict(collections.Counter(r['status'] for r in rows))
            manifest=json.loads((out/'manifest.json').read_text())
            prepare=json.loads((out/'prepare.json').read_text()) if (out/'prepare.json').exists() else None
            readiness=json.loads((out/'readiness.json').read_text()) if (out/'readiness.json').exists() else None
            record={'side':side,'mode':mode,'source_commit':commit,'command':argv,'run_exit':rc,
                    'replay_command':replay,'replay_exit':replay_rc,'replay_byte_identical':before==after,
                    'started_utc':started,'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                    'scheduled':len(p[mode].splitlines())*suite['repetitions'],'executed':len(rows),
                    'missing':len(p[mode].splitlines())*suite['repetitions']-len(rows),'statuses':statuses,
                    'infrastructure_failure':manifest['infrastructure_failure'],
                    'prepare':prepare,'readiness':readiness,'metrics':json.loads((out/'metrics.json').read_text()),
                    'gate':json.loads((out/'gate.json').read_text()),'raw_sha256':after,
                    'suite_sha256':sha(suitepath.read_bytes()),'query_sha256':sha(p[mode]),
                    'source_digest':manifest['input']['source_digest'],
                    'retrieval_costs_sha256':sha((out/'costs.jsonl').read_bytes())}
            records.append(record); write(HERE/'runs.json',records)
            print(side,mode,'scheduled',record['scheduled'],'executed',len(rows),statuses,'replay',before==after,flush=True)
            assert before==after
            assert record['source_digest']==p['input_lock']['original_source_inventory_digest']
            assert record['infrastructure_failure'] is None
            assert prepare['result']['parse_errors']==[] and readiness['state']=='ready'
    assert locks['base']==locks['candidate']
    for mode in ('native','compat'):
        a,b=[r for r in records if r['mode']==mode]
        assert a['query_sha256']==b['query_sha256'] and a['suite_sha256']==b['suite_sha256']
    write(HERE/'paired-input-lock.json',locks)
if __name__=='__main__': main()
