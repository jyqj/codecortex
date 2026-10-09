"""Actual D0 capacity body under an explicit local host-path adapter; never cleanup."""
import contextlib, hashlib, json, os, pathlib, subprocess, sys, types

ROOT=pathlib.Path('/workspace/scratch/28fef0db5e01')
HERE=ROOT/'recovery-audit/round14-wave002'
SOURCE=ROOT/'integration-validation/round12-D0-receiver-inputs/source'
EXPECTED='d0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
SCRIPT=SOURCE/'scripts/p8_runner_capacity.py'
OUT=HERE/'capacity-local';OUT.mkdir(parents=True,exist_ok=False)
TEMP=OUT/'receipts';TEMP.mkdir()
raw=SCRIPT.read_bytes()
committed=subprocess.check_output(['git','-C',str(SOURCE),'show',EXPECTED+':scripts/p8_runner_capacity.py'])
assert raw==committed
assert subprocess.check_output(['git','-C',str(SOURCE),'rev-parse','HEAD'],text=True).strip()==EXPECTED
module=types.ModuleType('fixed_D0_capacity_probe');module.__file__=str(SCRIPT)
exec(compile(raw,str(SCRIPT),'exec'),module.__dict__)
base_environment=os.environ.copy()
base_environment.pop('GITHUB_ACTIONS',None)
base_environment.pop('RUNNER_ENVIRONMENT',None)
base_environment.update(PYTHONDONTWRITEBYTECODE='1',P8_EXPECTED_SOURCE=EXPECTED,
                        GITHUB_WORKSPACE=str(SOURCE),RUNNER_TEMP=str(TEMP),P8_CAPACITY_PREPARE_ALLOWED='false')
cli=subprocess.run([sys.executable,'-B',str(SCRIPT),'--scale','100000','--output',str(TEMP/'original-cli')],
                   env=base_environment,cwd=SOURCE,stdin=subprocess.DEVNULL,capture_output=True,timeout=30,check=False)
(OUT/'original-cli.stdout').write_bytes(cli.stdout);(OUT/'original-cli.stderr').write_bytes(cli.stderr)
assert cli.returncode==2 and b'fresh GitHub-hosted Ubuntu 24 VM' in cli.stderr and not (TEMP/'original-cli').exists()
results=[{'case':'unchanged original CLI refuses non-hosted local environment before output or commands', 'passed':True,'exit_code':cli.returncode}]
command_calls=[]
def forbid_commands(*args,**kwargs):
    command_calls.append(repr(args));raise AssertionError('No SDK command is permitted in this local probe')
module.command=forbid_commands
def local_host_paths(environment):
    # Only the hosted location/context boundary is adapted. Real path validation,
    # the checkout/committed-script guards, disk usage, and capacity body are original.
    return module.regular_directory(environment['GITHUB_WORKSPACE']),module.regular_directory(environment['RUNNER_TEMP'])
module.hosted_context=local_host_paths
@contextlib.contextmanager
def environment(extra):
    previous=os.environ.copy()
    try:os.environ.clear();os.environ.update(base_environment);os.environ.update(extra);yield
    finally:os.environ.clear();os.environ.update(previous)
for label,workspace,source,wanted_error in (
    ('wrong-workspace',SOURCE.parent,EXPECTED,'capacity observer belongs to another checkout'),
    ('correct-workspace',SOURCE,EXPECTED,None),
    ('wrong-source-head',SOURCE,'f'*40,'capacity source head differs')):
    with environment({'GITHUB_WORKSPACE':str(workspace),'P8_EXPECTED_SOURCE':source}):
        value=module.prepare(100000,TEMP/label,allow_preparation=False)
    assert value['required_free_bytes']==30509367296 and value['steps']==[] and value['allow_preparation'] is False
    assert value['artifact_state']=='sealed_after_commands_finished'
    if wanted_error:
        assert value['exit_code']==2 and wanted_error in value['error']
    else:
        assert value['exit_code']==0 and value['status']=='capacity_available'
        assert value['source_commit']==EXPECTED and value['observer_sha256']==hashlib.sha256(raw).hexdigest()
        assert value['initial']['free_bytes']>=value['required_free_bytes'] and value['final']['free_bytes']>=value['required_free_bytes']
    results.append({'case':label,'passed':True,'exit_code':value['exit_code'],'error':value.get('error'),'actual_initial_free_bytes':value['initial']['free_bytes'],'required_free_bytes':value['required_free_bytes']})
alias=OUT/'temporary-alias';alias.symlink_to(TEMP,target_is_directory=True)
with environment({'RUNNER_TEMP':str(alias)}):
    try:module.prepare(100000,alias/'must-not-exist',False);raise AssertionError('symlink temporary path accepted')
    except RuntimeError as error:assert 'not a real directory' in str(error)
assert not (TEMP/'must-not-exist').exists()
results.append({'case':'real symlink temporary ancestor rejects before command/output','passed':True})
for unsafe in (pathlib.Path('/'),HERE,TEMP,SOURCE):
    try:module.safe_sdk(unsafe,[SOURCE,TEMP]);raise AssertionError('caller path accepted as SDK target')
    except RuntimeError as error:assert 'only the two fixed' in str(error)
results.append({'case':'unchanged SDK whitelist rejects root/source/evidence paths without execution','passed':True})
assert not command_calls and SCRIPT.read_bytes()==raw
assert module.SDK_PATHS==(pathlib.Path('/usr/local/lib/android'),pathlib.Path('/usr/share/dotnet'))
receipt={'scope':'local read-only D0 capacity-body integration probe, not a GitHub-hosted VM end-to-end run',
 'adaptation':'hosted_context returns validated local workspace and owned temporary directory; SDK command entry is a fail-closed sentinel',
 'unchanged_real_operations':['prepare body','script == workspace/scripts guard','git rev-parse HEAD','git show committed script comparison','shutil.disk_usage','fixed 100000 capacity requirement','regular_directory and SDK whitelist'],
 'source_commit':EXPECTED,'observer_sha256':hashlib.sha256(raw).hexdigest(),'observer_source_unchanged':True,
 'allow_preparation':False,'P8_CAPACITY_PREPARE_ALLOWED':'false','sdk_commands_attempted':command_calls,
 'native_measurements_started':False,'results':results}
(OUT/'probe-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
for x in results:print('PASS '+x['case'])
print(json.dumps({'passed':len(results),'total':len(results),'observer_sha256':receipt['observer_sha256'],'cleanup_commands':len(command_calls),'required_free_bytes':30509367296}))
