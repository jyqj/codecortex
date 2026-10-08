#!/usr/bin/env python3
"""Read-only ZIP/trace acceptance audit for the completed, fixed ffdc CI run.

No binaries are executed here. The original fixed verifier is reapplied to
complete retained traces. Every ZIP member is streamed for CRC, length, SHA256,
and mode; only small original evidence members are copied to the review bundle.
"""
import os
import sys
sys.dont_write_bytecode = True
import collections
import hashlib
import json
import re
import stat
import subprocess
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(os.environ['P7_AUDIT_SOURCE_ROOT']).resolve()
HEAD = 'ffdc6f0f97db78cc25a6c026904e7c2adde05d14'
OUT = Path(os.environ['P7_AUDIT_OUTPUT']).resolve()
ZIPROOT = Path(os.environ['P7_AUDIT_ZIP_ROOT']).resolve()
SOURCES = json.loads(Path(__file__).with_name('ciffdc-verifier-source.json').read_text())
sys.path.insert(0, str(ROOT / 'scripts'))
import p7_offline_trace as trace
from p7_build_identity import file_sha256, json_bytes, source_snapshot

PACKAGES = {
 'default': (11529447454, 141787552, '21e15185fc7e0fe917d1253a283026b20e69266fcd75768cff0f2012632aef68',113155755350),
 'semantic': (11528648875, 18306424, '59f151919b95508017da6aca062d7be46a5c40b65cd87fae26e070707b6389d5',113155755253),
}

def need(value, message):
    if not value:
        raise ValueError(message)

def same(a, b):
    return json.loads(json.dumps(a, sort_keys=True)) == json.loads(json.dumps(b, sort_keys=True))

