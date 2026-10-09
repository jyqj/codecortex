import ast, base64, datetime, difflib, hashlib, json, pathlib, subprocess

ROOT=pathlib.Path('/workspace/scratch/28fef0db5e01')
HERE=ROOT/'recovery-audit/round14-wave002'
OLD=ROOT/'acceptance-review/round10-D0-recovery-controller/candidate/artifacts/checkpoints/p8-d0-recovery-20261009-28fe'
RUNTIME=ROOT/'runtime-review/round13-D0-live-receiver'
PREFIX='artifacts/checkpoints/p8-d0-recovery-wave002-20261009-28fe'
CANDIDATE=HERE/'candidate';PKG=CANDIDATE/PREFIX;PKG.mkdir(parents=True,exist_ok=True)
sha=lambda b:hashlib.sha256(b).hexdigest()
oldcode=(OLD/'recovery.py').read_text()
assert sha(oldcode.encode())=='2a6468f4e3d8cf8a24e22b7a4517ab197be34018148f0f25cf241f8431806c54'
original_files=['initial-identities.json','original-registration.json','predecessor-original-job.json',
 'predecessor-original-job.log','37854240827-run.json','37854240827-jobs1.json','37854240827-jobs2.json']
for name in original_files:(PKG/name).write_bytes((OLD/name).read_bytes())
copies={
 'predecessor-admission.json':HERE/'predecessor_admission.json',
 'prior-receiver-receipt.json':RUNTIME/'receive-001/receipt.json',
 'prior-wave001-registration.json':OLD/'registration.json',
 'prior-wave001-workflow.yml':ROOT/'acceptance-review/round10-D0-recovery-controller/candidate/.github/workflows/p8-d0-recovery.yml',
 'prior-wave001-controller.py':OLD/'recovery.py',
 'prior-wave001-run.json':RUNTIME/'capture-002/api-008.json',
 'prior-wave001-jobs.json':RUNTIME/'capture-002/api-014.json',
 'prior-wave001-job.log':RUNTIME/'capture-002/api-015.log',
 'prior-wave001-artifacts.json':RUNTIME/'capture-002/api-009.json',
 'original-capture001-receipt.json':RUNTIME/'capture-001/receipt.json',
 'current-original-jobs1.json':RUNTIME/'capture-002/api-004.json',
 'current-original-jobs2.json':RUNTIME/'capture-002/api-005.json',
 'current-original-artifacts.json':RUNTIME/'capture-002/api-003.json',
 'fixed-build-artifacts.json':RUNTIME/'capture-002/api-016.json',
 'README.md':HERE/'CANDIDATE-README.md',
 'test_recovery.py':HERE/'test_wave002.py'}
for name,source in copies.items():(PKG/name).write_bytes(source.read_bytes())
rawzip=(RUNTIME/'archives/11588924279.zip').read_bytes()
assert len(rawzip)==60953 and sha(rawzip)=='69639897e691c708dba91be441a7807df7d076ac12aea9afd7e8a280a439ef01'
(PKG/'prior-wave001.zip.base64').write_bytes(base64.b64encode(rawzip)+b'\n')
helper=(HERE/'official-D0-capacity-observer.py').read_bytes()
assert sha(helper)=='2e37954df30b153ad82b49a5841a977451288c1730d03bfd2d0d7a2ace412939'
(PKG/'original-capacity-observer.py').write_bytes(helper)
inputs=set(original_files)|set(copies)|{'prior-wave001.zip.base64','original-capacity-observer.py','recovery.py'}
code=oldcode.replace('import argparse\n','import argparse\nimport base64\nimport io\nimport stat\nimport zipfile\n',1)
code=code.replace("BRANCH = 'task/p8-d0-recovery-28fe-20261009'","BRANCH = 'task/p8-d0-recovery-wave002-28fe-20261009'",1)
start=code.index('INPUT_NAMES = ');end=code.index('\ndef require(',start)
code=code[:start]+'INPUT_NAMES = set('+repr(sorted(inputs))+')\n'+code[end:]
code=code.replace('def check_ledger(initial, attempts, wave):','def check_ledger(initial, attempts, wave, evidence):',1)
before="""    require(all(row['state'] == 'runner_interrupted_unobserved' and not row['known_errors'] for row in rows.values()),
            'late error, active, successful or unclassified predecessor blocks supplementation')"""
after="""    require(wave == FIXED_WAVE and len(rows) == 2 and
            rows[0]['state'] == 'runner_interrupted_unobserved' and not rows[0]['known_errors'] and
            not rows[0].get('late_evidence_revoked') and capacity_prior_eligible(rows[1], evidence),
            'late error, active, successful, unclassified or unproved capacity predecessor blocks supplementation')"""
