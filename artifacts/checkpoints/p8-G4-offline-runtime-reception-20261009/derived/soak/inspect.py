#!/usr/bin/env python3
"""Offline reception only: original G4 ZIP, no product/Cargo execution."""
from pathlib import Path, PurePosixPath
import ast
from collections import Counter
import hashlib
import json
import math
import stat
import subprocess
import zipfile
import re
import sys
import time
from cache_wire import transport, bind_compound_reads, bind_compound_reads_with_endpoint

HERE = Path(__file__).resolve().parent
REPO = '/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source'
G4 = '260f596582f2d82b8d7c707b61a6b8b6a43b069f'
RUN, JOB = 37854847820, 113576447137
def sha(b): return hashlib.sha256(b).hexdigest()
def require(ok, message):
    if not ok: raise AssertionError(message)

def main():
    expected=json.loads((HERE/'expected-artifact.json').read_bytes())
    require(expected['source']==G4 and expected['run_id']==RUN and expected['run_attempt']==1 and expected['job_id']==JOB, 'actual fixed G4 expected metadata required')
    api_run=json.loads((HERE/'run.json').read_bytes());api_jobs=json.loads((HERE/'jobs.json').read_bytes());api_artifacts=json.loads((HERE/'artifacts.json').read_bytes())
    require(api_run['id']==RUN and api_run['head_sha']==G4 and api_run['run_attempt']==1,'actual workflow identity')
    require(api_jobs['total_count']==len(api_jobs['jobs']),'complete jobs page')
    jobs=[j for j in api_jobs['jobs'] if j['id']==JOB]
    require(len(jobs)==1 and jobs[0]['name']=='soak' and jobs[0]['status']=='completed' and jobs[0]['conclusion']=='success' and jobs[0]['head_sha']==G4,'actual soak job success')
    require(api_artifacts['total_count']==len(api_artifacts['artifacts']),'complete artifacts page')
    artifacts=[a for a in api_artifacts['artifacts'] if a['id']==expected['artifact_id']]
    require(len(artifacts)==1 and artifacts[0]['name']=='p8-soak-'+G4 and artifacts[0]['workflow_run']['id']==RUN and artifacts[0]['workflow_run']['head_sha']==G4 and artifacts[0]['expired'] is False,'actual artifact role/source')
    require(artifacts[0]['size_in_bytes']==expected['bytes'] and artifacts[0]['digest']=='sha256:'+expected['sha256'],'actual GitHub artifact bytes')
    job_log=(HERE/f'job-{JOB}.log').read_text()
    require('git checkout --progress --force '+G4 in job_log and re.search(r'git log -1 --format=%H\n[^\n]*'+G4,job_log),'actual fixed-head checkout')
    archive = HERE/f"artifact-{expected['artifact_id']}.zip"
    ZIP_SHA=expected['sha256']
    with archive.open('rb') as f: archive_hash=hashlib.file_digest(f,'sha256').hexdigest()
    require(archive.stat().st_size == expected['bytes'] and archive_hash == ZIP_SHA, 'GitHub ZIP digest')
    output=HERE/'independent-review-01';output.mkdir()
    inventory = {}
    with zipfile.ZipFile(archive) as z:
        for info in z.infolist():
            require(not info.is_dir(), 'unexpected directory entry')
            path = PurePosixPath(info.filename)
            require(not path.is_absolute() and '..' not in path.parts and path.as_posix()==info.filename and '\\' not in info.filename and not info.flag_bits&1 and info.filename not in inventory, 'unsafe duplicate member')
            mode = info.external_attr >> 16
            require(stat.S_IFMT(mode) in (0, stat.S_IFREG), 'nonregular ZIP member')
            h = hashlib.sha256(); size = 0
            with z.open(info) as f:
                while block := f.read(1024*1024): h.update(block); size += len(block)
            require(size == info.file_size, 'ZIP byte count')
            inventory[info.filename] = {'bytes':size,'sha256':h.hexdigest(),'crc32':format(info.CRC,'08x')}
        require(len(inventory) == 2125, 'member population')
        def read(name): return json.loads(z.read(name))
        for prefix, count in [('p8-build/',15),('p8-runtime/',2108)]:
            seal = read(prefix+'seal.json')['artifact_inventory']
            actual = {n[len(prefix):]:{k:v for k,v in r.items() if k!='crc32'} for n,r in inventory.items()
                      if n.startswith(prefix) and n != prefix+'seal.json'}
            require(seal == actual and len(seal)==count, 'complete seal '+prefix)
        plan, report = read('p8-runtime/plan.json'),read('p8-runtime/report.json')
        build = read('p8-build/build-receipt.json')
        source = read('p8-build/source-before.json')
        require(source == read('p8-build/source-after.json') == build['source_before'] == build['source_after'], 'source equality')
        require(source['source_commit']==G4 and source['source_tree']=='645404431ca15770d6e729785a3ada68b096c593' and source['input_count']==len(source['inputs'])==1087, 'G4 source scope')
        # Batch read the immutable Git blobs, then recompute every source SHA.
        batch = subprocess.Popen(['git','-C',REPO,'cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
        for path, digest in source['inputs'].items():
            batch.stdin.write((G4+':'+path+'\n').encode()); batch.stdin.flush()
            head = batch.stdout.readline().decode().split(); require(len(head)==3 and head[1]=='blob','missing fixed source')
            body = batch.stdout.read(int(head[2])); require(batch.stdout.read(1)==b'\n','Git framing')
            require(sha(body)==digest,'fixed Git source differs '+path)
        batch.stdin.close(); require(batch.wait()==0,'Git reader exit')
        require(build['observer_before']==build['observer_after'], 'observer before/after')
        observer = build['observer_before']; require(observer['source_commit']==G4,'observer commit')
        for path, identity in observer['files'].items():
            b=z.read('p8-build/observer-source/'+path)
            committed=subprocess.check_output(['git','-C',REPO,'show',G4+':'+path])
            require(b==committed and len(b)==identity['bytes'] and sha(b)==identity['sha256'], 'observer body')
        require(len(observer['files'])==7,'observer count')
        require(z.read('p8-build/build-receipt.json') == z.read('p8-runtime/build-evidence/build-receipt.json'),'retained build copy')
        require(plan['build_identity']==report['final_build_verification'],'final build identity recheck')
        require(plan['build_identity']['source']==source and plan['build_identity']['observer']==observer and plan['build_identity']['artifacts']==build['artifacts'],'full initial identity')
        require(plan['build_identity']['build_seal_sha256']==sha(z.read('p8-build/seal.json')),'build seal identity')
        for name,row in plan['retained_build_evidence'].items():
            require(z.read('p8-build/'+name)==z.read('p8-runtime/build-evidence/'+name) and row=={k:v for k,v in inventory['p8-build/'+name].items() if k!='crc32'},'all original retained build proof copies')
        require(plan['build_identity']['receipt_sha256']==sha(z.read('p8-build/build-receipt.json')),'build receipt binding')
        cargo=[json.loads(line) for line in z.read('p8-build/product-build.jsonl').splitlines() if line]
        require(cargo[-1]=={'reason':'build-finished','success':True} and build['build_exit_code']==0,'Cargo completion')
        require(build['schema_version']==2 and build['status']=='passed','strict builder schema/status')
        require(build['build_command']==['cargo','build','--release','--locked','--offline','--no-default-features','-p','cc-server','--bin','codecortex','-p','cc-eval','--bin','p8-oracle','--bin','p8-runtime-statistics','--message-format=json-render-diagnostics','--target-dir',build['target_dir']],'original exact Cargo argv')
        require(sha(z.read('p8-build/product-build.jsonl'))==build['cargo_log_sha256'] and sha(z.read('p8-build/product-build.stderr'))==build['stderr_sha256'],'both Cargo log hashes')
        targets={'codecortex':('cc-server','src/main.rs','binary'),'p8-oracle':('cc-eval','src/bin/p8-oracle.rs','oracle'),'p8-runtime-statistics':('cc-eval','src/bin/p8-runtime-statistics.rs','statistics')}
        require(set(build['artifacts'])==set(targets),'original three release artifacts')
        for name, identity in build['artifacts'].items():
            a=identity['cargo_artifact']; matches=[m for m in cargo if m.get('reason')=='compiler-artifact' and m.get('target',{}).get('name')==name]
            require(matches==[a] and a['profile']['opt_level']=='3' and not a['profile']['test'] and not a['profile']['debug_assertions'],'Cargo selected release')
            b=z.read('p8-build/'+name); require(len(b)==identity['binary_bytes']==identity['copy_source']['bytes'] and sha(b)==identity['binary_sha256']==identity['copy_source']['sha256'],'binary copy chain')
            package,relative,field=targets[name]
            require(a['target']['kind']==['bin'] and a['features']==[] and a['executable']==build['target_dir']+'/release/'+name==identity['copy_source']['path'] and a['manifest_path']=='/home/runner/work/codecortex/codecortex/crates/'+package+'/Cargo.toml' and a['target']['src_path']=='/home/runner/work/codecortex/codecortex/crates/'+package+'/'+relative,'exact producer paths/profile')
            require(identity['binary_sha256']==build[field+'_sha256'],'binary role binding')
        require(plan['profile']=='soak' and plan['operations']==3601 and plan['offer_interval_ms']==1000 and plan['offer_schedule']=='uniform' and plan['files']==1000 and plan['concurrency']==4,'original soak plan')
        require(report['status']=='passed_observation' and report['exit_code']==0 and report['failures']==[] and report['artifact_seal_status']=='sealed','successful terminal report')
        require(report['task_complete'] is False and report['release_approval'] is False,'scope guard')
        for file,key in [('plan.json','plan_sha256'),('raw.jsonl','raw_sha256'),('parity.json','parity_sha256')]:
            require(inventory['p8-runtime/'+file]['sha256']==report[key], 'report raw binding')
        records=[json.loads(line) for line in z.read('p8-runtime/raw.jsonl').splitlines() if line]
        rows=[r for r in records if r['kind']=='operation']; samples=[r for r in records if r['kind']=='resources']
        require(len(rows)==3601 and sorted(r['id'] for r in rows)==list(range(3601)), 'complete operation IDs')
        require(Counter(r['status'] for r in rows)=={'success':3601},'terminal states')
        operations=Counter(r['operation'] for r in rows);require(operations=={'read':2400,'build':1201},'operation denominator')
        builds=[r for r in rows if r['operation']=='build']; require(sorted(r['mutation_ordinal'] for r in builds)==list(range(1201)), 'mutation admission order')
        for r in rows:
            require(r['finished_ns']>=r['call_started_ns']>=r['offered_ns'],'nonnegative observation clocks')
            if r['operation']=='read':require(any(h.get('name')=='p8_runtime_stable_signal' and h.get('file_path')=='stable.py' for h in r['response']),'actual stable public hit')
        # Execute only fixed, pure summary functions; no observer run/main or product.
        text=z.read('p8-build/observer-source/scripts/p8_runtime.py').decode();tree=ast.parse(text)
        names={'median','rss_trend','sample_coverage','latency_summary','owned','require_stable_symbol','cache_identity','cache_lookup','require_stable_hybrid','validate_cache_probe','soak_cache_summary'}
        constants={'MARKER','QUERY','CACHE_COUNTERS','SOAK_READ_PROTOCOL','SOAK_READ_ROLES'}
        assignments=[n for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id in constants for t in n.targets)]
        pure=ast.Module(body=assignments+[n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name in names],type_ignores=[])
        ns={'math':math,'Counter':Counter,'hashlib':hashlib};rollback=ast.parse(z.read('p8-build/observer-source/scripts/p8_rollback.py'));original_require=next(n for n in rollback.body if isinstance(n,ast.FunctionDef) and n.name=='require');exec(compile(ast.Module(body=[original_require],type_ignores=[]),'fixed_G4_require','exec'),ns);exec(compile(pure,'fixed_G4_pure_summaries','exec'),ns)
        require(ns['latency_summary'](rows)==report['latency'],'all raw quantiles')
        for op in operations:require(ns['latency_summary']([r for r in rows if r['operation']==op])==report['latency_by_operation'][op], 'operation quantiles')
        require(ns['rss_trend'](samples)==report['rss'] and report['rss']['passed'],'native RSS replay')
        cov=report['resource_time_coverage'];require(ns['sample_coverage'](samples,cov['work_start_ns'],cov['work_end_ns'])==cov and cov['passed'],'temporal resource coverage')
        # The report reads the monotonic clock after drain; coverage reads it
        # again. These distinct readings are not falsely forced byte-equal.
        raw_work_span=max(r['finished_ns'] for r in rows)-min(r['offered_ns'] for r in rows)
        require(cov['work_end_ns']-cov['work_start_ns']>=report['observed_work_ns']>=raw_work_span>=3600*10**9,'whole hour work and ordered terminal/drain observations')
        switches=sum(r.get('mutation',{}).get('action')=='real_git_branch_switch' for r in rows)
        compactions=z.read('p8-runtime/product/product-stderr.log').decode().count('resolver catalog dropped for tombstone compaction')
        require(switches==report['real_branch_switches']==200 and compactions==report['observed_catalog_compactions']==25,'real branch/compaction receipts')
        parity=read('p8-runtime/parity.json');comparison=parity['comparison']
        require(parity['exit_code']==report['parity_exit_code']==0 and parity['error'] is None and comparison['equal'] and comparison['different_tables']==[],'parity original receipt')
        require(len(comparison['tables'])==len({r['table'] for r in comparison['tables']})==15,'full15table oracle')
        require(all(r['equal'] and r['different_row_count']==0 and r['full_rows']==r['incremental_rows'] and r['full_digest']==r['incremental_digest'] for r in comparison['tables']),'parity all tables')
        statistics=read('p8-runtime/statistics.json');require(z.read('p8-runtime/statistics.json')==z.read('p8-runtime/statistics-replay.json'),'two retained Rust statistics bytes')
        require(statistics['recorded_samples']==statistics['expected_samples']==3601 and statistics['missing_samples']==statistics['unexpected_samples']==0,'Rust denominator')
        require({x['operation']:x['recorded_samples'] for x in statistics['by_operation']}==dict(operations),'Rust operation count')
        require(statistics['raw_sha256']==report['raw_sha256'] and statistics['plan_sha256']==report['plan_sha256'],'Rust input hashes')
        project=output/'archived-stable-fixture';project.mkdir()
        for name in ('.p8-owned','stable.py'):
            b=z.read('p8-runtime/project/'+name)
            require(b==z.read('p8-runtime/fresh-full/'+name),'original endpoint stable fixture bytes differ')
            with (project/name).open('xb') as f:f.write(b)
        cache=ns['soak_cache_summary'](rows,3601,cov['work_start_ns'],cov['work_end_ns'],project)
        require(cache==report['cache_reuse'] and cache['passed'] and cache['validated_reads']==2400,'full exact original cache summary replay')
        require(cache['request_counts']=={role:{'success':2400} for role in ns['SOAK_READ_ROLES']} and cache['status_request_counts']=={'success':4800},'9600 original cache RPC denominator')
        protocol=plan['read_protocol']
        require(protocol['name']==ns['SOAK_READ_PROTOCOL'] and protocol['offered_read_operations']==2400 and protocol['planned_request_counts']=={role:2400 for role in ns['SOAK_READ_ROLES']} and protocol['request_roles']==list(ns['SOAK_READ_ROLES']) and protocol['strategy']=='local','original compound read plan')
        require(protocol['rpc_timeout_seconds']==dict(before_status=30,symbol=60,hybrid=60,after_status=30) and protocol['rpc_timeout_sum_seconds']==180,'per-RPC budgets; no new whole-read SLA')
        (output/'cache-replay.json').write_text(json.dumps(cache,sort_keys=True,indent=2)+'\n')
        replay_binary=output/'original-p8-runtime-statistics'
        with replay_binary.open('xb') as f:f.write(z.read('p8-build/p8-runtime-statistics'))
        replay_binary.chmod(0o555)
        for name in ('plan.json','raw.jsonl'):
            with (output/name).open('xb') as f:f.write(z.read('p8-runtime/'+name))
        command=[str(replay_binary),'--plan',str(output/'plan.json'),'--raw',str(output/'raw.jsonl'),'--output',str(output/'statistics.json')]
        (output/'statistics-command.json').write_text(json.dumps(command)+'\n')
        started=time.monotonic()
        with (output/'statistics.stdout').open('xb') as stdout,(output/'statistics.stderr').open('xb') as stderr:
            replayed=subprocess.run(command,stdout=stdout,stderr=stderr,timeout=120)
        require(replayed.returncode==0 and (output/'statistics.json').read_bytes()==z.read('p8-runtime/statistics.json'),'actual offline retained Rust statistics replay')
        require(sha(replay_binary.read_bytes())==build['statistics_sha256'] and all(sha((output/name).read_bytes())==inventory['p8-runtime/'+name]['sha256'] for name in ('plan.json','raw.jsonl')),'offline replay inputs/binary unchanged')
        statistics_replay=dict(argv=command,exit_code=replayed.returncode,seconds=time.monotonic()-started,byte_identical=True)
        # Stream all original transport JSON, ensuring both owned sessions close cleanly.
        transports={};cache_wire=None;sessions={}
        for prefix in ['product','full-product']:
            events=(json.loads(line) for line in z.open('p8-runtime/'+prefix+'/rpc.jsonl'))
            requests,responses,times,kinds=transport(events)
            sessions[prefix]=(requests,responses,times)
            process=read('p8-runtime/'+prefix+'/process.json')
            require(process['exit_code']==0 and process['cleanup']=='completed' and process['tool_count']==14 and process['binary_sha256']==build['binary_sha256'],'original process closure')
            transports[prefix]={'kinds':kinds,'process_exit_0_observed':True,'stdout_eof_observed':True}
        requests,responses,times=sessions['product']
        process=read('p8-runtime/product/process.json')
        cache_wire=bind_compound_reads_with_endpoint(rows,requests,responses,times,records,*sessions['full-product'])
        require(cache_wire['compound_reads']==2400 and cache_wire['bound_RPCs']==9600,'all compound reads bound to original stdio')
        require(all(r['cache_probe']['server_pid']==process['pid'] for r in rows if r['operation']=='read'),'cache probes from actual owned server')
        indexes=[p for p in requests.values() if p.get('method')=='tools/call' and p.get('params',{}).get('name')=='index']
        require(len(indexes)==1202 and indexes[0]['params']['arguments']['full'] is True and all(p['params']['arguments']['full'] is False for p in indexes[1:]),'initialfull+1201increments; no endpoint repair')
        (output/'cache-wire-binding.json').write_text(json.dumps(cache_wire,sort_keys=True,indent=2)+'\n')
        endpoint=next(r['response'] for r in records if r['kind']=='endpoint_status')
        result={'decision':'accepted_scoped_original_G4_soak_observation','source_commit':G4,'run_id':RUN,'job_id':JOB,'artifact_id':expected['artifact_id'],'artifact_sha256':ZIP_SHA,'artifact_bytes':expected['bytes'],'all_member_count':len(inventory),'all_member_bytes':sum(x['bytes'] for x in inventory.values()),'all_member_CRC_SHA_verified':True,'run_seal_files':2108,'build_seal_files':15,'source_inputs_exact_Git':1087,'observer_files_exact_Git':7,'operations':dict(operations),'outcomes':{'success':3601},'observed_work_ns':report['observed_work_ns'],'rss':report['rss'],'resource_coverage':cov,'branch_switches':switches,'catalog_compactions':compactions,'configured_C':4,'observed_workload_peak':report['actual_concurrency']['maximum'],'parity_tables':15,'original_parity_exit_code':0,'statistics_replay':statistics_replay,'cache_reuse':cache,'bound_cache_RPCs':cache_wire['bound_RPCs'],'endpoint_search_cache':endpoint['search_cache'],'transport':transports,'limitations':['Offline receipt/raw replay only; no product/Cargo/new performance measurement. Original full parity evidence retained without rerunning product or oracle.','Compound read is one offered sample containing two status, symbol and local hybrid calls. All 9600 subrequests stay within 2400 read samples.','Same native server and bounded shared query-pool counters are observed; no individual OS-thread, semantic-worker, provider or actual C4-peak claim.','Per-RPC timeout sum is180 seconds excluding admission wait; no new performance SLA. Uniform soak timings are not directly comparable to previous symbol-only reads.','Original IID intervals retain serial-correlation limitations; original Rust statistics replay is byte-identical without creating a new estimator.'],'TODO_closed':0,'TODO_remaining':29}
        with archive.open('rb') as f:require(hashlib.file_digest(f,'sha256').hexdigest()==ZIP_SHA,'original ZIP unchanged after replay')
        (HERE/'member-hashes.json').write_text(json.dumps(inventory,sort_keys=True,indent=2)+'\n')
        (HERE/'inspection.json').write_text(json.dumps(result,sort_keys=True,indent=2)+'\n')
        print(json.dumps({'accepted':True,'inspection_sha256':sha((HERE/'inspection.json').read_bytes()),'operations':dict(operations),'transport':transports}))

if __name__=='__main__':main()
