#!/usr/bin/env python3
"""Read/parse-only checks for the proposed workflow; never imports test code."""
import ast
import hashlib
import json
from pathlib import Path
import subprocess
import yaml

ROOT = Path(__file__).resolve().parent
BASE = '8e542e644b06a83fc82d18d9896cf82f5b2d796e'
SOURCE = Path('/workspace/scratch/2eaa00d0f93a/p8-staging-engineering-exact-source')
DRAFT = Path('/dev/shm/p8-runtime-finalization-draft-8e')

def require(value, reason):
    if not value:
        raise ValueError(reason)

def entry(path):
    body = path.read_bytes()
    return dict(bytes=len(body), sha256=hashlib.sha256(body).hexdigest(),
                git_blob=hashlib.sha1(b'blob '+str(len(body)).encode()+b'\0'+body).hexdigest())

def main():
    manifest = json.loads((ROOT/'test-expectations.json').read_bytes())
    controller = (ROOT/'controller.py').read_text()
    ast.parse(controller)
    workflow = yaml.load((ROOT/'p8-runtime-finalization-controls.yml').read_text(), Loader=yaml.BaseLoader)
    require(workflow['on'] == {'push': {'branches': ['task/p8-runtime-finalization-controls-20261009']}}, 'trigger changed')
    require(workflow['permissions'] == {'contents': 'read'}, 'permissions changed')
    require(list(workflow['jobs']) == ['python_controls'], 'extra job')
    job=workflow['jobs']['python_controls']
    require(job['if']=='github.run_attempt == 1' and job['runs-on']=='ubuntu-24.04' and job['timeout-minutes']=='20', 'job admission changed')
    steps=job['steps']
    require(len(steps)==4 and steps[0]['run']=='test "$GITHUB_RUN_ATTEMPT" = 1', 'attempt first guard changed')
    require(steps[1]['uses']=='actions/checkout@v4' and steps[1]['with']=={'ref':'${{ github.sha }}'}, 'checkout changed')
    run=steps[2]['run']
    require(run.startswith("python3 -B -u - <<'P8_CONTROLLER'\n") and run.endswith('P8_CONTROLLER\n'), 'bootstrap changed')
    bootstrap=run.split('\n',1)[1].rsplit('P8_CONTROLLER\n',1)[0]
    parsed=ast.parse(bootstrap)
    payloads=[n.value for n in parsed.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='payload' for t in n.targets)]
    require(len(payloads)==1 and ast.literal_eval(payloads[0])==controller, 'embedded body differs')
    require(steps[3]['if']=='always()' and steps[3]['uses']=='actions/upload-artifact@v4', 'failure artifact step changed')
    require(steps[3]['with']['if-no-files-found']=='error', 'missing artifact accepted')
    expected_counts=[]
    unchanged=[]
    for module in manifest['modules']:
        source=module['source']; path=source['path']
        body=(DRAFT/path).read_bytes() if module['module']=='test_p8_runtime_finalization' else subprocess.check_output(['git','-C',str(SOURCE),'show',BASE+':'+path])
        require(hashlib.sha256(body).hexdigest()==source['sha256'] and len(body)==source['bytes'], 'test bytes differ')
        tree=ast.parse(body.decode())
        ids=[]
        for cls in tree.body:
            if isinstance(cls,ast.ClassDef) and any(ast.unparse(b)=='unittest.TestCase' for b in cls.bases):
                for method in cls.body:
                    if isinstance(method,ast.FunctionDef) and method.name.startswith('test_'):
                        ids.append(module['module']+'.'+cls.name+'.'+method.name)
        require(sorted(ids)==[m['id'] for m in module['expected_methods']], 'static methods differ')
        expected_counts.append(len(ids))
        if module['module']!='test_p8_runtime_finalization':unchanged.append(path)
    require(expected_counts==[14,12,13,8], 'wrong denominator')
    require(sum(len(m['expected_subtests']) for module in manifest['modules'] for m in module['expected_methods'])==24, 'wrong subcase count')
    for source in manifest['fixed_observer_files']:
        path=source['path']
        body=(DRAFT/path).read_bytes() if path=='scripts/p8_runtime.py' else subprocess.check_output(['git','-C',str(SOURCE),'show',BASE+':'+path])
        require(hashlib.sha256(body).hexdigest()==source['sha256'] and len(body)==source['bytes'], 'observer bytes differ')
        if path!='scripts/p8_runtime.py':unchanged.append(path)
    report=dict(status='static_preparation_passed_not_test_execution', base_source=BASE,
        expected_methods=47, expected_explicit_subtests=24, module_method_counts=expected_counts,
        original_tests_and_observers_unchanged=unchanged,
        exact_embedded_controller=True, only_registered_push=True, attempt=1,
        source_inputs=1087, source_manifest_sha256=manifest['source_manifest_sha256'],
        files={name:entry(ROOT/name) for name in ('controller.py','p8-runtime-finalization-controls.yml','test-expectations.json','generate_manifest.py','check_static.py','README.md')},
        tests_executed=False, Cargo_executed=False, product_workloads_executed=False,
        remote_writes=False, new_TODO_done=0, TODO_remaining=29)
    (ROOT/'static-review.json').write_text(json.dumps(report,sort_keys=True,indent=2)+'\n')
    print(json.dumps({'status':report['status'],'report':entry(ROOT/'static-review.json')}))

if __name__=='__main__':main()
