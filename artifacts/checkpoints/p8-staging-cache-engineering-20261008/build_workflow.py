#!/usr/bin/env python3
"""Prepare a new, separate engineering workflow from the reviewed D0 wrapper."""
from pathlib import Path
import hashlib
import json
import re

HERE = Path(__file__).resolve().parent
ORIGINAL = Path('/dev/shm/p8-D0-source-preparation/p8-prefix-window-engineering.yml')

def main():
    text = ORIGINAL.read_text()
    text = text.replace('name: P8 dependency window engineering', 'name: P8 full staging cache engineering')
    text = text.replace('task/p8-prefix-window-engineering-20261009', 'task/p8-staging-cache-engineering-20261008')
    text = text.replace('prefix-', 'staging-')
    before = text.index('          commands = [\n')
    after = text.index('          record = {\n', before)
    commands = [
        ['cargo', 'fmt', '--all', '--', '--check'],
        ['cargo', 'clippy', '--locked', '--offline', '-p', 'cc-index', '-p', 'cc-db', '-p', 'cc-eval', '--all-targets', '--', '-D', 'warnings'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--test', 'p2b_resolution_store'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-index', '--test', 'qname_identity_transaction'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-index', '--test', 'qname_identity_proof'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--test', 'public_surface_store'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--lib', 'full_rebuild_advances_both_epochs_past_previous_values'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--lib', 'rebuild_generation_exceeds_writes_committed_during_rebuild'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--lib', 'staging_checkpoint_busy_preserves_wal_and_refuses_replacement'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--lib', 'schema_mismatch_rebuild_advances_generation_past_old_values'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-db', '--test', 'p2d_dependency_cost'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--test', 'p8_scale'],
        ['cargo', 'test', '--locked', '--offline', '-p', 'cc-eval', '--test', 'benchmark_oracle_streaming'],
    ]
    block = '          commands = [\n' + ''.join('              ' + repr(c) + ',\n' for c in commands) + '          ]\n'
    text = text[:before] + block + text[after:]
    text = text.replace('  controls_and_build:\n    runs-on:', '  controls_and_build:\n    if: github.run_attempt == 1\n    runs-on:')
    text = text.replace('  diagnostic:\n', '  diagnostic_small:\n')
    text = text.replace('    needs: controls_and_build\n    runs-on:', '    needs: controls_and_build\n    if: github.run_attempt == 1\n    runs-on:')
    start = text.index('  diagnostic_small:\n')
    large = text[start:]
    large = large.replace('  diagnostic_small:\n', '  diagnostic_100k:\n', 1)
    large = large.replace('    name: engineering scale ${{ matrix.scale }} repetition 0 only', '    name: engineering scale 100000 repetition 0 only')
    large = large.replace('    needs: controls_and_build', '    needs: [controls_and_build, diagnostic_small]')
    large = large.replace('    strategy:\n      fail-fast: false\n      max-parallel: 2\n      matrix:\n        scale: [1000, 10000]\n', '')
    large = large.replace('${{ matrix.scale }}', '100000')
    large = large.replace('SCALE: 100000', "SCALE: '100000'")
    text += '\n' + large
    assert text.count('github.run_attempt == 1') == 3
    assert text.count('--deadline-ms 18000000 --capacity-profile scale_capacity_v1') == 2
    assert text.count('--shard-index 0 --shard-count 30 --repetitions 30 --seed 12648430') == 2
    assert 'timeout-minutes: 350' in large and 'if: always()' in large
    assert 'staging-' in text and 'prefix-' not in text
    assert 'concurrency:' not in text and 'rerun' not in text
    (HERE / 'p8-staging-cache-engineering.yml').write_text(text)
    (HERE / 'engineering-commands.json').write_text(json.dumps(commands, indent=2) + '\n')
    plan = {
        'schema': 'p8-staging-cache-engineering-plan-v1',
        'status': 'draft_pending_independent_source_and_workflow_review',
        'trigger_branch': 'task/p8-staging-cache-engineering-20261008',
        'scope': 'Single new fixed-source engineering investigation, not an N30 primary matrix and not a repair of any prior study',
        'base_product_source': '260f596582f2d82b8d7c707b61a6b8b6a43b069f',
        'actual_source': 'The immutable new publication commit, checked out via github.sha and independently verified after publication; no invented SHA',
        'commands': commands,
        'fixed_diagnostic_scales': [1000, 10000, 100000],
        'fixed_diagnostic_repetition': 0,
        'diagnostic_order': 'Controls and original release build; then 1k and 10k; 100k only after both small diagnostics complete successfully',
        'original_driver': 'scripts/p8_scale_matrix.py',
        'seed': 12648430, 'shard_count': 30, 'repetitions': 30,
        'all_diagnostics_are_auxiliary_not_primary': True,
        'only_attempt': 1, 'retry_or_replacement_allowed': False,
        'native_deadline_ms': 18000000, 'original_wrapper_minutes': 302,
        'job_timeout_minutes': 350, 'capacity_profile': 'scale_capacity_v1',
        'dirty_budget': 200, 'resume_iterations': 1024,
        'raw_budget_bytes': 536870912,
        'canonical_stream_budget_bytes': 17179869184,
        'oracle_scratch_budget_bytes': 8589934592,
        'original_workloads_and_consumers': 'Unmodified original driver, all original stages and all 15 parity tables; no custom fallback evaluator',
        'failed_execution': 'Retain every available original log/receipt/partial artifact; no rerun, replacement, successful-only grouping or invented terminal data',
        'known_transport_limit': 'if: always() cannot guarantee upload after hard runner loss; missing raw stays missing and cannot be accepted',
        'source_and_build': 'Original full source-before/source-after and exact release build validation, all compiler/copy identities and raw retained',
        'source_wrapper_reference': {'path': str(ORIGINAL), 'sha256': hashlib.sha256(ORIGINAL.read_bytes()).hexdigest()},
        'workflow_sha256': hashlib.sha256(text.encode()).hexdigest(),
        'native_or_Cargo_run_locally': False, 'new_formal_study_included': False,
        'TODO_closed': 0, 'TODO_remaining': 29,
    }
    (HERE / 'engineering-plan.json').write_text(json.dumps(plan, sort_keys=True, indent=2) + '\n')
    print(json.dumps({'workflow_bytes': len(text.encode()), 'workflow_sha256': plan['workflow_sha256'], 'control_commands': len(commands), 'diagnostic_scales': plan['fixed_diagnostic_scales'], 'native_or_Cargo_executed': False}))

if __name__ == '__main__':
    main()
