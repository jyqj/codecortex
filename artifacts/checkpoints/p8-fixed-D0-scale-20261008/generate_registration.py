#!/usr/bin/env python3
"""Freeze a new full D0 study only after its actual original prerequisites pass."""
import datetime
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
SOURCE = 'd0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
SOURCE_ROOT = Path('/workspace/scratch/2eaa00d0f93a/p8-D0-exact-source')
BUILD = Path('/workspace/scratch/2eaa00d0f93a/p8-D0-build-11582571291/original')
CONTROLS = Path('/dev/shm/p8-d0-controls-reception/original')
DIAGNOSTICS = Path('/dev/shm/p8-engineering-shard-reception')
RUN = 37846370300
SCRIPT_SHA = 'a65e819d3a00f8032e04d54c8da881a6efb160671f14252e96258f57813df189'
TEMPLATE_SHA = '5dda7c2170c2552eee0608e4ddbf84e95577761ad6e390e1d0ec3aa78c2a02ab'


def require(value, why):
    if not value:
        raise ValueError(why)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def load(path):
    return json.loads(path.read_bytes())


def write_new(path, raw):
    with path.open('xb') as output:
        output.write(raw)


def encoded(value):
    return (json.dumps(value, indent=2, sort_keys=True) + '\n').encode()


