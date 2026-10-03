#!/usr/bin/env python3
"""Prepare immutable public-dev inputs, then run/replay fixed default MCP binaries."""
import argparse
from collections import Counter
from datetime import datetime,timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

HERE=Path(__file__).resolve().parent
OWNER=HERE.parent/'protocol/global-dev-review'
REPO=Path('/workspace/codecortex')
SOURCE='88f2cf099c8b81f3acef485fd5ac9b01c63ce790'
ADMISSION=OWNER/'typescript-extension/admission.json'
sha=lambda b:hashlib.sha256(b).hexdigest()
canon=lambda d:json.dumps(d,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()


def frozen_blob(entry,locks):
    if entry not in locks:raise ValueError('INPUT_NOT_ADMITTED')
    raw=subprocess.check_output(['git','show',entry],cwd=REPO,stderr=subprocess.DEVNULL)
    if sha(raw)!=locks[entry]:raise ValueError('INPUT_HASH_DRIFT')
    return raw


def verify_build(path):
    r=json.loads(path.read_bytes())
    if r['source_sha']!=SOURCE or r['build_exit_code']!=0:raise ValueError('WRONG_EXECUTION_SOURCE')
    for name in ['cc-eval','codecortex']:
        a=r['artifacts'][name]
        if sha(Path(a['copied_binary']).read_bytes())!=a['binary_sha256']:raise ValueError('BINARY_HASH_DRIFT')
        if any(f in a['features'] for f in ['semantic','eval-http']):raise ValueError('NETWORK_FEATURE_ENABLED')
    return r


def prepare(inputs,build_path,plan_path):
    if inputs.exists() or plan_path.exists():raise ValueError('IMMUTABLE_OUTPUT_EXISTS')
    admission=json.loads(ADMISSION.read_bytes());build=verify_build(build_path)
    if admission['errors'] or admission['development_admitted_native_rows']!=301:raise ValueError('ADMISSION_BLOCKED')
    inputs.mkdir(parents=True);entries=[];query_locks={};scheduled=Counter();counts=Counter()
    for repo,r in admission['repo_results'].items():
        if repo not in ['requests','gin']:continue
        suites=r.get('dev_suite_entries') or [r['native_dev_suite_entry'],r['compat_dev_suite_entry']]
        for entry in suites:
            commit,path=entry.split(':',1);relative=path.split('/'+repo+'/',1)[1]
            raw=frozen_blob(entry,admission['inputs_sha256']);suite=json.loads(raw)
            profile='native' if suite['scoring']=='codecortex-native-v1' else 'compat'
            qp=Path(relative).parent/suite['queries']
            if qp!=Path(relative).parent/('queries.'+profile+'.dev.jsonl'):raise ValueError('QUERY_OUTSIDE_CURRENT_DEV_ALLOWLIST')
            qentry=commit+':crates/cc-eval/benchmarks/public-v19/'+repo+'/'+qp.as_posix()
            qraw=frozen_blob(qentry,admission['inputs_sha256']);rows=[json.loads(l) for l in qraw.splitlines()]
            if any(q['split']!='dev' for q in rows):raise ValueError('NON_DEV_INTAKE')
            target=inputs/repo/relative;target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes(raw)
            (inputs/repo/qp).write_bytes(qraw)
            source_root=(target.parent/suite['source']['root']).resolve()
            if not source_root.is_relative_to(inputs.resolve()):raise ValueError('SOURCE_ESCAPE')
            for path in suite['source']['files']:
                source_entry=commit+':crates/cc-eval/benchmarks/public-v19/'+repo+'/source/'+path
                src=frozen_blob(source_entry,admission['inputs_sha256']);dest=source_root/path
                if not dest.resolve().is_relative_to(inputs.resolve()):raise ValueError('SOURCE_ESCAPE')
                dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(src)
            block=Path(relative).parent.as_posix().replace('/','-')
            key=repo+'-'+block+'-'+profile
            entries.append({'key':key,'repo':repo,'profile':profile,'suite_entry':entry,'suite_path':str(target),'suite_sha256':sha(raw),
                'query_entry':qentry,'query_sha256':sha(qraw),'query_count':len(rows),'no_answer_count':sum(q['no_answer'] for q in rows),
                'scheduled_rows':len(rows)*suite['repetitions'],'repetitions':suite['repetitions'],'warmup':suite['warmup'],'top_k':suite['top_k'],
                'timeout_ms':suite['timeout_ms'],'seed':suite['seed'],'source_lock':suite['source'],
                'engine_config_sha256':sha(canon(suite['engine_config'])),'gold_native_answers_sha256':sha(canon([q['answers'] for q in rows])),
                'compat_expected_files_sha256':sha(canon([q['expected_files'] for q in rows]))})
            scheduled[profile]+=len(rows)*suite['repetitions'];counts[profile]+=len(rows);query_locks[qentry]=sha(qraw)
    pilot=[e['key'] for e in entries if e['repo']=='typescript' and '/blocks/01-migration/' in e['suite_path']]
    if pilot or dict(counts)!={'native':158,'compat':138}:raise ValueError('INTAKE_COUNTS')
    inventory={p.relative_to(inputs).as_posix():sha(p.read_bytes()) for p in sorted(inputs.rglob('*')) if p.is_file()}
    plan={'schema_version':1,'scope':'public_development_only_default_baseline_not_confirmatory','created_utc':datetime.now(timezone.utc).isoformat(),
        'ranking_seen_before_this_plan':True,'exposure_note':'public DEV already exposed in prior baseline; current ranking not seen before this receipt','execution_source_sha':SOURCE,'admission_sha256':sha(ADMISSION.read_bytes()),
        'admission_commit':'5385f5a7a2a875c6d5cbd049bdde039bf71bbf32','build_receipt_path':str(build_path),'build_receipt_sha256':sha(build_path.read_bytes()),
        'actual_binary_sha256':{n:a['binary_sha256'] for n,a in build['artifacts'].items()},
        'protocol_file_sha256':{n:sha((OWNER.parent/n).read_bytes()) for n in ['PREREGISTRATION.md','preregistration.json','evaluator-query.schema.json']},
        'global_correlation_registry_sha256':admission['global_review']['registry_sha256'],'global_correlation_components':280,
        'inputs_root':str(inputs),'input_file_sha256':inventory,'query_file_sha256':query_locks,'suite_entries':entries,'counts':dict(counts),
        'scheduled_rows':dict(scheduled),'pilot_suite_keys':pilot,'pilot_rule':'original TypeScript01-migration pilot outside group-pygo ownership; not_run here; no substitute pilot',
        'pilot_continuation_rule':'complete run artifacts and integrity replay; quality/Partial/no-answer failures retained and do not tune or prevent remaining suites',
        'configuration':'unchanged suite repetitions/warmup/top_k/timeout/seed/source domains; MCP adapter fixed hybrid; product default features; no semantic/network',
        'measurement_profile':'quality','optional_process_tree_probe':'disabled via CODECORTEX_BENCH_PROCESS_PROBE=0; native PID RSS separate; no peak certification',
        'analysis_definitions':{'repetition_unit':'arithmetic score mean within query; scheduled repetitions never multiply question N',
            'micro':'mean eligible per-query means; native answerable and compat separate; missing scheduled rows invalidates global aggregation',
            'repository_macro':'equal mean of nonempty fixed repo per-query means','category_macro':'equal mean of nonempty frozen category per-query means',
            'family_balanced':'mean variants within frozen global correlation component; equal component weight within repo, then equal repo macro',
            'paired_projection':'join native and compat by fixed case ID; descriptive side-by-side existing scores, distinct formulas; not a candidate-minus-baseline intervention',
            'strict_no_answer':'normalized no_match and empty hits for every scheduled native no-answer row; failures/Partial incorrect',
            'intervals':'candidate-minus-baseline preregistered 10000 stratified family bootstrap NOT_RUN: only one product arm and two distinct scoring profiles',
            'facet_graph_Recall20_SymbolAccuracy_DuplicationRate':'not_implemented/not_run; retain raw/normalized evidence, no proxy'},
        'formal_600_accepted':0,'clean_holdout':0,'protected_body_reads':0,'live_provider_calls':0,'paid_provider_calls':0}
    plan_path.parent.mkdir(parents=True,exist_ok=True);plan_path.write_text(json.dumps(plan,indent=2)+'\n')
    print(json.dumps({'status':'preregistered_before_ranking','plan_sha256':sha(plan_path.read_bytes()),'counts':dict(counts),'scheduled_rows':dict(scheduled)}))


def execute(plan_path,output,phase):
    plan=json.loads(plan_path.read_bytes());build=verify_build(Path(plan['build_receipt_path']))
    if sha(Path(plan['build_receipt_path']).read_bytes())!=plan['build_receipt_sha256']:raise ValueError('BUILD_RECEIPT_DRIFT')
    inputs=Path(plan['inputs_root'])
    if any(sha((inputs/name).read_bytes())!=expected for name,expected in plan['input_file_sha256'].items()):raise ValueError('INPUT_HASH_DRIFT')
    output.mkdir(parents=True,exist_ok=False)
    env=os.environ.copy()
    env['CODECORTEX_BENCH_PROCESS_PROBE']='0'
    commands=[]
    for entry in plan['suite_entries']:
        if phase=='pilot' and entry['key'] not in plan['pilot_suite_keys']:continue
        dest=output/entry['key'];binary=build['artifacts']['cc-eval']['copied_binary'];product=build['artifacts']['codecortex']['copied_binary']
        command=[binary,'run','--backend','mcp-stdio','--binary',product,'--suite',entry['suite_path'],'--output',str(dest),'--profile',plan['measurement_profile']]
        started=datetime.now(timezone.utc).isoformat();proc=subprocess.run(command,env=env,capture_output=True)
        (output/(entry['key']+'.run.stdout')).write_bytes(proc.stdout);(output/(entry['key']+'.run.stderr')).write_bytes(proc.stderr)
        before={p.relative_to(dest).as_posix():sha(p.read_bytes()) for p in dest.rglob('*') if p.is_file()} if dest.exists() else {}
        replay_command=[binary,'replay','--run',str(dest)];replay=subprocess.run(replay_command,env=env,capture_output=True)
        (output/(entry['key']+'.replay.stdout')).write_bytes(replay.stdout);(output/(entry['key']+'.replay.stderr')).write_bytes(replay.stderr)
        after={p.relative_to(dest).as_posix():sha(p.read_bytes()) for p in dest.rglob('*') if p.is_file()} if dest.exists() else {}
        changed=[p for p in set(before)|set(after) if before.get(p)!=after.get(p)]
        record={'key':entry['key'],'repo':entry['repo'],'profile':entry['profile'],'command':command,'run_exit_code':proc.returncode,
            'replay_command':replay_command,'replay_exit_code':replay.returncode,'started_utc':started,'finished_utc':datetime.now(timezone.utc).isoformat(),
            'before_replay_sha256':before,'after_replay_sha256':after,'changed_on_replay':sorted(changed)}
        commands.append(record);(output/'commands.json').write_text(json.dumps(commands,indent=2)+'\n')
        print(json.dumps({'key':entry['key'],'run_exit_code':proc.returncode,'replay_exit_code':replay.returncode,'changed_on_replay':len(changed)}),flush=True)
    if verify_build(Path(plan['build_receipt_path']))!=build:raise ValueError('BINARY_CHANGED_AFTER_RUN')
    if any(sha((inputs/name).read_bytes())!=expected for name,expected in plan['input_file_sha256'].items()):raise ValueError('INPUT_CHANGED_AFTER_RUN')


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);sub=p.add_subparsers(dest='action',required=True)
    a=sub.add_parser('prepare');a.add_argument('--inputs',type=Path,required=True);a.add_argument('--build-receipt',type=Path,required=True);a.add_argument('--plan',type=Path,required=True)
    a=sub.add_parser('execute');a.add_argument('--plan',type=Path,required=True);a.add_argument('--output',type=Path,required=True);a.add_argument('--phase',choices=['pilot','full'],required=True)
    a=p.parse_args()
    if a.action=='prepare':prepare(a.inputs.resolve(),a.build_receipt.resolve(),a.plan.resolve())
    else:execute(a.plan.resolve(),a.output.resolve(),a.phase)
