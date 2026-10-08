#!/usr/bin/env python3
"""Actual fixed helper entry, isolated Git fixture, no mocked validators/builds."""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

WORK = Path('/workspace/scratch/031390cf22eb')
REPO = WORK / 'codecortex-closeout'
SOURCE = '2d575c60040d852a7c02e2bf6fa68d21caecd62d'
OUT = WORK / 'round5-corpus-coexistence/package-boundary-original'
sha = lambda raw: hashlib.sha256(raw).hexdigest()
FILES = ['scripts/p7_build_identity.py', 'scripts/p7_stdio_build_receipt.py',
         'scripts/p8_candidate_execution.py', 'scripts/p8_external_candidate.py',
         'scripts/p8_release_evidence.py',
         'artifacts/benchmarks/p8-external-recovery-20261008/manifest.json',
         'artifacts/benchmarks/p8-external-recovery-20261008/revision-2/recover_external.py']


def git(root, *args):
    return subprocess.check_output(['git', '-C', str(root), *args], stderr=subprocess.PIPE)


def snapshot(root):
    return {p.relative_to(root).as_posix(): sha(p.read_bytes()) for p in root.rglob('*')
            if p.is_file() and '.git' not in p.relative_to(root).parts}


assert not OUT.exists()
OUT.mkdir()
source_inputs = {}
results = []
with tempfile.TemporaryDirectory(prefix='p8-package-boundary-', dir=WORK) as tmp:
    temp = Path(tmp)
    source = temp / 'source'
    source.mkdir()
    for name in FILES:
        raw = git(REPO, 'show', SOURCE + ':' + name)
        assert raw == git(REPO, 'show', '56d15ed246eedd9add0c0d2843001b5f4e971ee8:' + name)
        source_inputs[name] = {'sha256': sha(raw), 'bytes': len(raw)}
        target = source / name
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(raw)
    # Real fixed-source inventory for source_snapshot, deliberately not a build.
    (source / 'Cargo.toml').write_text('[workspace]\nmembers = []\nresolver = "2"\n')
    (source / 'Cargo.lock').write_text('# Isolated input-shape fixture; no build performed.\nversion = 3\n')
    (source / 'crates').mkdir()
    (source / 'crates/path-boundary-fixture.rs').write_text('// Source inventory sentinel only.\n')
    git(source, 'init', '-q')
    git(source, 'add', '--all')
    git(source, '-c', 'user.name=Local boundary fixture', '-c', 'user.email=fixture@example.invalid',
        '-c', 'commit.gpgsign=false', 'commit', '-qm', 'Isolated original-helper path boundary fixture')
    fixture_commit = git(source, 'rev-parse', 'HEAD').decode().strip()
    product, runner = temp / 'product-build', temp / 'runner-build'
    for d in (product, runner):
        d.mkdir()
        (d / 'evidence-sentinel.txt').write_text('No real build, binary, raw output or receipt in this fixture.\n')
    waypoint = temp / 'waypoint'
    waypoint.mkdir()
    source_alias, product_alias, runner_alias = (temp / name for name in ('source-alias', 'product-alias', 'runner-alias'))
    source_alias.symlink_to(source, target_is_directory=True)
    product_alias.symlink_to(product, target_is_directory=True)
    runner_alias.symlink_to(runner, target_is_directory=True)
    cases = [
        ('disjoint_positive_path', temp / 'disjoint-output', product, runner, False),
        ('direct_source_negative', source / 'direct-output', product, runner, True),
        ('direct_product_negative', product / 'direct-output', product, runner, True),
        ('output_ancestor_negative', temp, product, runner, True),
        ('output_parent_symlink_to_source', source_alias / 'linked-output', product, runner, True),
        ('output_parent_symlink_to_product', product_alias / 'linked-output', product, runner, True),
        ('output_parent_symlink_to_runner', runner_alias / 'linked-output', product, runner, True),
        ('output_dotdot_into_source', waypoint / '..' / 'source' / 'dotdot-output', product, runner, True),
        ('output_dotdot_into_product', waypoint / '..' / 'product-build' / 'dotdot-output', product, runner, True),
        ('product_input_parent_alias', product / 'aliased-input-output', product_alias, runner, True),
        ('runner_input_parent_alias', runner / 'aliased-input-output', product, runner_alias, True),
        ('product_input_dotdot', product / 'dotdot-input-output', waypoint / '..' / 'product-build', runner, True),
    ]
    before_files = {str(source): snapshot(source), str(product): snapshot(product), str(runner): snapshot(runner)}
    for label, output, product_arg, runner_arg, must_refuse in cases:
        resolved = output.resolve(strict=False)
        roots_crossed = [kind for kind, root in (('source', source), ('product', product), ('runner', runner))
                         if resolved == root or resolved.is_relative_to(root) or root.is_relative_to(resolved)]
        existed_before = resolved.exists()
        command = ['python3', str(source / 'scripts/p8_external_candidate.py'),
                   '--source-root', str(source), '--expected-source', fixture_commit,
                   '--product-build', str(product_arg), '--runner-build', str(runner_arg),
                   '--output', str(output)]
        started = datetime.now(timezone.utc).isoformat()
        tick = time.monotonic()
        result = subprocess.run(command, cwd=temp, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'}, timeout=30)
        log = OUT / (label + '.log')
        log.write_bytes(result.stdout)
        created = not existed_before and resolved.exists()
        after = {str(source): snapshot(source), str(product): snapshot(product), str(runner): snapshot(runner)}
        assert after == before_files, 'Fixture file bytes unexpectedly changed'
        lines = result.stdout.decode(errors='replace').splitlines()
        results.append({'case': label, 'command': command, 'started_utc': started,
            'elapsed_seconds': time.monotonic() - tick, 'exit_code': result.returncode,
            'output_argument': str(output), 'resolved_output': str(resolved),
            'input_roots_crossed_by_actual_output': roots_crossed,
            'isolation_requires_precreation_refusal': must_refuse,
            'output_directory_existed_before': existed_before,
            'output_directory_exists_after': resolved.exists(),
            'new_directory_created': created,
            'boundary_violation': must_refuse and created and bool(roots_crossed),
            'final_exception': lines[-1] if lines else None,
            'log': str(log), 'log_sha256': sha(result.stdout),
            'source_and_build_sentinel_file_bytes_unchanged': True,
            'new_package_files': sorted(p.relative_to(resolved).as_posix() for p in resolved.rglob('*') if p.is_file())
                if created else []})
    assert results[0]['new_directory_created'] and not results[0]['input_roots_crossed_by_actual_output']
    assert all(not r['boundary_violation'] for r in results[1:4])
    assert all(r['boundary_violation'] for r in results[4:])
    report = {'schema_version': 1, 'status': 'confirmed_prevalidation_output_isolation_violation',
        'auditor': '/root/p8_corpus_closeout', 'fixed_original_source_commit': SOURCE,
        'fixed_helper_sha256': source_inputs['scripts/p8_external_candidate.py']['sha256'],
        'fixture_commit': fixture_commit, 'source_inputs': source_inputs,
        'script_sha256': sha(Path(__file__).read_bytes()), 'cases': results,
        'boundary_violation_cases': sum(r['boundary_violation'] for r in results),
        'no_validators_or_methods_mocked_or_replaced': True,
        'actual_entry': 'Original unmodified p8_external_candidate.py CLI -> package()',
        'failure_stage': 'Path guard accepted aliases/.., out.mkdir created an empty directory across an input boundary, then original missing-build-receipt validation rejected. No fixture is claimed as a real successful build or package.',
        'positive_control_scope': 'Legitimate disjoint output reaches the same missing-build-receipt validation; direct source/product/ancestor outputs reject before mkdir.',
        'data_written_across_boundary': 'Empty directories only in disposable fixture source/build roots; no existing file bytes changed.',
        'real_repository_or_F_D_or_raw_writes': 0, 'retrieval_requests': 0, 'builds': 0,
        'protected_or_fresh_holdout_body_reads': 0,
        'minimal_fix_recommendation': 'Before any output mkdir/write, reject parent symlinks and traversal in source/product/runner/output arguments, then apply both-direction overlap checks to normalized absolute targets. Existing p8_release_evidence.path provides the required path checks; preserve output-not-exists and exact source/build/template guards.',
        'scope_limits': ['No OS-level defense against malicious concurrent rename is claimed or required by this reproduction.',
                         'This proves an early directory side effect. Later valid-build package writes were not exercised.']}
target = OUT / 'reproduction.json'
target.write_text(json.dumps(report, ensure_ascii=False, indent=2, sort_keys=True) + '\n')
print(json.dumps({'path': str(target), 'sha256': sha(target.read_bytes()), 'status': report['status'],
                  'violations': report['boundary_violation_cases'], 'cases': len(results)}))
