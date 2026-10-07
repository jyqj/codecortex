#!/usr/bin/env python3
"""Bind already-executed P7-019 HTTP three-policy inputs; run no product."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time

BASE = Path('/workspace/scratch/71bc431c4e4f')
REPO = BASE / 'codecortex-p8-input-lock'
OUT = Path(__file__).resolve().parent
RUN = BASE / 'p7-019-validation'
SOURCE = '1bb5ea9c6d66e9109fdaf8766606337eedec8219'
BENCH_SOURCE = '93356fc87e9534c286192bde1fc88aa95abe878b'
BENCH_EVIDENCE = '4b78f683cda4b83eb750557c79072dcf8b651d17'


def load(path):
    return json.loads(path.read_text())


def digest(path):
    value = hashlib.sha256()
    with path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            value.update(chunk)
    return value.hexdigest()


def write_new(name, value):
    with (OUT / name).open('x') as stream:
        json.dump(value, stream, indent=2, sort_keys=True)
        stream.write('\n')


def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args]).decode().strip()


assert git('rev-parse', 'HEAD') == SOURCE
script = REPO / 'scripts/lock_benchmark_inputs.py'
script_sha = digest(script)
execution = load(RUN / 'canonical-three-policy-stdio.json')
build = load(RUN / 'canonical-http-product-build-receipt.json')
product = load(RUN / 'canonical-http-product.json')
freeze = load(RUN / 'canonical-source-freeze.json')
local = load(RUN / 'canonical-three-policy/run/local/manifest.json')
comparison = load(RUN / 'canonical-three-policy/run/comparison.json')
assert execution['exit_code'] == build['exit_code'] == comparison['exit_code'] == 0
assert execution['source_commit_before'] == freeze['source_commit'] == BENCH_SOURCE
assert execution['source_inputs_unchanged'] and execution['product_binary_before'] == execution['product_binary_after'] == build['binary_sha256']
assert build['build_options']['features'] == ['semantic-http']
assert comparison['profile'] == 'fake' and comparison['effect_interpretation'] == 'none; engineering mechanism only'
assert local['measurement_profile'] == 'fake'

# These are projections of existing run/build fields, not a new host survey.
allowed = ['os', 'arch', 'cpu_parallelism', 'hardware', 'rustc', 'rustflags', 'sdkroot', 'eval_version', 'eval_debug_assertions']
environment = {
    'schema_version': 1, 'collection': 'projection_of_existing_recorded_fields_only',
    'source_manifest': 'p7-019-validation/canonical-three-policy/run/local/manifest.json',
    'fields': {key: local['engine'][key] for key in allowed},
    'build_profile': {key: build['build_options'][key] for key in ['features', 'profile', 'jobs', 'cargo', 'rustc']},
    'unknown_note': 'hardware is null in the original report; no CPU model, RAM measurement or paid-provider cost is invented',
}
write_new('environment-allowlist.json', environment)

inputs, bindings = [], []


def add(name, path, roles, kind='file'):
    relative = Path(path).relative_to(BASE).as_posix()
    inputs.append({'id': name, 'path': relative, 'kind': kind, 'roles': roles})


def at(name, pointer):
    return {'input': name, 'pointer': pointer}


def hashed(receipt, pointer, artifact, kind='sha256'):
    bindings.append({'kind': kind, 'receipt': at(receipt, pointer), 'artifact': artifact})


def equal(left, lp, right, rp):
    bindings.append({'kind': 'equal', 'left': at(left, lp), 'right': at(right, rp)})


def fixed(name, pointer, value):
    bindings.append({'kind': 'value', 'at': at(name, pointer), 'expected': value})


add('product', RUN / 'canonical-codecortex-semantic-http', ['binary'])
add('source', RUN / 'canonical-source-snapshot', ['source'], 'tree')
add('source_manifest', RUN / 'canonical-source-inputs.json', ['source_receipt'])
add('source_freeze', RUN / 'canonical-source-freeze.json', ['source_receipt'])
add('source_helper', Path(freeze['helper_source']), ['supporting_evidence'])
add('build', RUN / 'canonical-http-product-build-receipt.json', ['build_receipt'])
add('product_record', RUN / 'canonical-http-product.json', ['supporting_evidence'])
add('build_capture', RUN / 'canonical-http-product-build.json', ['supporting_evidence'])
add('build_log', RUN / 'canonical-http-product-build.log', ['supporting_evidence'])
add('workspace_clean', RUN / 'canonical-workspace-clean.json', ['supporting_evidence'])
add('workspace_clean_log', RUN / 'canonical-workspace-clean.log', ['supporting_evidence'])
add('executor_helper', RUN / 'run_checked.py', ['supporting_evidence'])
add('product_helper', RUN / 'capture_canonical_product.py', ['supporting_evidence'])
add('execution', RUN / 'canonical-three-policy-stdio.json', ['execution_receipt'])
add('execution_log', RUN / 'canonical-three-policy-stdio.log', ['supporting_evidence'])
add('suite', RUN / 'canonical-three-policy/suite.json', ['config', 'scoring', 'model'])
add('plan', RUN / 'canonical-three-policy/plan.json', ['model', 'supporting_evidence'])
add('corpus', RUN / 'canonical-three-policy/input', ['corpus'], 'tree')
add('queries', RUN / 'canonical-three-policy/queries.jsonl', ['queries'])
add('reports', RUN / 'canonical-three-policy/run', ['report'], 'tree')
add('provider_observations', RUN / 'canonical-three-policy/fake-provider-observations.json', ['supporting_evidence'])
add('environment', OUT / 'environment-allowlist.json', ['environment'])
for policy in ['local', 'auto', 'semantic']:
    add(policy + '_manifest', RUN / f'canonical-three-policy/run/{policy}/manifest.json', ['supporting_evidence'])
add('comparison', RUN / 'canonical-three-policy/run/comparison.json', ['supporting_evidence'])

hashed('build', '/binary_sha256', 'product')
hashed('execution', '/product_binary_before', 'product')
hashed('execution', '/product_binary_after', 'product')
hashed('execution', '/environment/P7_STRATEGY_BINARY_SHA256', 'product')
hashed('product_record', '/binary_sha256', 'product')
hashed('product_record', '/build_receipt_sha256', 'build')
hashed('build', '/build_options/binding/source_manifest_sha256', 'source_manifest')
hashed('execution', '/source_manifest_sha256', 'source_manifest')
hashed('source_freeze', '/manifest_sha256', 'source_manifest')
hashed('source_manifest', '', 'source', 'tree_sha256')
equal('build', '/source_files', 'source_manifest', '')
equal('build', '/build_options/binding/source_commit', 'source_freeze', '/source_commit')
equal('execution', '/source_commit_before', 'source_freeze', '/source_commit')
equal('build_capture', '/source_commit_before', 'source_freeze', '/source_commit')
equal('workspace_clean', '/source_commit_before', 'source_freeze', '/source_commit')
equal('execution', '/environment/CODECORTEX_BENCH_BINARY', 'product_record', '/binary')
equal('plan', '/binary', 'product_record', '/binary')
equal('plan', '/build_receipt', 'execution', '/environment/P7_STRATEGY_BUILD_RECEIPT')
equal('plan', '/source_snapshot', 'execution', '/environment/P7_STRATEGY_SOURCE_SNAPSHOT')
equal('source_freeze', '/snapshot_path', 'plan', '/source_snapshot')
equal('build_capture', '/command', 'build', '/build_options/command')
hashed('build_capture', '/log_sha256', 'build_log')
hashed('execution', '/log_sha256', 'execution_log')
hashed('workspace_clean', '/log_sha256', 'workspace_clean_log')
hashed('source_freeze', '/helper_sha256', 'source_helper')
hashed('build_capture', '/executor_script_sha256', 'executor_helper')
hashed('execution', '/executor_script_sha256', 'executor_helper')
hashed('build', '/build_options/binding/capture_executor_sha256', 'product_helper')
fixed('source_freeze', '/source_commit', BENCH_SOURCE)
fixed('source_freeze', '/input_count', 772)
fixed('build', '/build_options/features', ['semantic-http'])
fixed('build', '/build_options/profile', 'dev debug=0, incremental=false')
fixed('product_record', '/features_requested', ['semantic-http'])
fixed('plan', '/strategies', ['local', 'auto', 'semantic'])
fixed('plan', '/network', 'loopback_only')
fixed('suite', '/scoring', 'codecortex-native-v1')
fixed('suite', '/engine_config/semantic/model_id', 'p7-019-fake-constant-vector')
for name in ['build', 'build_capture', 'execution', 'workspace_clean']:
    fixed(name, '/exit_code', 0)
for name in ['build_capture', 'execution', 'workspace_clean']:
    fixed(name, '/source_inputs_unchanged', True)
    fixed(name, '/executor_script_unchanged', True)
for policy in ['local', 'auto', 'semantic']:
    name = policy + '_manifest'
    equal(name, '/engine/strategy_ablation/product_build_receipt', 'build', '')
    equal(name, '/suite/engine_config', 'suite', '/engine_config')
    equal(name, '/suite/scoring', 'suite', '/scoring')
    equal(name, '/suite/source', 'suite', '/source')
    equal(name, '/suite/queries_digest', 'suite', '/queries_digest')
    equal(name, '/input/source_digest', 'suite', '/source/digest')
    equal(name, '/input/query_digest', 'suite', '/queries_digest')
    equal(name, '/engine/engine_head_observed', 'source_freeze', '/source_commit')
    fixed(name, '/measurement_profile', 'fake')
    fixed(name, '/engine/strategy_ablation/requested', policy)
    fixed(name, '/infrastructure_failure', None)
    for field in allowed:
        equal('environment', '/fields/' + field, name, '/engine/' + field)
for field in environment['build_profile']:
    equal('environment', '/build_profile/' + field, 'build', '/build_options/' + field)
fixed('comparison', '/exit_code', 0)
fixed('comparison', '/profile', 'fake')
fixed('comparison', '/status', 'engineering_strategy_observations_not_quality_certification')
fixed('comparison', '/effect_interpretation', 'none; engineering mechanism only')

spec = {
    'schema_version': 1, 'scope': 'preparation_only', 'inputs': inputs, 'bindings': bindings,
    'unresolved': [
        'P7-020 remains incomplete; P8-001 stays todo/groundwork. No release candidate or P8 acceptance starts here.',
        'P7-019 is a synthetic loopback engineering profile, not public holdout quality or a live-model benefit certificate.',
        'The model_id and expected encoding space describe a pinned fake provider; no real model revision is certified.',
        'Original hardware is null; physical CPU model/RAM and paid-provider costs are unknown, not zero.',
        'This locks supplied source/build/execution relationships, not their independent authenticity or full hermetic input closure.',
        'Native cc-eval BLAKE3 declarations are retained and related across reports; this SHA256 lock does not replace the original loader/scorer execution.',
        'Exact retained product binary, source snapshot and canonical raw bundle are external inputs required for later verification.',
    ],
}
write_new('input-spec.json', spec)
prepare_command = [sys.executable, '-B', str(script), 'prepare', '--root', str(BASE), '--spec', str(OUT / 'input-spec.json'), '--out', str(OUT / '019-input-lock.json')]
started = time.monotonic()
prepared = subprocess.run(prepare_command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
with (OUT / 'prepare-lock.log').open('xb') as file:
    file.write(prepared.stdout)
assert prepared.returncode == 0, prepared.stdout.decode()
prepare_result = json.loads(prepared.stdout)
lock_sha = prepare_result['lock_sha256']
verify_command = [sys.executable, '-B', str(script), 'verify', '--root', str(BASE), '--lock', str(OUT / '019-input-lock.json'), '--expected-lock-sha256', lock_sha]
verified = subprocess.run(verify_command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
with (OUT / 'verify-lock.log').open('xb') as file:
    file.write(verified.stdout)
assert verified.returncode == 0, verified.stdout.decode()
assert git('rev-parse', 'HEAD') == SOURCE and digest(script) == script_sha
receipt = {
    'schema_version': 1, 'task': 'P8-001', 'scope': 'preparation_only',
    'source_commit': SOURCE, 'script_sha256': script_sha,
    'prior_benchmark_source': BENCH_SOURCE, 'prior_benchmark_evidence_commit': BENCH_EVIDENCE,
    'prior_benchmark_execution_owner': '/root/pr_audit', 'lock_execution_owner': '/root/todo_audit',
    'selected_report': 'P7-019 canonical HTTP product with local/auto/semantic public policies',
    'default_smoke_not_reclassified_as_three_policy_report': True,
    'root': str(BASE), 'input_spec': 'input-spec.json', 'input_spec_sha256': digest(OUT / 'input-spec.json'),
    'lock': '019-input-lock.json', 'lock_sha256': lock_sha,
    'prepare': {'command': prepare_command, 'exit_code': prepared.returncode, 'log': 'prepare-lock.log', 'log_sha256': digest(OUT / 'prepare-lock.log'), 'result': prepare_result},
    'verify': {'command': verify_command, 'exit_code': verified.returncode, 'log': 'verify-lock.log', 'log_sha256': digest(OUT / 'verify-lock.log'), 'result': json.loads(verified.stdout)},
    'elapsed_seconds': round(time.monotonic() - started, 6),
    'creator_script_sha256': digest(Path(__file__)),
    'product_binary_sha256': build['binary_sha256'],
    'source_file_count': freeze['input_count'],
    'environment_collection': 'Existing allowlisted fields only; no os.environ enumeration, external key dereference, hardware query or cost imputation',
    'rust_or_product_executed_by_this_lock_task': False, 'release_candidate': False,
    'new_benchmark_run_count': 0, 'task_status_changed': False, 'original_dependencies_changed': False,
}
write_new('019-input-lock-receipt.json', receipt)
print(json.dumps({key: receipt[key] for key in ['source_commit', 'prior_benchmark_source', 'lock_sha256', 'product_binary_sha256', 'source_file_count', 'elapsed_seconds', 'release_candidate', 'new_benchmark_run_count']}))
