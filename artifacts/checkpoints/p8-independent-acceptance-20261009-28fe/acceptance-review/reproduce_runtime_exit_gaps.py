#!/usr/bin/env python3
"""Independent synthetic control of owned runtime exit records, no product evidence."""
import json, pathlib, sys
from concurrent.futures import ThreadPoolExecutor, wait as real_wait
from unittest import mock
repo=pathlib.Path('/workspace/scratch/28fef0db5e01/codecortex')
sys.path[:0]=[str(repo/'scripts'),str(repo/'scripts/tests')]
import p8_runtime as runtime
from test_p8_runtime import RuntimeDriverControls
results={'scope':'isolated synthetic runtime controls; build admission mocked, original process/RPC/future/journal/seal paths used','files':{str(p.relative_to(repo)):runtime.digest(p) for p in [repo/'scripts/p8_runtime.py',repo/'scripts/tests/test_p8_runtime.py']},'cases':[]}
for kind in ['keyboard_interrupt','keyboard_interrupt_after_drain','submit_failure']:
    control=RuntimeDriverControls()
    args,identity=control.prepare_control(0)
    attempts=[]
    futures=[]
    class FailingPool(ThreadPoolExecutor):
        def submit(self,*a,**kw):
            attempts.append(a[1])
            if kind=='submit_failure' and len(attempts)==5:
                raise RuntimeError('independent submit failure before enqueue')
            future=super().submit(*a,**kw)
            futures.append(future)
            return future
    def interrupt_wait(selected,timeout):
        if kind.startswith('keyboard_interrupt') and timeout==180:
            if kind=='keyboard_interrupt_after_drain':
                real_wait(selected, timeout=timeout)
            raise KeyboardInterrupt('independent scheduler cancellation')
        return real_wait(selected,timeout=timeout)
    result={'id':kind}
    try:
        with mock.patch.object(runtime,'verify_build',return_value=identity), mock.patch.object(runtime,'verify_receipt',return_value=identity['build_identity']),mock.patch.object(runtime,'ThreadPoolExecutor',FailingPool),mock.patch.object(runtime,'wait',side_effect=interrupt_wait):
            try:runtime.run(args)
            except BaseException as error:result['raised']=type(error).__name__+': '+str(error)
        report=json.loads((args.output/'report.json').read_text())
        raw=[json.loads(line) for line in (args.output/'raw.jsonl').read_text().splitlines()]
        operation_ids=sorted(row['id'] for row in raw if row['kind']=='operation')
        result.update(attempted_ids=attempts,operation_terminal_ids=operation_ids,missing_attempt_terminal_ids=sorted(set(attempts)-set(operation_ids)),report={key:report.get(key) for key in ('status','exit_code','offered_terminal_rows','error','work_cleanup','artifact_seal')},statistics_present=(args.output/'statistics.json').exists(),all_returned_futures_done=all(f.done() for f in futures))
        try:runtime.verify_output(args.output);result['archive_seal_valid']=True
        except Exception as error:result['archive_seal_valid']=str(error)
    finally:
        real_wait(futures,timeout=5)
        control.doCleanups()
    results['cases'].append(result)
print(json.dumps(results,ensure_ascii=False,indent=2))