def save(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('xb') as stream:
        stream.write(json_bytes(value))

def fixed_sources():
    for name, record in SOURCES['files'].items():
        need(file_sha256(ROOT/name) == record['sha256'], 'review source changed: '+name)

def cargo_record(z, prefix, receipt, target, kind):
    rows = [json.loads(line) for line in z.read(prefix+'/cargo-build.jsonl').splitlines()]
    need([r.get('success') for r in rows if r.get('reason')=='build-finished'] == [True], 'Cargo did not finish successfully')
    selected = [r for r in rows if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')==target and r.get('target',{}).get('kind')==[kind]]
    need(selected == [receipt['cargo_artifact']], 'raw Cargo artifact differs from receipt')

def raw_trace_check(rawdir, record):
    need(record['returncode']==0 and 'error' not in record, 'trace execution incomplete')
    need(record['trace_expression']==trace.TRACE_EXPRESSION, 'trace expression drift')
    need(all(x in record['command'] for x in ['-ff','-ttt','--decode-fds=socket']), 'missing full inode/timestamp trace options')
    files={p.name:file_sha256(p) for p in rawdir.iterdir() if p.is_file() and p.name!='trace-run.json'}
    need(files==record['raw_files'], 'trace raw file inventory/hash mismatch')
    return trace.read_traces(rawdir/'syscalls',require_timestamps=True)

def audit_package(profile, info, source):
    artifact, expected_size, expected_sha, job = info
    zpath=ZIPROOT/f'github-actions-artifact-{artifact}.zip'
    need(zpath.stat().st_size==expected_size and file_sha256(zpath)==expected_sha,'ZIP bytes do not match GitHub digest')
    destination=OUT/profile
    destination.mkdir(parents=True,exist_ok=False)
    records={}
    with zipfile.ZipFile(zpath) as z:
        infos=z.infolist()
        names=[i.filename for i in infos]
        need(len(names)==len(set(names)), 'duplicate ZIP entries')
        for item in infos:
            parts=PurePosixPath(item.filename).parts
            need(not item.is_dir() and not item.filename.startswith('/') and '\\' not in item.filename
                 and all(p not in ('','.','..') for p in parts), 'noncanonical ZIP name')
            mode=item.external_attr>>16
            need(stat.S_ISREG(mode), 'nonregular ZIP member')
            digest=hashlib.sha256(); size=0
            with z.open(item) as stream:
                while block:=stream.read(1024*1024):
                    digest.update(block);size+=len(block)
            need(size==item.file_size, 'ZIP member length mismatch')
            records[item.filename]={'sha256':digest.hexdigest(),'bytes':size,'executable':bool(mode&0o111)}
        def get(name):return json.loads(z.read(name))
        # Preserve exact small evidence; never extract the product/test binaries.
        selected=[]
        for name in names:
            keep=(not name.startswith(('p8-candidate-execution/','p8-runner-build/'))
                  and name not in ('product/codecortex','procfs-controls'))
            if keep:
                selected.append(name)
                target=destination/'raw'/name;target.parent.mkdir(parents=True,exist_ok=True)
                with target.open('xb') as stream:stream.write(z.read(name))
        receipt=get('product/build-receipt.json')
        runner=get('runner-identity.json')
        observation_name=f'trace-matrix/observations/p7-017-{profile}.json'
        observations=get(observation_name)
        reported=get('trace-matrix/verification.json')
        parent=get('trace-matrix/parent-policy.json')
        product_sha=records['product/codecortex']['sha256']
        need(receipt['binary_sha256']==product_sha and receipt['package_kind']==profile,'product/package bytes mismatch')
        need(receipt['source_before']==source==receipt['source_after']==runner['source_before']==runner['source_after']==reported['source'], 'build/verifier source mismatch')
        need(receipt['build_exit_code']==0 and receipt['build_profile']=='dev' and receipt['actual_cargo_profile']==receipt['cargo_artifact']['profile'], 'product build identity invalid')
        need(receipt['cargo_artifact']['features']==({'default':[],'semantic':['semantic']}[profile]),'package feature mismatch')
        need(receipt['builder_sha256']==SOURCES['files']['scripts/p7_stdio_build_receipt.py']['sha256'] and receipt['identity_helper_sha256']==SOURCES['files']['scripts/p7_build_identity.py']['sha256'],'builder identity mismatch')
        inputs=get('product/source-inputs.json')
        need(inputs==get('runner-source-inputs.json')==get('runner-source-before.json')['inputs'],'runner/product full input set mismatch')
        need(len(inputs)==source['input_count']==795 and hashlib.sha256(json_bytes(inputs)).hexdigest()==source['manifest_sha256'],'source manifest digest mismatch')
        need(inputs==SOURCE_SNAPSHOT['inputs'],'retained source manifest differs from fixed live/Git source')
        cargo_record(z,'product',receipt,'codecortex','bin')
        cargo_rows=[json.loads(line) for line in z.read('runner-build.jsonl').splitlines()]
        need([r.get('success') for r in cargo_rows if r.get('reason')=='build-finished']==[True],'test runner build did not finish')
        need([r for r in cargo_rows if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='benchmark_adapters']==[runner['cargo_artifact']],'test runner artifact mismatch')
        need(runner['cargo_artifact']['target']['kind']==['test'],'wrong test runner kind')
        need(parent['exec_sha256']==runner['sha256']==reported['runner_sha256'],'runner execution identity mismatch')
        need(parent['action']=='errno' and parent['result']=='filter_loaded_before_exec' and parent['socket_fds_before_exec']==[],'parent guard incomplete')
        need(parent['ipv4']==parent['ipv6']=={'blocked':True,'errno':1},'actual parent AF_INET/6 denial missing')
        need(int(parent['after']['Seccomp_filters'])==int(parent['before']['Seccomp_filters'])+1 and parent['after']['NoNewPrivs']=='1' and parent['after']['Seccomp']=='2','parent exact filter installation missing')
        need('pidfd_getfd' in parent['denied_syscalls'] and all(x in parent['denied_syscalls'] for x in ('socket','connect','bind','sendto','sendmsg','recvmsg','io_uring_setup','io_uring_enter','io_uring_register')),'network/FD import denial drift')
        rawdir=destination/'raw/trace-matrix/trace'
        trace_run=get('trace-matrix/trace/trace-run.json')
        need(trace_run==reported['trace_run'],'reported trace execution metadata mismatch')
        traces=raw_trace_check(rawdir,trace_run)
        recomputed=trace.verify_product_observations(observations,traces,product_sha)
        for key,value in recomputed.items():need(same(value,reported[key]),'formal trace replay differs: '+key)
        need(recomputed['status']=='passed' and recomputed['network_socket_attempts']==0 and recomputed['violations']==[], 'actual external network attempt')
        for field,member in [('build_receipt_sha256','product/build-receipt.json'),('runner_receipt_sha256','runner-identity.json'),('observations_sha256',observation_name)]:
            need(reported[field]==records[member]['sha256'],'verification payload binding mismatch: '+field)
        need(observations['build_receipt']==receipt,'adapter consumed different product receipt')
        need(reported['verifier_sha256']==SOURCES['files']['scripts/p7_offline_trace.py']['sha256'] and reported['guard_sha256']==SOURCES['files']['scripts/p7_offline_network.py']['sha256']==parent['wrapper_sha256'],'guard/verifier source mismatch')
        guard_hash=reported['guard_sha256']; python_hash=parent['launcher_python_sha256']
        for case in observations['cases']:
            need(case['package']==profile and case['child_environment']=='env_clear; fixture HOME/XDG paths; PATH only; no key','case environment/package mismatch')
            need(case['network_socket_attempts'] is None and case['network_attempt_basis'] is None,'parent-exit-only zero attempt claim')
            for guard in (case['child_network_guard'],case['reopened_network_guard']):
                need(guard['wrapper_sha256']==guard_hash and guard['launcher_python_sha256']==python_hash and guard['exec_sha256']==product_sha,'guard/product execution bytes mismatch')
        controls=get('trace-controls/controls.json')
        need(controls['status']=='passed' and controls['verifier_sha256']==reported['verifier_sha256'] and controls['guard_sha256']==guard_hash,'standalone control source drift')
        controls_replayed=[]
        for attack in (False,True):
            prefix='trace-controls/'+('child-socket-parent-zero' if attack else 'clean-child')
            ct=raw_trace_check(destination/'raw'/prefix,get(prefix+'/trace-run.json'))
            guard=get(prefix+'.json')
            tree=trace.inspect_tree(ct,trace.guard_root(guard,require_parent_policy=False),guard['exec_command'])
            row={'attack':attack,'parent_returncode':0,'tree':tree}
            need(same(row,controls['controls'][int(attack)]),'standalone control trace replay drift')
            need(tree['root_terminal']=={'exit_code':0} and bool(tree['attempts'])==attack,'parent-zero hidden child control failed')
            if attack:need(any(r.get('syscall')=='socket' and r['pid']!=tree['root_pid'] for r in tree['attempts']),'missing actual child network attempt')
            else:need(any(r['syscall']=='recvfrom' for r in tree['local_anonymous_ipc']),'missing real inode-bound receive positive IPC control')
            controls_replayed.append({'attack':attack,'root_pid':tree['root_pid'],'members':len(tree['processes_and_threads']),'attempts':tree['attempts'],'local_anonymous_ipc':tree['local_anonymous_ipc']})
        products=[]
        for t in recomputed['product_trees']:
            products.append({'root_pid':t['root_pid'],'explicitly_disabled':t['explicitly_disabled'],'reopened':t['reopened'],'processes_and_threads':len(t['processes_and_threads']),'terminal':t['root_terminal'],'external_network_attempts':len(t['attempts']),'local_anonymous_ipc_count':len(t['local_anonymous_ipc']),'local_anonymous_ipc':t['local_anonymous_ipc']})
        report={'schema_version':1,'status':'passed','source':source,'github_run_id':37729686665,'github_job_id':job,'artifact_id':artifact,'zip':{'path':str(zpath),'bytes':expected_size,'sha256':expected_sha,'members':len(records),'uncompressed_bytes':sum(v['bytes'] for v in records.values()),'all_members_streamed_and_crc_sha256_verified':True},'package':profile,'product_sha256':product_sha,'runner_sha256':runner['sha256'],'test_runner_binary_archived':False,'test_runner_identity_basis':'CI hashed the actual Cargo-reported executable before and after the traced matrix; retained Cargo JSONL and source receipts bind that witness','kernel':'complete process-local seccomp + complete timestamped per-PID strace; direct parse replay performed here','complete_matrix_trace_pids':len(traces),'products':products,'config_cases':len(observations['cases']),'tools_per_config':sorted(trace.TOOLS),'original_reopen_assertions':['status retains indexed file','search returns local source','persisted ADR is listed and deleted'],'external_network_attempts':0,'local_anonymous_ipc_count':sum(len(t['local_anonymous_ipc']) for t in recomputed['product_trees']),'positive_network_controls':recomputed['positive_controls_outside_product_trees'],'standalone_controls_replayed':controls_replayed,'source_binding_files':SOURCES['files'],'raw_members_preserved':selected,'task_ledger_mutated':False,'scope':'P7-017 original default/semantic local and explicitly-disabled tool contract with complete descendant network observation; prior task dependencies are decided separately'}
        save(destination/'zip-members.json',records)
        save(destination/'review.json',report)
        print(json.dumps({k:report[k] for k in ('package','status','complete_matrix_trace_pids','config_cases','external_network_attempts','local_anonymous_ipc_count')}),flush=True)
        return report

if __name__=='__main__':
    fixed_sources()
    paths=subprocess.check_output(['git','ls-tree','-r','--name-only',HEAD,'--','crates','Cargo.toml','Cargo.lock'],cwd=ROOT,text=True).splitlines()
    need(not any(re.search(r'(^|/)(?:hold[-_]?out|held[-_]?out)(?:/|\.)',p,re.I) for p in paths),'unexpected protected corpus path; audit will not read bodies')
    SOURCE_SNAPSHOT=source_snapshot(ROOT)
    source={k:v for k,v in SOURCE_SNAPSHOT.items() if k!='inputs'}
    need(source['source_commit']==HEAD and source['source_tree']=='9e59b41540eb3769e9ca0a787c9ed60441259d20','fixed source HEAD/tree drift')
    reports={profile:audit_package(profile,info,source) for profile,info in PACKAGES.items()}
    fixed_sources()
    save(OUT/'source-binding.json',SOURCES)
    save(OUT/'offline-review.json',{'schema_version':1,'status':'passed','source':source,'packages':reports,'external_network_attempts':0,'product_roots':8,'config_cases':4,'local_anonymous_ipc_count':sum(r['local_anonymous_ipc_count'] for r in reports.values()),'validation_script_sha256':file_sha256(__file__),'original_tool_assertions_preserved':True,'task_ledger_mutated':False})
