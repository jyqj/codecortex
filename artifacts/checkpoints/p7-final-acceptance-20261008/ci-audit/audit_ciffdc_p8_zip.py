#!/usr/bin/env python3
"""Audit the original CI P8 freeze/execution/archive without executing binaries."""
import sys
sys.dont_write_bytecode=True
import hashlib
import json
import subprocess
import zipfile
from pathlib import Path
from audit_ciffdc_offline_zip import ROOT,HEAD,OUT,ZIPROOT,SOURCES,need,save,fixed_sources,cargo_record
sys.path.insert(0,str(ROOT/'scripts'))
import p8_release_evidence as lock
from p7_build_identity import file_sha256,json_bytes

fixed_sources()
zip_path=ZIPROOT/'github-actions-artifact-11529447454.zip'
need(file_sha256(zip_path)=='21e15185fc7e0fe917d1253a283026b20e69266fcd75768cff0f2012632aef68','original ZIP changed')
records=json.loads((OUT/'default/zip-members.json').read_text())
source=json.loads((OUT/'offline-review.json').read_text())['source']
destination=OUT/'p8'
destination.mkdir(parents=True,exist_ok=False)
with zipfile.ZipFile(zip_path) as z:
    base='p8-candidate-execution/'
    def get(name):return json.loads(z.read(name))
    def prefixed(prefix):return {name[len(prefix):]:record for name,record in records.items() if name.startswith(prefix)}
    def manifest(name,kind,field,expected=None):
        result=get(name)
        need(result['kind']==kind and result['scope']=='local_engineering_only' and result['schema_version']==1 and result['release_certified'] is False,'manifest scope/schema drift')
        need(lock.digest({k:v for k,v in result.items() if k!=field})==result[field],'manifest digest mismatch')
        need(expected is None or expected==result[field],'trusted manifest pin mismatch')
        return result
    result=get(base+'result.json')
    need(result['status']=='executed_frozen_candidate' and result['measured_rows']==1 and result['actual_build_profile']=='dev' and result['release_certified'] is False,'actual result claim mismatch')
    pin=result['candidate_sha256']
    need(pin=='aed4dffedaa7e73c0f4fe355d1f89616eaf197bdbed70be44dd1ba5f9c0649cd','CI log candidate pin mismatch')
    candidate=manifest(base+'candidate/candidate.json','p8_local_candidate','candidate_sha256',pin)
    candidate_prefix=base+'candidate/'
    need(candidate['source']['head']==HEAD and candidate['source']['tree']==source['source_tree'],'candidate source HEAD/tree mismatch')
    need(candidate['build']=={'features':[],'profile':'debug','provenance':'operator_declaration_not_build_proof'} and candidate['model']=={'mode':'disabled'},'candidate build/model declaration mismatch')
    expected={'candidate.json'}
    for role,record in candidate['inputs'].items():
        need(record['snapshot']=='inputs/'+role,'candidate role path mismatch')
        relative=record['snapshot'];expected.add(relative)
        need(records[candidate_prefix+relative]=={k:record[k] for k in ('sha256','bytes','executable')},'candidate input bytes mismatch')
    need(set(candidate['inputs'])=={'binary','config','corpus','scoring','model'},'candidate input roles missing')
    source_entries={}
    raw=subprocess.check_output(['git','ls-tree','-r','-z',HEAD,'--',*candidate['source']['prefixes']],cwd=ROOT)
    for row in raw.split(b'\0'):
        if not row:continue
        metadata,name=row.split(b'\t',1);mode,kind,oid=metadata.decode().split()
        need(kind=='blob' and mode in ('100644','100755'),'nonregular fixed source entry')
        source_entries[name.decode()]={'mode':mode,'oid':oid}
    need(set(source_entries)==set(candidate['source']['entries']),'candidate fixed source inventory mismatch')
    for relative,record in candidate['source']['entries'].items():
        need(record['head']==record['index']==source_entries[relative],'candidate index/head differs from fixed Git source')
        name='source/'+relative;expected.add(name)
        need(records[candidate_prefix+name]==record['content'],'candidate source snapshot SHA/mode mismatch')
        data=z.read(candidate_prefix+name)
        blob=hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest()
        need(blob==source_entries[relative]['oid'],'candidate source bytes differ from committed Git blob')
    corpus=candidate['corpus_content']
    need(corpus['status']=='verified_explicit_corpus_files' and len(corpus['files'])==4 and lock.digest(corpus['files'])==corpus['content_sha256'],'explicit original corpus inventory mismatch')
    for relative,record in corpus['files'].items():
        name='corpus/'+relative;expected.add(name)
        need(records[candidate_prefix+name]==record,'candidate corpus bytes/mode mismatch')
        original=subprocess.check_output(['git','show',HEAD+':'+relative],cwd=ROOT)
        need(z.read(candidate_prefix+name)==original,'original P0 corpus bytes changed')
    need(set(prefixed(candidate_prefix))==expected,'candidate has missing/extra files')
    arch_dir=result['archive']['archive'].rsplit('/',1)[-1]
    arch_prefix=base+'archives/'+arch_dir+'/'
    archive=manifest(arch_prefix+'archive.json','p8_local_archive','archive_sha256',result['archive']['archive_sha256'])
    need(archive['archive_sha256']=='ec53990459cd8aedbc34573d33a966e8c96000d975033e34bef3aca834672ddb','CI log archive pin mismatch')
    need(set(prefixed(arch_prefix))==set(archive['files'])|{'archive.json','checksums.sha256'},'archive payload inventory mismatch')
    for name,record in archive['files'].items():need(records[arch_prefix+name]==record,'archive member hash/mode mismatch')
    need(prefixed(arch_prefix+'candidate/')==prefixed(candidate_prefix),'archived candidate differs from original frozen candidate')
    need(prefixed(arch_prefix+'evidence/')==prefixed(base+'evidence/'),'archived evidence differs from retained original evidence')
    checksums=''.join(record['sha256']+'  '+name+'\n' for name,record in sorted(archive['files'].items()))
    checksums+=records[arch_prefix+'archive.json']['sha256']+'  archive.json\n'
    need(z.read(arch_prefix+'checksums.sha256')==checksums.encode(),'original archive checksum list mismatch')
    witness=get(base+'evidence/execution-witness.json')
    scoring=get(candidate_prefix+'inputs/scoring')
    environment=get(base+'environment.json')
    need(witness['source']==source and witness['candidate_sha256']==pin and witness['actual_build_profile']=='dev' and witness['actual_features']==[] and witness['release_certified'] is False,'execution witness source/build mismatch')
    need(witness['environment']==environment==scoring['environment'] and environment['provider']=={'mode':'disabled','live_cost':None,'live_quality_certification':'not_run'},'execution environment/model mismatch')
    need(get(candidate_prefix+'inputs/model')=={'mode':'disabled'} and scoring['scoring']=='codecortex-native-v1','original model/scorer profile mismatch')
    need(scoring['driver_sha256']==SOURCES['files']['scripts/p8_candidate_execution.py']['sha256'],'executed driver source hash mismatch')
    for name,digest in scoring['implementation'].items():need(candidate['source']['entries'][name]['content']['sha256']==digest,'scorer code snapshot mismatch')
    builds={}
    for role,binary_name,package in [('product','codecortex','cc-server'),('runner','cc-eval','cc-eval')]:
        prefix=base+'evidence/builds/'+role
        receipt=get(prefix+'/build-receipt.json')
        proof=scoring['builds'][role]
        need(receipt==proof['receipt'] and receipt['source_before']==source==receipt['source_after'],'source-bound build receipt mismatch')
        need(receipt['build_profile']=='dev' and receipt['build_exit_code']==0 and receipt['actual_cargo_profile']==receipt['cargo_artifact']['profile'],'actual build profile mismatch')
        artifact=receipt['cargo_artifact']
        need(artifact['features']==[] and artifact['target']['name']==binary_name and artifact['target']['kind']==['bin'] and artifact['profile']['test'] is False,'wrong built default binary artifact')
        need(artifact['manifest_path']==candidate['source_root']+'/crates/'+package+'/Cargo.toml' and artifact['package_id'].rpartition('#')[0].endswith('/'+package),'artifact from wrong source checkout/package')
        command=receipt['build_command']
        need(command[:2]==['cargo','build'] and '--locked' in command and '--no-default-features' in command and '--release' not in command and '--profile' not in command and command[command.index('--bin')+1]==binary_name,'actual build command/profile drift')
        cargo_record(z,prefix,receipt,binary_name,'bin')
        need(get(prefix+'/source-inputs.json')==get('product/source-inputs.json'),'build source manifest bytes mismatch')
        binary_sha=records[prefix+'/'+binary_name]['sha256']
        need(binary_sha==receipt['binary_sha256']==proof['binary_sha256']==witness[role+'_sha256'],'execution binary bytes mismatch')
        need(records[prefix+'/build-receipt.json']['sha256']==proof['receipt_sha256']==witness['build_receipts'][role],'build receipt hash mismatch')
        need(records[prefix+'/cargo-build.jsonl']['sha256']==proof['raw_build_sha256'] and records[prefix+'/source-inputs.json']['sha256']==proof['source_manifest_sha256'],'build raw/source digest mismatch')
        need(receipt['toolchain']==get('product/build-receipt.json')['toolchain'],'actual compiler identity differs across product/scorer')
        builds[role]={'binary_sha256':binary_sha,'receipt_sha256':proof['receipt_sha256'],'profile':receipt['build_profile'],'features':artifact['features'],'cargo_target_initially_absent':receipt['cargo_target_initially_absent'],'cargo_artifact_fresh':artifact['fresh']}
    need(builds['product']['binary_sha256']==records['product/codecortex']['sha256']==candidate['inputs']['binary']['sha256'],'frozen product differs from offline package')
    need(builds['runner']['binary_sha256']==records['p8-runner-build/cc-eval']['sha256'],'frozen scorer differs from originally built scorer')
    execution={}
    for label in ('validate','measurement','replay'):
        prefix=base+'evidence/commands/'+label
        command=get(prefix+'/execution.json')
        need(command['exit_code']==0 and 'timed_out' not in command and command['executable_sha256']==builds['runner']['binary_sha256'],'actual command did not execute fixed scorer successfully')
        need(command['cwd']==candidate['source_root'] and command['command'][0].endswith('/p8-candidate-execution/evidence/builds/runner/cc-eval'),'command used different scorer/cwd')
        for name,digest in command['raw_files'].items():need(records[prefix+'/'+name]['sha256']==digest,'command raw log digest mismatch')
        execution[label]=command
    need(execution['measurement']==witness['measurement_execution'] and execution['replay']==witness['replay_execution'],'witness/execution command drift')
    command=execution['measurement']['command']
    need(command[1:4]==['run','--backend','mcp-stdio'] and command[command.index('--binary')+1].endswith('/candidate/inputs/binary') and command[command.index('--suite')+1].endswith('/candidate/corpus/crates/cc-eval/benchmarks/manifests/p0-rust-api.json'),'measurement did not use the frozen product/suite')
    need(execution['replay']['command'][1]=='replay' and execution['replay']['command'][-1].endswith('/evidence/replay'),'replay did not use separate result copy')
    measurement_prefix=base+'evidence/measurement/'
    manifest_result=get(measurement_prefix+'manifest.json')
    gate=get(measurement_prefix+'gate.json')
    suite=get(candidate_prefix+'corpus/crates/cc-eval/benchmarks/manifests/p0-rust-api.json')
    need(manifest_result['suite']==suite and manifest_result['adapter']=='mcp-stdio' and manifest_result['infrastructure_failure'] is None,'actual MCP suite/transport incomplete')
    need(manifest_result['engine']['engine_head_observed']==HEAD and manifest_result['engine']['dirty_observed']=='' and manifest_result['engine']['eval_debug_assertions'] is True,'actual measured source/build identity drift')
    need(suite['repetitions']==1 and suite['seed']==27 and suite['timeout_ms']==30000 and suite['warmup']==0 and suite['top_k']==10 and suite['scoring']=='codecortex-native-v1','original fixture budget/scorer drift')
    queries=[json.loads(line) for line in z.read(measurement_prefix+'queries.jsonl').splitlines()]
    normalized=[json.loads(line) for line in z.read(measurement_prefix+'normalized.jsonl').splitlines()]
    need(len(queries)==len(normalized)==witness['measured_rows']==1 and all(q['split']=='dev' for q in queries),'planned original DEV request denominator mismatch')
    need(normalized[0]['case_id']==queries[0]['id'] and normalized[0]['repetition']==0 and normalized[0]['status']=='success' and normalized[0]['raw_path'] in prefixed(measurement_prefix),'actual raw MCP request incomplete')
    need(gate==witness['original_gate']=={'status':'baseline_recorded_not_quality_certified','exit_code':0,'reasons':[]},'original measurement gate altered')
    need(prefixed(measurement_prefix)==witness['original_measurement_files'],'original measured raw changed after run/replay/drift checks')
    replay_equal={}
    for name in ('metrics.json','gate.json','report.md'):
        need(records[measurement_prefix+name]==records[base+'evidence/replay/'+name],'original scorer replay result bytes differ')
        replay_equal[name]=records[measurement_prefix+name]
    metrics=get(measurement_prefix+'metrics.json')
    need(metrics['queries']==metrics['measured_rows']==1 and metrics['invalid_hits']==metrics['unverified_hits']==0,'original metric denominator/evidence mismatch')
    controls=get(base+'evidence/drift-controls/results.json')
    expected_controls={'binary','config','scoring','model','source','query-and-embedded-gold','corpus-source'}
    need(len(controls)==7 and {c['case'] for c in controls}==expected_controls and controls==witness['drift_controls'],'seven distinct original drift controls missing')
    for row in controls:
        need(row['rejected'] is True and row['before_sha256']!=row['after_sha256'] and row['before_sha256']==records[candidate_prefix+row['changed_path']]['sha256'] and row['error'],'drift control lacks changed bytes/rejection')
    wrapper=get(base+'evidence/gate.json')
    need({k:wrapper[k] for k in gate}==gate and wrapper['candidate_sha256']==pin and wrapper['original_gate_sha256']==records[measurement_prefix+'gate.json']['sha256'] and wrapper['execution_witness_sha256']==records[base+'evidence/execution-witness.json']['sha256'],'archive gate no longer preserves original measurement gate')
    need(archive['gate']=={k:wrapper[k] for k in ('candidate_sha256','status','exit_code')}==result['archive']['gate'] and result['archive']['latest_updated'] is False,'archive/result gate mismatch or unsupported promotion')
    preserved=[]
    for name,record in records.items():
        keep=(name.startswith('p8-runner-build/') and name!='p8-runner-build/cc-eval')
        keep|=name.startswith(base) and not name.startswith((base+'candidate/source/',base+'archives/')) and record['bytes']<2_000_000
        keep|=name in (arch_prefix+'archive.json',arch_prefix+'checksums.sha256')
        if keep:
            target=destination/'raw'/name;target.parent.mkdir(parents=True,exist_ok=True)
            with target.open('xb') as stream:stream.write(z.read(name))
            preserved.append(name)
    report={'schema_version':1,'status':'passed','github_run_id':37729686665,'github_job_id':113155755350,'artifact_id':11529447454,'source':source,'zip_sha256':file_sha256(zip_path),'candidate_sha256':pin,'candidate_source_entries':len(source_entries),'candidate_file_count':len(expected),'candidate_source_bytes_match_every_Git_blob':True,'archive_sha256':archive['archive_sha256'],'archive_payload_files':len(archive['files']),'archive_checksums_and_complete_candidate_evidence_bytes_verified':True,'builds':builds,'actual_environment':environment,'execution':execution,'measured_queries':len(queries),'measured_rows':len(normalized),'metrics':metrics,'original_gate':gate,'replay_byte_equal_outputs':replay_equal,'original_measurement_inventory_unchanged':True,'drift_controls':controls,'model':{'mode':'disabled'},'release_certified':False,'latest_updated':False,'raw_members_preserved':preserved,'validation_script_sha256':file_sha256(__file__),'reviewer':'pr_audit; independent reviewer of P8 driver, artifact review distinct from source review','scope':'P8-001 actual frozen default DEV product and original scorer execution/replay; only the original one-question fixture and original gate','unexecuted_certifications':witness['unexecuted_certifications'],'task_ledger_mutated':False,'task_dependencies':'P8-001 still requires P7-020 closure by the main task ledger owner'}
    save(destination/'review.json',report)
    print(json.dumps({k:report[k] for k in ('status','candidate_sha256','archive_sha256','archive_payload_files','measured_queries','measured_rows','original_gate','release_certified')}),flush=True)
fixed_sources()
