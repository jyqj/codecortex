#!/usr/bin/env python3
"""Static AST inventory only; never imports or runs test modules."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess

OUT = Path(__file__).resolve().parent
ROOT = Path('/workspace/scratch/2eaa00d0f93a/p8-staging-engineering-exact-source')
BASE = '8e542e644b06a83fc82d18d9896cf82f5b2d796e'
DRAFT = Path('/dev/shm/p8-runtime-finalization-draft-8e')
MODULES = ['test_p8_runtime', 'test_p8_runtime_evidence', 'test_p8_runtime_cache', 'test_p8_runtime_finalization']

def git_bytes(path):
    return subprocess.check_output(['git', '-C', str(ROOT), 'show', BASE + ':' + path])

def file_entry(path, data):
    return dict(path=path, bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
                git_blob=hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest(), mode='100644')

def generate():
    modules=[]
    for module in MODULES:
        path='scripts/tests/'+module+'.py'
        data=(DRAFT/path).read_bytes() if module==MODULES[-1] else git_bytes(path)
        source=data.decode();tree=ast.parse(source)
        methods=[]
        for cls in tree.body:
            if not isinstance(cls,ast.ClassDef) or not any(ast.unparse(base)=='unittest.TestCase' for base in cls.bases):
                continue
            for method in cls.body:
                if isinstance(method,ast.FunctionDef) and method.name.startswith('test_'):
                    require_no_skip=all('skip' not in ast.unparse(d) and 'expectedFailure' not in ast.unparse(d) for d in method.decorator_list)
                    if not require_no_skip:raise ValueError('unexpected ignored method')
                    body=ast.get_source_segment(source,method).encode()
                    methods.append(dict(id=f'{module}.{cls.name}.{method.name}',line=method.lineno,
                        source_sha256=hashlib.sha256(body).hexdigest(),expected_subtests=[]))
        methods.sort(key=lambda x:x['id'])
        modules.append(dict(module=module,source=file_entry(path,data),expected_methods=methods,expected_count=len(methods)))
    by_suffix={m['id'].split('.')[-1]:m for mod in modules for m in mod['expected_methods']}
    by_suffix['test_zero_ambiguous_reset_missing_and_boolean_counters_rejected']['expected_subtests']=[
        {'changes':x} for x in [dict(graph_hits=0,graph_misses=0),dict(graph_hits=1,graph_misses=1),
        dict(graph_hits=2,graph_misses=0),dict(graph_hits=True),dict(graph_hits=-1),dict(result_misses=1,graph_hits=1)]]
    by_suffix['test_cross_generation_or_server_cannot_be_cache_receipt']['expected_subtests']=[{'key':x} for x in ['pid','epoch','incarnation','incomplete']]
    cache=git_bytes('scripts/tests/test_p8_runtime_cache.py').decode();ct=ast.parse(cache)
    cache_cls=next(x for x in ct.body if isinstance(x,ast.ClassDef) and x.name=='SoakCacheProtocolTests')
    cache_method=next(x for x in cache_cls.body if isinstance(x,ast.FunctionDef) and x.name=='test_cache_hit_cannot_hide_stale_source_or_wrong_entity')
    bad_list=next(x.value for x in cache_method.body if isinstance(x,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='bad_hits' for t in x.targets))
    by_suffix[cache_method.name]['expected_subtests']=[{'bad':{'kind':'callable','module':'test_p8_runtime_cache',
        'qualname':f'SoakCacheProtocolTests.{cache_method.name}.<locals>.<lambda>','first_line':x.lineno}} for x in bad_list.elts]
    by_suffix[cache_method.name]['subtest_source_expressions']=[ast.get_source_segment(cache,x) for x in bad_list.elts]
    by_suffix['test_each_failed_request_prefix_keeps_actual_counts_no_invented_attempts']['expected_subtests']=[{'role':x} for x in ['before_status','symbol','hybrid','after_status']]
    by_suffix['test_keyboard_interrupt_in_seal_or_verify_keeps_failed_report_after_workers_finish']['expected_subtests']=[{'fault':x} for x in ['seal_interrupt','verify_interrupt']]
    assert [m['expected_count'] for m in modules]==[14,12,13,8]
    assert sum(len(m['expected_subtests']) for mod in modules for m in mod['expected_methods'])==24
    fixed=[]
    for path in ['scripts/p8_runtime.py','scripts/p8_runtime_build.py','scripts/p7_build_identity.py',
                 'scripts/p8_cold_build.py','scripts/p8_rollback.py',
                 'scripts/resource_harness/__init__.py','scripts/resource_harness/runtime.py']:
        data=(DRAFT/path).read_bytes() if path=='scripts/p8_runtime.py' else git_bytes(path)
        fixed.append(file_entry(path,data))
    result=dict(schema='p8-runtime-finalization-python-static-population-v1',status='static_expectation_not_execution',
        baseline_source=BASE,source_input_count=1087,source_manifest_sha256='7a561d39191708023052b22ef5d626bf26ecf4ca6042eb39dc2f50a82ceb34bf',
        branch='task/p8-runtime-finalization-controls-20261009',modules=modules,expected_methods=47,expected_explicit_subtests=24,
        expected_skipped=0,expected_failures=0,expected_unexpected_successes=0,fixed_observer_files=fixed,
        limitations=['Declared subTest callbacks are separate observations within their parent method, not extra test methods.',
                    'Loops/assertions without self.subTest remain part of the unchanged parent method.',
                    'This manifest records no executed test or product result.'])
    (OUT/'test-expectations.json').write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
    print(json.dumps({'methods':47,'subtests':24,'sha256':hashlib.sha256((OUT/'test-expectations.json').read_bytes()).hexdigest()}))

if __name__=='__main__':generate()
