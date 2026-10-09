#!/usr/bin/env python3
"""Fixed pure-Python auxiliary controls; no Cargo or product workload."""
import argparse
import datetime
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time
import unittest

ROOT = Path(os.environ['GITHUB_WORKSPACE']).resolve(strict=True)
EXPECTED_SOURCE = os.environ['GITHUB_SHA']
RELATIVE_MANIFEST = 'artifacts/checkpoints/p8-completion-20261009/runtime-finalization-controls/test-expectations.json'
MANIFEST = ROOT / RELATIVE_MANIFEST
MANIFEST_SHA = 'd0278ef5a24787aea32f5f95d6892b3aa46d19d01dcd8b10d30456c9665b4db9'
WORKFLOW = '.github/workflows/p8-runtime-finalization-controls.yml'
MODULES = ['test_p8_runtime','test_p8_runtime_evidence','test_p8_runtime_cache','test_p8_runtime_finalization']
sys.path[:0] = [str(ROOT/'scripts'), str(ROOT/'scripts/tests')]
from p7_build_identity import json_bytes, source_snapshot
from p8_runtime_build import artifact_inventory, observer_snapshot

def require(ok, reason):
    if not ok: raise ValueError(reason)

def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()

def read(path):
    require(Path(path).is_file() and not Path(path).is_symlink(), 'nonregular JSON')
    return json.loads(Path(path).read_bytes())

def write_new(path, value):
    with Path(path).open('xb') as stream:stream.write(json_bytes(value))

def utc():return datetime.datetime.now(datetime.timezone.utc).isoformat()

def git_bytes(path):
    return subprocess.check_output(['git','-C',str(ROOT),'show',EXPECTED_SOURCE+':'+path])

def fixed_inputs(manifest):
    paths = [entry['source'] for entry in manifest['modules']] + manifest['fixed_observer_files']
    result={}
    for entry in paths:
        path=ROOT/entry['path'];body=path.read_bytes()
        require(path.is_file() and not path.is_symlink() and path.resolve()==path, 'aliased fixed input')
        require(len(body)==entry['bytes'] and hashlib.sha256(body).hexdigest()==entry['sha256'], 'fixed input bytes changed: '+entry['path'])
        require(body==git_bytes(entry['path']), 'fixed input differs from actual committed source')
        mode=subprocess.check_output(['git','-C',str(ROOT),'ls-tree',EXPECTED_SOURCE,'--',entry['path']],text=True).split()[0]
        require(mode==entry['mode'], 'fixed input Git mode changed')
        result[entry['path']]=entry
    require(MANIFEST.read_bytes()==git_bytes(RELATIVE_MANIFEST) and sha(MANIFEST)==MANIFEST_SHA, 'static population differs from actual commit')
    require((ROOT/WORKFLOW).read_bytes()==git_bytes(WORKFLOW), 'workflow differs from actual commit')
    result[RELATIVE_MANIFEST]={'bytes':MANIFEST.stat().st_size,'sha256':sha(MANIFEST)}
    result[WORKFLOW]={'bytes':(ROOT/WORKFLOW).stat().st_size,'sha256':sha(ROOT/WORKFLOW)}
    return result

def load_manifest():
    require(not sys.flags.optimize, 'ordinary Python required')
    require(sha(MANIFEST)==MANIFEST_SHA, 'expected static population changed')
    manifest=read(MANIFEST)
    require(manifest['expected_methods']==47 and manifest['expected_explicit_subtests']==24, 'fixed method/subtest denominator')
    require([m['expected_count'] for m in manifest['modules']]==[14,12,13,8], 'fixed module denominator')
    return manifest

def normalize(value):
    if value is None or type(value) in (str,bool,int,float):return value
    if isinstance(value,(list,tuple)):return [normalize(x) for x in value]
    if isinstance(value,dict):return {str(k):normalize(v) for k,v in value.items()}
    if callable(value) and hasattr(value,'__code__'):
        return {'kind':'callable','module':value.__module__,'qualname':value.__qualname__,'first_line':value.__code__.co_firstlineno}
    raise ValueError('unregistered subtest parameter type: '+type(value).__name__)

def flatten(suite):
    for test in suite:
        if isinstance(test,unittest.TestSuite):yield from flatten(test)
        else:yield test

