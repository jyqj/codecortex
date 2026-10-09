#!/usr/bin/env python3
"""Review the fixed-275e lifecycle artifact; this does not rerun measurements."""
import collections
import hashlib
import json
import math
from pathlib import Path
import shutil
import subprocess
import sys
import time
import zipfile

ROOT = Path(__file__).resolve().parent
FILES = ROOT / 'files'
MEASURE = FILES / 'measurement'
REPO = ROOT.parent / 'source'
OBSERVER = REPO / 'scripts/p8_lifecycle.py'
G = '275e8799d4947d297329073eaa3ca675d3fd0777'
R = 'f66daf6088f6f6a8d990d736ee0c1213032ca175'
OUT = ROOT / 'independent-review-01'


def sha(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def read(path):
    return json.loads(Path(path).read_text())


def git(path, commit=G):
    return subprocess.check_output(['git', 'show', commit + ':' + path], cwd=REPO)


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def invoke(args, name, timeout=120):
    start = time.monotonic()
    with (OUT / (name + '.stdout')).open('xb') as stdout, (OUT / (name + '.stderr')).open('xb') as stderr:
        result = subprocess.run(args, stdout=stdout, stderr=stderr, timeout=timeout)
    return {'argv': args, 'exit_code': result.returncode, 'elapsed_seconds': time.monotonic() - start,
            'stdout_sha256': sha(OUT / (name + '.stdout')), 'stderr_sha256': sha(OUT / (name + '.stderr'))}


def main():
    OUT.mkdir()
    archive = ROOT / 'original-artifact.zip'
    expected = read(ROOT / 'expected-artifact.json')
    require(expected['source'] == G and expected['run_id'] == 37890757049
            and expected['run_attempt'] == 1 and archive.stat().st_size == expected['bytes'], 'fixed 275e archive provenance')
    expected_zip = expected['sha256']
    require(sha(archive) == expected_zip, 'GitHub original ZIP digest')
    zipped = {}
    with zipfile.ZipFile(archive) as source:
        for item in source.infolist():
            if item.is_dir():
                continue
            path = FILES / item.filename
            require(path.is_file() and not path.is_symlink() and path.is_relative_to(FILES), 'original member path')
            with source.open(item) as member:
                digest = hashlib.file_digest(member, 'sha256').hexdigest()
            require(path.stat().st_size == item.file_size and sha(path) == digest, 'extracted member differs: ' + item.filename)
            zipped[item.filename] = {'bytes': item.file_size, 'sha256': digest}
    require({x.relative_to(FILES).as_posix() for x in FILES.rglob('*') if x.is_file()} == set(zipped), 'extra extracted files')
    receipt, data, report = [read(MEASURE / name) for name in ['receipt.json', 'replay-input.json', 'report.json']]
    product, build = receipt['product'], read(FILES / 'product/build-receipt.json')
    evaluator = read(FILES / 'replay-build-receipt.json')
    review = json.loads(git('artifacts/checkpoints/p8-surface-window-controls-20261009/independent-source-review.json', R))
    inputs = read(FILES / 'product/source-inputs.json')
    require(inputs == review['complete_inputs'] and len(inputs) == 1089, 'complete reviewed Cargo/Rust source inventory')
    require(build['source_before'] == build['source_after'] == evaluator['source_before'] == evaluator['source_after'] == product['source'], 'source stability and evaluator/product identity')
    require(product['source']['source_commit'] == G and product['source']['source_tree'] == '5859c18f6ead4f8eff5abc3be05d80dfc2fc5b46', 'fixed source G/tree')
    require(product['binary_sha256'] == build['binary_sha256'] == sha(FILES / 'product/codecortex'), 'original product binary')
    require(product['receipt_sha256'] == sha(FILES / 'product/build-receipt.json'), 'product receipt')
    require(receipt['evaluator_sha256'] == evaluator['binary_sha256'] == sha(FILES / 'p8-measurements'), 'original evaluator binary')
    require(receipt['evaluator_build']['receipt_sha256'] == sha(FILES / 'replay-build-receipt.json'), 'evaluator receipt')
    cargo_rows = {}
    for label, path, item, target, features in [
        ('product', FILES / 'product/cargo-build.jsonl', build['cargo_artifact'], 'codecortex', []),
        ('evaluator', FILES / 'replay-build.jsonl', evaluator['artifact'], 'p8-measurements', ['default'])]:
        rows = [json.loads(line) for line in path.read_text().splitlines() if line]
        require(rows[-1] == {'reason': 'build-finished', 'success': True}, label + ' actual Cargo completion')
        artifacts = [x for x in rows if x.get('reason') == 'compiler-artifact' and x.get('target', {}).get('name') == target]
        require(artifacts == [item] and item['features'] == features and item['profile']['test'] is False,
                label + ' unique actual artifact/features/profile')
        require(item['profile']['opt_level'] == '3' and item['profile']['debug_assertions'] is False,
                label + ' optimized release profile')
        command = build['build_command'] if label == 'product' else evaluator['build_command']
        require('--locked' in command and '--release' in command, label + ' original release argv')
        cargo_rows[label] = {'artifact': item, 'command': command, 'log_sha256': sha(path)}
    for key in ['build_stdout', 'build_stderr']:
        require(sha(FILES / evaluator[key]['name']) == evaluator[key]['sha256'], 'evaluator original log digest')
    for absolute, digest in receipt['observer_sha256'].items():
        relative = 'scripts/' + absolute.split('/scripts/', 1)[1]
        require(hashlib.sha256(git(relative)).hexdigest() == digest, 'fixed 275e observer: ' + relative)
    require(hashlib.sha256(git('scripts/p8_lifecycle.py')).hexdigest() == sha(OBSERVER), 'local verifier has exact G bytes')
    sealed = invoke([sys.executable, '-B', str(OBSERVER), 'verify', '--output-dir', str(MEASURE)], 'sealed-verify')
    require(sealed['exit_code'] == 0, 'unchanged original inventory verification')
    require(receipt['status'] == report['measurement_status'] == 'complete_observation' and receipt['exit_code'] == report['exit_code'] == 0 and receipt['failures'] == [], 'complete original observation')
    require(receipt['sample_counts'] == {'expected': 1230, 'recorded': 1230} and data['expected_samples'] == len(data['samples']) == 1230, 'original full sample schedule')
    require(receipt['plan'] == {'profile': 'release', 'query_samples': 400, 'cold_samples': 30, 'files': 32,
                               'request_timeout_seconds': 30, 'run_timeout_seconds': 3600,
                               'artifact_budget_bytes': 536870912}, 'unchanged original plan')
    events = [json.loads(x) for x in (MEASURE / 'events.jsonl').read_text().splitlines()]
    by_kind = collections.defaultdict(list)
    for row in events:
        by_kind[row['kind']].append(row)
    require(len(by_kind['pristine_fixture']) == len(by_kind['cold_build']) == 30, 'all cold fixtures')
    for row in by_kind['pristine_fixture']:
        require(row['index_and_persistent_parse_cache_absent'] is True, 'pristine cold fixture witness')
        fixture = MEASURE / 'fixtures' / Path(row['project']).name
        require(len(row['source_manifest']) == 32, 'fixture source count')
        for name, digest in row['source_manifest'].items():
            require(sha(fixture / name) == digest, 'retained fixture source')
    phases = collections.defaultdict(list)
    for index, row in enumerate(by_kind['cold_build']):
        sample = data['samples'][index]
        require(row['error'] is None and row['before']['indexed_files'] == 0 and row['after']['indexed_files'] == 32,
                'cold build actual initial/final state')
        require(sample['elapsed_us'] == (row['finished_ns'] - row['started_ns']) // 1000, 'cold elapsed original raw')
        phases['cold_build'].append(sample['elapsed_us'])
    queries = {q['sample_index']: q for q in data['raw_queries']}
    require(set(queries) == set(range(30, 1230)), 'one original source witness for every query')
    seen = set()
    for row in by_kind['query_attempt']:
        if row['sample_index'] is None:
            continue
        index = row['sample_index']
        require(index not in seen and index in queries, 'query sample uniqueness')
        seen.add(index)
        sample, witness = data['samples'][index], queries[index]
        require(row['error'] is None and sample['status'] == 'success' and row['response'] == witness['response'], 'raw response and successful attempt binding')
        require(row['elapsed_us'] == sample['elapsed_us'] == (row['finished_ns'] - row['started_ns']) // 1000, 'query original timing')
        old, new = [row[k]['diagnostics']['search_cache'] for k in ['before', 'after']]
        delta = {key: new[key] - old[key] for key in ['graph_hits', 'graph_misses', 'result_hits', 'result_misses']}
        require(delta == row['cache_control']['delta'], 'original public cache counters')
        require(row['before']['diagnostics']['retrieval']['generation'] == row['after']['diagnostics']['retrieval']['generation'], 'stable generation during query')
        wanted = 'hit' if row['phase'] == 'cache_hit' else 'miss'
        require(sample['evidence']['result_cache'] == wanted, 'cache stratum')
        require((delta['graph_hits'], delta['graph_misses']) == ((1, 0) if wanted == 'hit' else (0, 1)), 'one observed graph cache lookup')
        phases[row['phase']].append(sample['elapsed_us'])
    require(seen == set(queries) and {k: len(v) for k, v in phases.items()} == {'cold_build': 30, 'process_reopen': 400, 'warm_uncached': 400, 'cache_hit': 400}, 'all original strata without dropping attempts')
    distributions = []
    for layer in report['latency_layers']['layers']:
        name = layer['stratum']
        if name == 'unknown':
            require(layer['samples'] == 0, 'no unknown strata')
            continue
        values = sorted(phases[name])
        observed = layer['all_attempt_elapsed']
        require(observed['samples'] == len(values) and observed['max_us'] == values[-1] and
                observed['p50_us'] == values[math.ceil(len(values) * 0.5) - 1] and
                observed['p95_us'] == values[math.ceil(len(values) * 0.95) - 1], 'independent nearest-rank distributions')
        require(layer['completed'] == len(values) and all(layer[x] == 0 for x in ['errors', 'timeouts', 'cancelled', 'partial', 'missing_timings']), 'every denominator retained')
        distributions.append(layer)
    starts = {x['session']: x for x in by_kind['session_started']}
    closes = {x['session']: x for x in by_kind['session_closed']}
    require(len(starts) == len(closes) == receipt['session_count'] == 431 and set(starts) == set(closes), 'all owned sessions closed')
    for name, close in closes.items():
        process = read(MEASURE / 'sessions' / name / 'process.json')
        require(process == close['process'] and process['cleanup'] == 'completed' and process['exit_code'] == 0 and process['initialized'] is True, 'actual stdio lifecycle clean closure')
        require(process['pid'] == starts[name]['pid'] and process['binary_sha256'] == product['binary_sha256'], 'actual PID and product')
    require(len({starts[f'reopen-{i:03d}']['pid'] for i in range(400)}) == 400, '400 distinct reopened owned processes')
    require([x['ledger'] for x in by_kind['resource']] == data['resources'] and len(data['resources']) == 1261, 'all original native resource snapshots')
    require(report['memory_ledger']['total_rss_bytes'] is None and all(x['runner_rss_bytes'] is None and x['external_service_rss_bytes'] is None for x in data['resources']), 'unavailable/overlapping roles not zero or summed')
    # Physical byte counts belong to this actual 275e observation. Check every
    # retained closed-fixture storage object, not a historical G byte literal.
    storage = receipt['storage']
    require(len(storage) == 30 and len(by_kind['final_closed_fixture_storage']) == 30,
            'all final closed-fixture storage observations')
    expected_projects = {f'/home/runner/work/_temp/p8-lifecycle/measurement/fixtures/cold-{i:03d}'
                         for i in range(30)}
    require({item['project'] for item in storage} == expected_projects
            == {event['project'] for event in by_kind['final_closed_fixture_storage']},
            'each of the 30 original cold fixtures has one final storage observation')
    parts = []
    storage_paths = set()
    for item, event in zip(storage, by_kind['final_closed_fixture_storage']):
        require(all(event[key] == value for key, value in item.items()), 'storage event/receipt binding')
        for obj in item['objects']:
            within_project = Path(obj['path']).relative_to(item['project'])
            require(len(within_project.parts) >= 2 and within_project.parts[0] in ('.codecortex', '.cache'),
                    'storage object belongs to its own closed fixture')
            relative = Path(obj['path']).relative_to('/home/runner/work/_temp/p8-lifecycle').as_posix()
            require(relative.startswith('measurement/fixtures/') and relative not in storage_paths,
                    'duplicate or foreign original storage path')
            storage_paths.add(relative)
            require(relative in zipped and zipped[relative]['bytes'] == obj['bytes'],
                    'actual retained storage byte count differs')
            parts.append({key: obj[key] for key in ('storage_id', 'component', 'bytes')})
    archived_storage_paths = {name for name in zipped
                              if len(Path(name).parts) >= 5
                              and Path(name).parts[:2] == ('measurement', 'fixtures')
                              and Path(name).parts[3] in ('.codecortex', '.cache')}
    require(archived_storage_paths == storage_paths,
            'all archived fixture cache files and all physical objects correspond in both directions')
    require(parts == data['disk_partitions'], 'every original physical object represented once')
    require(report['disk_ledger']['total_bytes'] == sum(x['bytes'] for x in parts), 'physical disk accounting')
    require(len({x['storage_id'] for x in parts}) == len(parts) == len(storage_paths), 'physical storage not double counted')
    require(receipt['provider_cost']['status'] == 'not_applicable_to_disabled_profile' and all(receipt['provider_cost'][key] is None for key in ['reported', 'estimated', 'input_tokens', 'output_tokens', 'requests_billed']), 'no fictional provider zero cost')
    require(receipt['os_page_cache'] == 'not_cleared_unknown_not_cold_disk' and report['release_certified'] is False and report['performance_improvement_claim'] is False, 'measurement limits retained')
    # Reuse exact archived synthetic source bytes, or restore an absent path.
    # The input/report bytes and all sealed original files are left untouched.
    source_root = Path('/home/runner/work/_temp/p8-lifecycle/measurement/fixtures/cold-029')
    require({q['source_root'] for q in queries.values()} == {str(source_root)} and not source_root.is_symlink(), 'original replay source location')
    expected_sources = {p.name: sha(p) for p in sorted((MEASURE / 'fixtures/cold-029').glob('*.py'))}
    require(len(expected_sources) == 32, 'complete original replay fixture')
    for parent in source_root.parents:
        require(not parent.is_symlink(), 'aliased replay source parent')
    if source_root.exists():
        require(source_root.is_dir() and all(p.is_file() and not p.is_symlink() for p in source_root.iterdir()),
                'preexisting replay location is not the retained synthetic source')
        require({p.name: sha(p) for p in source_root.iterdir()} == expected_sources,
                'preexisting replay fixture differs; do not overwrite it')
    else:
        source_root.mkdir(parents=True)
        for original in sorted((MEASURE / 'fixtures/cold-029').glob('*.py')):
            with (source_root / original.name).open('xb') as destination:
                destination.write(original.read_bytes())
    restored = {p.name: sha(p) for p in source_root.iterdir()}
    require(restored == expected_sources, 'retained original synthetic source')
    binary = OUT / 'original-p8-measurements'
    shutil.copyfile(FILES / 'p8-measurements', binary)
    binary.chmod(0o755)
    require(sha(binary) == receipt['evaluator_sha256'], 'retained evaluator bytes before actual replay')
    repeated = invoke([str(binary), '--input', str(MEASURE / 'replay-input.json'), '--output', str(OUT / 'report-replayed.json')], 'actual-retained-replay')
    require(repeated['exit_code'] == 0 and sha(OUT / 'report-replayed.json') == sha(MEASURE / 'report.json'), 'actual independent replay byte-identical')
    require(sha(binary) == receipt['evaluator_sha256'] and sha(MEASURE / 'replay-input.json') == receipt['replay']['input_sha256'], 'unchanged replay inputs/binary')
    final_seal = invoke([sys.executable, '-B', str(OBSERVER), 'verify', '--output-dir', str(MEASURE)], 'after-replay-sealed-verify')
    require(final_seal['exit_code'] == 0, 'original files unchanged after independent replay')
    summary = {'schema_version': 1, 'verdict': 'accepted_scoped_lifecycle_observation', 'source': product['source'],
               'workflow_run': 37890757049, 'artifact_id': expected['artifact_id'], 'zip_sha256': expected_zip,
               'zip_bytes': archive.stat().st_size, 'original_member_count': len(zipped),
               'original_member_bytes': sum(x['bytes'] for x in zipped.values()),
               'receipt_sha256': sha(MEASURE / 'receipt.json'), 'complete_source_inputs': len(inputs),
               'cargo': cargo_rows, 'product_sha256': product['binary_sha256'],
               'evaluator_sha256': receipt['evaluator_sha256'], 'sealed_verify': sealed, 'replay': repeated,
               'reconstructed_synthetic_source_root': str(source_root), 'reconstructed_sources': restored,
               'exact_original_input_sha256': sha(MEASURE / 'replay-input.json'),
               'byte_identical_report_sha256': sha(MEASURE / 'report.json'), 'final_seal': final_seal,
               'events': {k: len(v) for k, v in by_kind.items()}, 'distributions': distributions,
               'session_count': 431, 'native_resource_snapshots': 1261, 'physical_all_fixture_index_bytes': sum(x['bytes'] for x in parts),
               'limits': ['No new measurement or current-runner product performance claim.',
                          '30 cold samples retain unbounded upper tail intervals; 400 query samples retain original IID assumptions.',
                          'No OS page-cache reset, continuous peak memory guarantee, performance improvement, provider cost zero, or release certification.',
                          'Original task dependency chain remains open; this scoped acceptance closes zero original TODOs.'],
               'todo_counts': {'total': 192, 'done': 163, 'remaining': 29}, 'review_script_sha256': sha(__file__)}
    (OUT / 'inspection.json').write_text(json.dumps(summary, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'verdict': summary['verdict'], 'replay_exit': repeated['exit_code'],
                      'samples': 1230, 'source_verified_queries': report['source_verified_queries'],
                      'original_members': len(zipped), 'inspection_sha256': sha(OUT / 'inspection.json')}))


if __name__ == '__main__':
    main()