def main():
    require(not sys.flags.optimize, 'generation requires Python without -O')
    for name in ['registration.json', 'workflow.yml', 'registration-generation.json']:
        require(not (HERE / name).exists(), 'registration outputs must be new: ' + name)
    run = load(HERE / 'upstream-run-completed.json')
    jobs = load(HERE / 'upstream-jobs-completed.json')
    artifacts = load(HERE / 'upstream-artifacts-completed.json')
    require(run['id'] == RUN and run['head_sha'] == SOURCE and run['run_attempt'] == 1
            and run['status'] == 'completed' and run['conclusion'] == 'success'
            and run['event'] == 'push'
            and run['head_branch'] == 'task/p8-prefix-window-engineering-20261009'
            and run['path'] == '.github/workflows/p8-prefix-window-engineering.yml',
            'actual upstream run has not passed')
    expected_jobs = {113548146305, 113566745945, 113566745952}
    require(jobs['total_count'] == len(jobs['jobs']) == 3
            and {job['id'] for job in jobs['jobs']} == expected_jobs,
            'expected three original upstream jobs differ')
    require(all(job['status'] == 'completed' and job['conclusion'] == 'success'
                and job['head_sha'] == SOURCE and job['run_id'] == RUN
                for job in jobs['jobs']), 'an original upstream job did not pass')
    names = {
        'p8-prefix-scale-build-' + str(RUN),
        'p8-prefix-controls-' + str(RUN),
        'p8-prefix-shard-1000-0-' + str(RUN),
        'p8-prefix-shard-10000-0-' + str(RUN),
        'p8-prefix-capacity-1000-' + str(RUN),
        'p8-prefix-capacity-10000-' + str(RUN),
    }
    require(artifacts['total_count'] == len(artifacts['artifacts']) == 6
            and {item['name'] for item in artifacts['artifacts']} == names
            and len({item['id'] for item in artifacts['artifacts']}) == 6,
            'complete six unique original artifacts are required')
    by_name = {item['name']: item for item in artifacts['artifacts']}
    for item in artifacts['artifacts']:
        require(item['expired'] is False and item['workflow_run']['id'] == RUN
                and item['workflow_run']['head_sha'] == SOURCE
                and item['digest'].startswith('sha256:') and len(item['digest']) == 71,
                'original artifact provenance differs')
    build_artifact = by_name['p8-prefix-scale-build-' + str(RUN)]
    control_artifact = by_name['p8-prefix-controls-' + str(RUN)]
    require(build_artifact['id'] == 11582571291 and control_artifact['id'] == 11581509438,
            'fixed build or control artifact ID differs')
    sys.path.insert(0, str(SOURCE_ROOT / 'scripts'))
    import p8_scale_matrix as original
    snapshot = original.source_snapshot(SOURCE_ROOT)
    observers = original.driver_snapshot(SOURCE_ROOT)
    require(snapshot['source_commit'] == SOURCE and len(snapshot['inputs']) == 1087,
            'actual exact D0 source is missing')
    require(load(BUILD / 'source-before.json') == load(BUILD / 'source-after.json') == snapshot,
            'original build source snapshots differ from actual D0')
    built = load(BUILD / 'build.json')
    build_review_path = Path('/dev/shm/p8-D0-build-independent-review/review.json')
    build_review = load(build_review_path)
    require(build_review['original_validate_build_passed'] is True
            and build_review['source_commit'] == SOURCE
            and build_review['files_unchanged_after_validation'] is True
            and build_review['build_receipt_sha256'] == sha((BUILD / 'build.json').read_bytes())
            and build_review['zip_sha256'] == build_artifact['digest'][7:],
            'actual independent original build validation is missing')
    require(built['driver_source'] == observers and built['source_commit'] == SOURCE,
            'original scale observer/source identity differs')
    control = load(CONTROLS / 'receipt.json')
    require(control['status'] == 'commands_completed' and control['exit_code'] == 0
            and control['source_unchanged'] is True and control['not_run_command_indices'] == []
            and len(control['commands']) == len(control['results']) == 12
            and [row['argv'] for row in control['results']] == control['commands']
            and all(row['exit_code'] == 0 for row in control['results'])
            and control['files'] == original.inventory(CONTROLS, ('receipt.json',))
            and load(CONTROLS / 'source-before.json') == load(CONTROLS / 'source-after.json') == snapshot,
            'original twelve controls and full sealed originals are not valid')
    require(sha((HERE / 'admit.py').read_bytes()) == SCRIPT_SHA
            and sha((HERE / 'workflow.template.yml').read_bytes()) == TEMPLATE_SHA,
            'independently reviewed controller bytes changed')
    primary = [{'scale': scale, 'shard_index': index,
                'plan': original.registered_plan(scale, index, 30, 30, 12648430,
                                                 18000000, original.CAPACITY_PROFILE)}
               for scale in [100000, 50000, 10000, 5000, 1000] for index in range(30)]
    require(len(primary) == len({(row['scale'], row['shard_index']) for row in primary}) == 150,
            'complete prospective primary plan differs')
    diagnostic_rows = []
    independent = [{'kind': 'original_build', 'sha256': sha(build_review_path.read_bytes())}]
    for scale in [1000, 10000]:
        artifact = by_name[f'p8-prefix-shard-{scale}-0-{RUN}']
        directory = DIAGNOSTICS / str(artifact['id'])
        receipt_raw = (directory / 'shard.json').read_bytes()
        receipt = json.loads(receipt_raw)
        report_path = Path('/dev/shm/p8-D0-diagnostic-validation') / str(artifact['id']) / 'review.json'
        report = load(report_path)
        plan = original.registered_plan(scale, 0, 30, 30, 12648430, 18000000, original.CAPACITY_PROFILE)
        require(report['source'] == SOURCE and report['artifact_id'] == artifact['id']
                and report['original_validate_build_passed'] is True
                and report['original_validate_shard_passed'] is True
                and report['source_driver_ZIP_shard_and_build_all_unchanged_after_validation'] is True
                and report['shard_receipt_sha256'] == sha(receipt_raw)
                and report['zip_sha256'] == artifact['digest'][7:]
                and report['zip_bytes'] == artifact['size_in_bytes']
                and report['build_receipt_sha256'] == sha((BUILD / 'build.json').read_bytes())
                and report['plan'] == receipt['plan'] == plan
                and receipt['status'] == 'passed' and receipt['exit_code'] == 0,
                'actual full original diagnostic validation is missing')
        diagnostic_rows.append({'scale': scale, 'artifact_id': artifact['id'],
                                'artifact_name': artifact['name'], 'receipt_sha256': sha(receipt_raw),
                                'role': 'upstream_admission_only_not_a_new_primary_cell'})
        independent.append({'kind': f'original_diagnostic_{scale}',
                            'sha256': sha(report_path.read_bytes())})
    registration = {
        'schema': 'p8-independent-fixed-D0-full-scale-study-registration-v1',
        'registered_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'repository': 'jyqj/codecortex', 'source': SOURCE,
        'source_tree': 'c94b6cd9f3e64de377cd646c683e5e031ddf10b1',
        'source_manifest_sha256': snapshot['manifest_sha256'],
        'complete_source_inputs': snapshot['inputs'], 'scale_observer_inputs': observers['inputs'],
        'controller_branch': 'task/p8-fixed-d0-scale-20261008',
        'controller_workflow': '.github/workflows/p8-fixed-d0-scale.yml',
        'controller_script_sha256': SCRIPT_SHA, 'reviewed_workflow_template_sha256': TEMPLATE_SHA,
        'upstream_run': RUN, 'upstream_attempt': 1,
        'upstream_jobs': [{'id': job['id'], 'name': job['name']} for job in sorted(jobs['jobs'], key=lambda x: x['id'])],
        'upstream_artifacts': [{'id': item['id'], 'name': item['name'],
                                'bytes': item['size_in_bytes'], 'sha256': item['digest'][7:]}
                               for item in sorted(artifacts['artifacts'], key=lambda x: x['id'])],
        'build': {'artifact_id': build_artifact['id'], 'artifact_name': build_artifact['name'],
                  'receipt_sha256': sha((BUILD / 'build.json').read_bytes()),
                  'binary_sha256': built['binary_sha256'], 'binary_blake3': built['binary_blake3'],
                  'role': 'unchanged_original_release_build_no_recompilation'},
        'controls': {'artifact_id': control_artifact['id'], 'artifact_name': control_artifact['name'],
                     'receipt_sha256': sha((CONTROLS / 'receipt.json').read_bytes()),
                     'commands': control['commands']},
        'diagnostics': diagnostic_rows, 'independent_prerequisite_review_sha256': independent,
        'new_execution': {'scales': [100000, 50000, 10000, 5000, 1000], 'indices': list(range(30)),
                          'repetitions': 30, 'shard_count': 30, 'new_primary_cells': 150,
                          'groups': 50, 'expected_raw_samples': 1500, 'maximum_concurrent_cells': 20,
                          'workflow_attempt': 1, 'seed': 12648430, 'capacity_profile': original.CAPACITY_PROFILE,
                          'native_deadline_ms': 18000000, 'python_timeout_seconds': 18120,
                          'job_timeout_minutes': 350, 'raw_bytes_per_cell': 536870912,
                          'scheduler_scope': '20 separate disposable hosted VMs; no within-cell budget change',
                          'failure_policy': 'Retain every failed/missing cell; do not rerun, replace or substitute cells'},
        'primary_cells': primary,
        'original_studies': {'github_G_run': 37830173594,
                             'local_G_registration_commit': 'b94a93a9d92a237e0c55a23fd8e931c5dae659f0',
                             'local_G_terminal_evidence_commit': 'bf48b9125514bfe3e22a77e025b4224acdb54ff2',
                             'policy': 'Remain independent and unchanged; no pooling or replacement'},
        'acceptance_limits': [
            'All 150 primary cells are new measurements; upstream 1k and10k are admission-only extras.',
            'All measured source checkouts, original build/shard/aggregate validators and raw engine identities remain actual D0.',
            'Original five scales/N30/seed/budgets/15table oracle/fanout/missing and source gates remain unchanged.',
            'A later P5 can only cite separately proven complete byte equivalence; D0 results are never renamed as P5 measurements.',
            'P5 source review/full CI, all original TODO dependencies and release lock remain separate acceptance gates.',
        ],
        'original_TODO_closed_here': 0, 'original_TODO_remaining': 29,
    }
    raw = encoded(registration)
    template = (HERE / 'workflow.template.yml').read_bytes()
    require(template.count(b'__REGISTRATION_SHA256__') == 1, 'workflow substitution is not unique')
    workflow = template.replace(b'__REGISTRATION_SHA256__', sha(raw).encode())
    require(original.source_snapshot(SOURCE_ROOT) == snapshot, 'source changed during registration generation')
    write_new(HERE / 'registration.json', raw)
    write_new(HERE / 'workflow.yml', workflow)
    result = {'status': 'fixed_prospective_registration_generated_after_actual_prerequisites',
              'source': SOURCE, 'upstream_run': RUN, 'registration_bytes': len(raw),
              'registration_sha256': sha(raw), 'workflow_sha256': sha(workflow),
              'controller_sha256': SCRIPT_SHA, 'generator_sha256': sha(Path(__file__).read_bytes()),
              'original_complete_source_inputs': 1087, 'upstream_artifacts': 6,
              'original_controls': 12, 'original_diagnostics_passed': 2,
              'new_primary_cells': 150, 'new_measurements_started': False,
              'original_TODO_closed': 0, 'original_TODO_remaining': 29}
    write_new(HERE / 'registration-generation.json', encoded(result))
    print(json.dumps(result, sort_keys=True))


if __name__ == '__main__':
    main()