def one_module(name, out):
    manifest=load_manifest()
    fixed_inputs(manifest)
    expected=next(m for m in manifest['modules'] if m['module']==name)
    out.mkdir(exist_ok=False)
    module=importlib.import_module(name)
    require(Path(module.__file__).resolve()==ROOT/expected['source']['path'], 'test module loaded from another checkout')
    suite=unittest.TestLoader().loadTestsFromModule(module)
    wanted=[x['id'] for x in expected['expected_methods']]
    require(sorted(test.id() for test in flatten(suite))==wanted, 'loaded suite does not equal all fixed methods')
    events=[]
    subtests={name:[] for name in wanted}
    summary={'module':name,'status':'running','expected_ids':wanted,'loaded_ids':wanted,'started_utc':utc()}
    code=2
    with (out/'events.jsonl').open('x') as log:
        def emit(kind, **fields):
            row={'kind':kind,**fields};events.append(row)
            log.write(json.dumps(row,sort_keys=True,ensure_ascii=False)+'\n');log.flush()
        class Result(unittest.TextTestResult):
            def startTest(self,test):
                emit('test_started',test_id=test.id());super().startTest(test)
            def stopTest(self,test):
                emit('test_stopped',test_id=test.id());super().stopTest(test)
            def addSuccess(self,test):
                emit('test_success',test_id=test.id());super().addSuccess(test)
            def addFailure(self,test,err):
                emit('test_failure',test_id=test.id(),error=repr(err[1]));super().addFailure(test,err)
            def addError(self,test,err):
                emit('test_error',test_id=test.id(),error=repr(err[1]));super().addError(test,err)
            def addSkip(self,test,reason):
                emit('test_skipped',test_id=test.id(),reason=reason);super().addSkip(test,reason)
            def addExpectedFailure(self,test,err):
                emit('test_expected_failure',test_id=test.id(),error=repr(err[1]));super().addExpectedFailure(test,err)
            def addUnexpectedSuccess(self,test):
                emit('test_unexpected_success',test_id=test.id());super().addUnexpectedSuccess(test)
            def addSubTest(self,test,subtest,err):
                params=normalize(dict(subtest.params));subtests[test.id()].append(params)
                emit('subtest',test_id=test.id(),parameters=params,passed=err is None,error=None if err is None else repr(err[1]))
                super().addSubTest(test,subtest,err)
        try:
            result=unittest.TextTestRunner(verbosity=2,resultclass=Result).run(suite)
            starts=[e['test_id'] for e in events if e['kind']=='test_started']
            stops=[e['test_id'] for e in events if e['kind']=='test_stopped']
            successes=[e['test_id'] for e in events if e['kind']=='test_success']
            exact=(sorted(starts)==wanted and sorted(stops)==wanted and sorted(successes)==wanted
                   and result.testsRun==expected['expected_count'] and result.wasSuccessful()
                   and not result.skipped and not result.expectedFailures and not result.unexpectedSuccesses)
            expected_subtests={m['id']:m['expected_subtests'] for m in expected['expected_methods']}
            exact=exact and subtests==expected_subtests and all(e['passed'] for e in events if e['kind']=='subtest')
            summary.update(tests_run=result.testsRun,started_ids=starts,stopped_ids=stops,success_ids=successes,
                           failed=len(result.failures),errors=len(result.errors),skipped=len(result.skipped),
                           expected_failures=len(result.expectedFailures),unexpected_successes=len(result.unexpectedSuccesses),
                           actual_subtests=subtests,expected_subtests=expected_subtests,
                           status='passed_exact_population' if exact else 'failed_or_incomplete_population')
            code=0 if exact else 1
        except BaseException as error:
            summary.update(status='invalid_or_interrupted_control',error=f'{type(error).__name__}: {error}')
            emit('runner_exception',error=summary['error'])
        finally:
            summary.update(exit_code=code,finished_utc=utc())
    write_new(out/'summary.json',summary)
    return code