assert code.count(before)==1;code=code.replace(before,after,1)
start=code.index('def load_package(');end=code.index('\ndef validate_original_run(',start)
code=code[:start]+(HERE/'controller_load.py').read_text().rstrip()+'\n'+code[end:]
start=code.index('def observe(');end=code.index('\ndef main():',start)
code=code[:start]+(HERE/'controller_observe.py').read_text().rstrip()+'\n'+code[end:]
start=code.index('def load_package(')
code=code[:start]+(HERE/'controller_additions.py').read_text().rstrip()+'\n\n'+code[start:]
ast.parse(code)
oldmain=next(n for n in ast.parse(oldcode).body if isinstance(n,ast.FunctionDef) and n.name=='main')
newmain=next(n for n in ast.parse(code).body if isinstance(n,ast.FunctionDef) and n.name=='main')
assert ast.dump(oldmain,include_attributes=False)==ast.dump(newmain,include_attributes=False)
(PKG/'recovery.py').write_text(code)
oldworkflow=(copies['prior-wave001-workflow.yml']).read_text()
oldreg=json.loads((OLD/'registration.json').read_bytes())
workflow=oldworkflow.replace('registered recovery wave 001','registered recovery wave 002').replace(
 'task/p8-d0-recovery-28fe-20261009','task/p8-d0-recovery-wave002-28fe-20261009').replace(
 'supplemental ordinal 1','supplemental ordinal 2').replace(
 'p8-d0-recovery-20261009-28fe','p8-d0-recovery-wave002-20261009-28fe').replace(
 'Admit only the fixed terminal predecessor and first wave','Admit only the fixed pre-native capacity failure and final ordinal').replace(
 'p8-d0-recovery-wave001-100000-8-ordinal1','p8-d0-recovery-wave002-100000-8-ordinal2')
oldhash=sha((OLD/'registration.json').read_bytes());assert workflow.count(oldhash)==1
workflow=workflow.replace(oldhash,'__REGISTRATION_SHA256__')
needle='          python3 -B scripts/p8_runner_capacity.py --scale 100000 '
assert workflow.count(needle)==1
workflow=workflow.replace(needle,'          GITHUB_WORKSPACE="$GITHUB_WORKSPACE/measured" \\\n'+needle,1)
prior=json.loads((PKG/'prior-receiver-receipt.json').read_bytes())
policy=json.loads((PKG/'predecessor-admission.json').read_bytes())
reg={k:v for k,v in oldreg.items() if k not in ('controller_inputs','first_wave_only','future_full_external_receiver_and_coverage')}
reg.update(schema='p8-D0-continuation-wave-v2',wave_id='wave-002',branch='task/p8-d0-recovery-wave002-28fe-20261009',
 registered_at_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(),fixed_wave_only=True,
 scope='retrospective adoption of all 150 original identities and the retained failed ordinal1 plus prospective fixed ordinal2; scheduling only',
 prior_receiver_receipt_sha256=sha((PKG/'prior-receiver-receipt.json').read_bytes()),
 prior_capture_receipts=prior['registry']['prior_capture_receipts'],
 prior_supplemental_attempts=[x for x in prior['attempts'] if x['ordinal']>0],prior_supplemental_attempt_count=1,
 prior_supplement_scope='Complete prior receiver eccffe7a retained; current complete inventories of both fixed branches required. No claim to enumerate arbitrary undeclared outside experiments.',
 predecessor_admission=policy,predecessor_log_sha256=policy['official_job_log_sha256'],
 original_predecessor_log_sha256=oldreg['predecessor_log_sha256'],
 wave=[{'scale':100000,'repetition':8,'ordinal':2,'predecessor_run_id':37865643378,'predecessor_run_attempt':1,'predecessor_job_id':113611597284}],
 capacity_fix='single capacity-command process GITHUB_WORKSPACE points to the fixed measured checkout; original helper and deletion/capacity guards unchanged',
 controller_inputs={name:sha((PKG/name).read_bytes()) for name in sorted(inputs)},workflow_template_sha256=sha(workflow.encode()),
 future_full_external_receiver_and_coverage='Requires separately reviewed receiver-v6 and original validators, complete raw coverage, all-history and partial reconciliation gates; not accepted here')
regraw=(json.dumps(reg,sort_keys=True,indent=2)+'\n').encode();(PKG/'registration.json').write_bytes(regraw)
(HERE/'workflow.template.yml').write_text(workflow)
workpath=CANDIDATE/'.github/workflows/p8-d0-recovery.yml';workpath.parent.mkdir(parents=True,exist_ok=True)
workpath.write_text(workflow.replace('__REGISTRATION_SHA256__',sha(regraw)))
(HERE/'wave001-to-wave002-controller.diff').write_text(''.join(difflib.unified_diff(oldcode.splitlines(True),code.splitlines(True),fromfile='published-wave001/recovery.py',tofile='candidate-wave002/recovery.py')))
(HERE/'wave001-to-wave002-workflow.diff').write_text(''.join(difflib.unified_diff(oldworkflow.splitlines(True),workpath.read_text().splitlines(True),fromfile='published-wave001/workflow.yml',tofile='candidate-wave002/workflow.yml')))
files=[]
for p in sorted(CANDIDATE.rglob('*')):
 if p.is_file():
  raw=p.read_bytes();raw.decode('utf-8');files.append({'path':str(p.relative_to(CANDIDATE)),'bytes':len(raw),'sha256':sha(raw)})
receipt={'status':'assembled_candidate_not_frozen_pending_controls_and_non_author_review','files':files,'total_bytes':sum(x['bytes'] for x in files),'original_main_driver_and_validation_AST_unchanged':True,'shared_repo_or_published_wave_modified':False,'measurement_or_scheduling_performed':False,'original_TODO_closed':0}
(HERE/'assembled-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps({'files':len(files),'bytes':receipt['total_bytes'],'controller_sha256':sha(code.encode()),'registration_sha256':sha(regraw),'workflow_sha256':sha(workpath.read_bytes()),'original_main_AST_unchanged':True}))
