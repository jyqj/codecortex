#!/usr/bin/env python3
"""Offline validator tests against retained real small-smoke native snapshots, not performance."""
import copy, hashlib, importlib.util, json, pathlib, sys
root=pathlib.Path(__file__).resolve().parents[1]
spec=importlib.util.spec_from_file_location('validator',root/'inputs/validate_resources.py')
m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
source=pathlib.Path('artifacts/benchmarks/p5e-protocol-smoke-20261001/run-final-lifecycle/resource-samples.json')
data=json.loads(source.read_text());positive=m.check(data);assert positive['exit_code']==0,positive
cases={}
def run(name, mutate):
    d=copy.deepcopy(data);mutate(d);r=m.check(d);assert r['exit_code']==1,name
    cases[name]={'status':r['status'],'issues':r['issues'],'resources':r['resources']}
run('missing_snapshot',lambda d:d['samples'][0].update(server=None))
run('wrong_child_pid',lambda d:d['samples'][0]['server'].update(pid=1))
run('uncalibrated_unit',lambda d:d['samples'][0]['server'].update(cpu_time_unit='uncalibrated'))
run('wrong_timebase',lambda d:d['samples'][0]['server'].update(timebase_numer=126))
run('raw_ns_mismatch',lambda d:d['samples'][0]['runner'].update(cpu_user_ns=1))
run('nonmonotonic_cpu',lambda d:d['samples'][-1]['server'].update(cpu_user_raw=0,cpu_user_ns=0))
run('one_sample',lambda d:d.update(samples=d['samples'][:1]))
run('invalid_timestamp',lambda d:d['samples'][0].update(started_us=-1))
run('interval_order',lambda d:d['samples'][1].update(started_us=0))
run('boolean_not_integer',lambda d:d['samples'][0]['runner'].update(threads=True))
run('overflow_ns',lambda d:d['samples'][0]['server'].update(cpu_user_raw=(1<<64)-1,cpu_user_ns=(1<<64)-1))
run('unknown_memory_method',lambda d:d['samples'][0]['server'].update(resident_method='tree-rss'))
run('zero_live_threads',lambda d:d['samples'][0]['runner'].update(threads=0))
output=pathlib.Path(sys.argv[1]);assert not output.exists()
output.write_text(json.dumps({'status':'offline_validator_selfcheck_passed','source':str(source),'source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'validator_sha256':hashlib.sha256((root/'inputs/validate_resources.py').read_bytes()).hexdigest(),'positive':positive,'negative_cases':cases,'limits':['no new process or cargo run','retained 5file/4job protocol smoke is not candidate or formal G5 performance','all13 negative mutations rejected; no unavailable snapshot inferred zero']},indent=2)+'\n')
print('positive PASS; 13 negative mutations rejected')