def all_modules():
    out=Path(os.environ['RUNNER_TEMP'])/'runtime-finalization-controls'
    out.mkdir(exist_ok=False)
    commands=[]
    for index,module in enumerate(MODULES):
        commands.append([sys.executable,'-B','-u',str(Path(__file__).resolve()),'--module',module,'--out',str(out/f'module-{index:02}')])
    record=dict(schema_version=1,status='running',expected_source=EXPECTED_SOURCE,commands=commands,results=[],
                expected_methods=47,expected_explicit_subtests=24,manifest_sha256=MANIFEST_SHA,
                scope='Pure Python protocol/regression controls only; no Cargo, product workload, N30, task or release approval.',
                environment={k:os.environ.get(k) for k in ('GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT','GITHUB_REF','RUNNER_OS')},
                python=dict(executable=sys.executable,version=sys.version),started_utc=utc())
    write_new(out/'plan.json',record)
    before=observer=inputs=manifest=None
    code=2
    child_confirmed_stopped=True
    try:
        manifest=load_manifest()
        require([m['module'] for m in manifest['modules']]==MODULES,'unexpected planned module')
        require(os.environ['GITHUB_RUN_ATTEMPT']=='1' and os.environ['GITHUB_REF']=='refs/heads/task/p8-runtime-finalization-controls-20261009', 'wrong attempt or branch')
        before=source_snapshot(ROOT);observer=observer_snapshot(ROOT);inputs=fixed_inputs(manifest)
        require(before['source_commit']==EXPECTED_SOURCE and before['input_count']==1087 and before['manifest_sha256']==manifest['source_manifest_sha256'],'actual complete source does not match fixed production inputs')
        write_new(out/'source-before.json',before);write_new(out/'observer-before.json',observer);write_new(out/'validation-before.json',inputs)
        (out/'controller.py').write_bytes(Path(__file__).read_bytes());(out/'workflow.yml').write_bytes((ROOT/WORKFLOW).read_bytes());(out/'test-expectations.json').write_bytes(MANIFEST.read_bytes())
        for index,command in enumerate(commands):
            row=dict(argv=command,started_utc=utc(),stdout=f'{index:02}.stdout',stderr=f'{index:02}.stderr',status='started')
            record['results'].append(row)
            (out/'progress.json').write_bytes(json_bytes(record))
            env=dict(os.environ)
            env['P8_RUNTIME_CONTROL_EVIDENCE_DIR']=str(out/'protocol-fixtures'/f'module-{index:02}')
            env['PYTHONDONTWRITEBYTECODE']='1'
            row['retained_protocol_fixtures']=env['P8_RUNTIME_CONTROL_EVIDENCE_DIR']
            begun=time.monotonic_ns()
            with (out/row['stdout']).open('xb') as stdout,(out/row['stderr']).open('xb') as stderr:
                child_confirmed_stopped=False
                process=subprocess.run(command,cwd=ROOT,env=env,stdout=stdout,stderr=stderr,check=False)
                child_confirmed_stopped=True
            row.update(status='finished',exit_code=process.returncode,elapsed_ns=time.monotonic_ns()-begun)
            (out/'progress.json').write_bytes(json_bytes(record))
            if process.returncode:break
        code=0 if len(record['results'])==4 and all(row.get('exit_code')==0 for row in record['results']) else 1
        record['status']='commands_completed' if code==0 else 'command_failed'
        if code==0:
            summaries=[read(out/f'module-{i:02}'/'summary.json') for i in range(4)]
            require(sum(s['tests_run'] for s in summaries)==47 and sum(sum(len(v) for v in s['actual_subtests'].values()) for s in summaries)==24,'actual aggregate population differs')
            require(all(s['status']=='passed_exact_population' and s['exit_code']==0 for s in summaries),'partial child result')
            record.update(actual_methods=47,actual_explicit_subtests=24,failed=0,skipped=0)
    except BaseException as error:
        code=2;record.update(status='invalid_or_interrupted_execution',error=f'{type(error).__name__}: {error}')
    finally:
        try:
            after=source_snapshot(ROOT);observer_after=observer_snapshot(ROOT)
            inputs_after=fixed_inputs(manifest) if manifest is not None else None
            write_new(out/'source-after.json',after);write_new(out/'observer-after.json',observer_after);write_new(out/'validation-after.json',inputs_after)
            record['source_and_observer_unchanged']=before==after and observer==observer_after and inputs==inputs_after
            if not record['source_and_observer_unchanged']:code=2;record['status']='invalid_source_or_observer_change'
        except BaseException as error:
            code=2;record.update(status='invalid_final_source_check',source_and_observer_unchanged=False,final_source_error=repr(error))
        record.update(exit_code=code,finished_utc=utc(),child_process_confirmed_stopped=child_confirmed_stopped,
                      not_run_command_indices=list(range(len(record['results']),len(commands))),
                      evidence_status='closed_control_inventory' if child_confirmed_stopped else 'unsealed_child_termination_unconfirmed',
                      hard_termination_limit='Runner shutdown or job hard timeout can prevent final receipt/upload; missing evidence never passes.',
                      Cargo_executed=False,new_product_workload_executed=False,new_TODO_done=0,TODO_remaining=29)
        try:record['files']=artifact_inventory(out,exclude=None)
        except BaseException as error:
            code=2;record.update(status='invalid_inventory',exit_code=2,inventory_error=repr(error),evidence_status='unsealed_inventory_error')
        write_new(out/'receipt.json',record)
    print(json.dumps({'status':record['status'],'exit_code':code,'actual_methods':record.get('actual_methods'),'receipt_sha256':sha(out/'receipt.json'),'new_TODO_done':0,'TODO_remaining':29}))
    return code

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--module')
    parser.add_argument('--out',type=Path)
    args=parser.parse_args()
    if args.module:
        require(args.out is not None,'module output missing')
        raise SystemExit(one_module(args.module,args.out))
    raise SystemExit(all_modules())
