"""Actual path functions and process environment only; no SDK/native execution."""
import hashlib, json, os, pathlib, shlex, subprocess, sys, types

HERE=pathlib.Path(__file__).resolve().parent
OUTPUT=HERE/'capacity-path-followup';OUTPUT.mkdir(exist_ok=False)
raw=(HERE/'official-D0-capacity-observer.py').read_bytes()
assert hashlib.sha256(raw).hexdigest()=='2e37954df30b153ad82b49a5841a977451288c1730d03bfd2d0d7a2ace412939'
c=types.ModuleType('fixed_capacity_paths');c.__file__=str(HERE/'official-D0-capacity-observer.py')
exec(compile(raw,c.__file__,'exec'),c.__dict__)
results=[]
real=OUTPUT/'real';real.mkdir();alias=OUTPUT/'alias';alias.symlink_to(real,target_is_directory=True)
assert c.regular_directory(real)==real
try:c.regular_directory(alias);raise AssertionError('symlink path accepted')
except RuntimeError as exc:assert 'not a real directory' in str(exc)
results.append({'case':'unchanged real-directory guard accepts owned real directory and rejects symlink ancestor','passed':True})
for target in (pathlib.Path('/'),HERE,real,pathlib.Path('/tmp')):
    try:c.safe_sdk(target,[HERE]);raise AssertionError('unlisted SDK deletion path accepted')
    except RuntimeError as exc:assert 'only the two fixed' in str(exc)
results.append({'case':'fixed SDK whitelist rejects root, source/evidence and temporary caller paths','passed':True})
environment={'GITHUB_ACTIONS':'true','GITHUB_REPOSITORY':'jyqj/codecortex','RUNNER_ENVIRONMENT':'github-hosted','RUNNER_OS':'Linux',
             'ImageOS':'ubuntu24','GITHUB_RUN_ID':'990000001','P8_EXPECTED_SOURCE':'d0cb69c601e530dcef738af0c2dffb8d3b8bcf28',
             'GITHUB_WORKSPACE':str(real),'RUNNER_TEMP':str(OUTPUT)}
try:c.hosted_context(environment);raise AssertionError('local paths accepted as hosted VM paths')
except RuntimeError as exc:assert 'separate hosted job paths' in str(exc)
environment['RUNNER_ENVIRONMENT']='self-hosted'
try:c.hosted_context(environment);raise AssertionError('self-hosted accepted')
except RuntimeError as exc:assert 'fresh GitHub-hosted Ubuntu 24 VM' in str(exc)
results.append({'case':'original hosted-context guard rejects local path spoof and self-hosted context','passed':True})
assert c.required_bytes(100000)==30509367296
assert c.SDK_PATHS==(pathlib.Path('/usr/local/lib/android'),pathlib.Path('/usr/share/dotnet'))
op=['rm','-rf','--one-file-system','--preserve-root=all','--',str(c.SDK_PATHS[0])]
assert c.privileged_argv(op)==['sudo','-n','timeout','--signal=TERM','--kill-after=5s','300s',*op]
try:c.privileged_argv(['rm','-rf','/']);raise AssertionError('caller cleanup target accepted')
except RuntimeError as exc:assert 'unexpected SDK command' in str(exc)
results.append({'case':'original requirement, whitelist and supervised argv unchanged; argv inspected only','passed':True})
workflow=(HERE/'candidate/.github/workflows/p8-d0-recovery.yml').read_text()
needle='          GITHUB_WORKSPACE="$GITHUB_WORKSPACE/measured" \\\n'
assert workflow.count(needle)==1
block=workflow[workflow.index(needle):].split('      - name:',1)[0]
syntax=subprocess.run(['bash','-n'],input=block.encode(),capture_output=True,check=False)
assert syntax.returncode==0,syntax.stderr
results.append({'case':'exact candidate capacity shell command parses without executing it','passed':True})
workspace=OUTPUT/'workspace with spaces';workspace.mkdir();measured=workspace/'measured';measured.mkdir()
standin=shlex.join([sys.executable,'-I','-B','-c',"import json,os; print(json.dumps({'workspace':os.environ['GITHUB_WORKSPACE'],'cwd':os.getcwd()}))"])
script='set -eu\n'+needle.strip()+'\n'+standin+'\n'+standin+'\n'
process_env=os.environ.copy();process_env['GITHUB_WORKSPACE']=str(workspace)
actual=subprocess.run(['bash','-c',script],cwd=measured,env=process_env,capture_output=True,check=False)
(OUTPUT/'process-env.stdout').write_bytes(actual.stdout);(OUTPUT/'process-env.stderr').write_bytes(actual.stderr)
assert actual.returncode==0,actual.stderr
child,parent=[json.loads(line) for line in actual.stdout.splitlines()]
assert child['workspace']==child['cwd']==str(measured)
assert parent['workspace']==str(workspace) and parent['cwd']==str(measured)
results.append({'case':'actual process assignment uses measured path with spaces and leaves parent workspace unchanged','passed':True,
                'scope':'Python environment-print stand-in; no capacity or native command executed','observed_child':child,'observed_parent':parent})
prior_correct=HERE/'capacity-local/receipts/correct-workspace/receipt.json'
prior_wrong=HERE/'capacity-local/receipts/wrong-workspace/receipt.json'
correct=json.loads(prior_correct.read_bytes());wrong=json.loads(prior_wrong.read_bytes())
assert correct['source_commit']=='d0cb69c601e530dcef738af0c2dffb8d3b8bcf28' and correct['observer_sha256']==hashlib.sha256(raw).hexdigest()
assert correct['status']=='not_run_capacity_unavailable' and correct['exit_code']==2
assert correct['initial']['free_bytes']==48517120 and correct['required_free_bytes']==30509367296
assert correct['steps']==wrong['steps']==[]
assert wrong['error']=='RuntimeError: capacity observer belongs to another checkout' and 'source_commit' not in wrong
manifest={p.name:{'bytes':p.stat().st_size,'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in (HERE/'capacity-local.log',HERE/'capacity_local_probe.py')}
receipt={'status':'passed_path_and_environment_controls_capacity_remains_unavailable','results':results,'original_observer_sha256':hashlib.sha256(raw).hexdigest(),
 'original_capacity_assertion_failure_preserved':True,'initial_probe_files':manifest,
 'prior_correct_workspace_receipt_sha256':hashlib.sha256(prior_correct.read_bytes()).hexdigest(),
 'prior_wrong_workspace_receipt_sha256':hashlib.sha256(prior_wrong.read_bytes()).hexdigest(),
 'hosted_capacity_available_claim':False,'actual_local_capacity_available':False,'SDK_commands_executed':False,'native_measurements_executed':False}
(OUTPUT/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
for x in results:print('PASS '+x['case'])
print(f'{len(results)}/{len(results)} path/environment controls passed; original local capacity remains unavailable')
