#!/usr/bin/env python3
"""Verify retained bytes and rederive counts; never execute Rust or a product."""

import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import tarfile


def digest(data):
    return hashlib.sha256(data).hexdigest()


def checked_relative(name):
    path = PurePosixPath(name)
    assert not path.is_absolute() and '..' not in path.parts
    return path


def test_rows(log, package):
    rows, target, pending = [], None, None
    for line in log.splitlines():
        running = re.match(r'^\s+Running (tests/\S+\.rs|unittests src/lib\.rs) \(', line)
        if running:
            target = running[1]
        case = re.match(r'^test (.+?) \.\.\. (ok|FAILED|ignored)(?:, (.*))?$', line)
        if case:
            assert target is not None
            rows.append({'identity': f'{package}/{target}::{case[1]}', 'result': case[2], 'ignored_reason': case[3]})
        elif re.match(r'^test (.+?) \.\.\. ', line):
            assert pending is None
            pending = re.match(r'^test (.+?) \.\.\. ', line)[1]
        elif line in ('ok', 'FAILED', 'ignored') and pending is not None:
            rows.append({'identity': f'{package}/{target}::{pending}', 'result': line, 'ignored_reason': None})
            pending = None
    assert pending is None
    return rows


def lifecycle(read, raw):
    inputs = read('lifecycle-inputs.json')
    receipt = read('wiring-lifecycle-receipt.json')
    prefix = 'raw/wiring-lifecycle/run/'
    summary, rpc, http = [read(prefix + name) for name in ('summary.json', 'stdio-raw.json', 'http-raw.json')]
    oracle = read('lifecycle-oracle-original.json')
    for original, copy in [
        ('artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/gate-review-map.json', 'lifecycle-oracle-original.json'),
        ('artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/lifecycle_stdio.py', 'lifecycle-stdio-original.py'),
    ]:
        assert digest(raw(copy)) == inputs['original_inputs'][original]
    assert summary['passed'] is True and receipt['exit_code'] == 0
    assert summary['binary_sha256'] == inputs['binary']['sha256'] == receipt['explicit_lifecycle_binary']['sha256']
    assert len(summary['cases']) == len(oracle['lifecycle_rows']) == 13
    for expected, observed in zip(oracle['lifecycle_rows'], summary['cases'], strict=True):
        assert observed['case'] == expected['case']
        assert observed['status']['semantic_state'] == expected['expected_semantic_state']
        assert observed['status']['dense_state'] == expected['expected_dense_state']
        assert observed['delta']['document_posts'] == expected['expected_document_posts_delta']
        assert observed['delta']['query_posts'] == expected['expected_query_posts_delta']
    checkpoints, latest_status, errors = [], {}, []
    tool_lists = positive = 0
    for row in rpc:
        if row['kind'] == 'rpc':
            request, response = row['request'], row['response']
            if 'error' in response:
                errors.append(response['error'])
                continue
            if request['method'] == 'tools/list':
                assert len(response['result']['tools']) == 14
                tool_lists += 1
            if request['method'] == 'tools/call':
                value = response['result']['structuredContent']
                value = value.get('result', value)
                if request['params']['name'] == 'status':
                    latest_status[row['session']] = value['retrieval']
                elif request['params']['name'] in ('search', 'context'):
                    retrieval = value.get('evidence_summary', {}).get('retrieval', {})
                    lanes = retrieval.get('lanes', retrieval.get('lane_receipts', []))
                    semantic = next((lane for lane in lanes if lane['lane_id'] == 'semantic'), None)
                    if semantic and semantic['status'] == 'complete' and semantic['candidate_count'] > 0:
                        hits = value['machine_pack']['hits']
                        assert hits and all(hit['file_path'] == 'one.rs' for hit in hits)
                        assert all(hit['text'] and hit['text'] in 'pub fn needle() -> u32 { 947 }\n' for hit in hits)
                        positive += 1
        elif row['kind'] == 'checkpoint':
            assert row['status'] == latest_status[row['session']]
            checkpoints.append({key: value for key, value in row.items() if key not in ('sequence', 'kind')})
    assert checkpoints == summary['cases']
    assert len(errors) == 4
    assert all(error['code'] == -32603 and error['message'] == 'semantic recall is not configured' for error in errors)
    assert len(http) == 8 and len(rpc) == 129 and tool_lists == 10 and positive == 8
    for row in http:
        assert row['only_loopback'] and row['dummy_authorization_matched']
        query = all(text.startswith('lifecycle_query_') for text in row['body']['input'])
        assert row['query_posts'] == int(query) and row['document_posts'] == int(not query)
        assert row['query_inputs'] + row['document_inputs'] == len(row['body']['input'])
    costs = {key: sum(row[key] for row in http) for key in summary['costs']}
    assert costs == summary['costs'] == read('lifecycle-raw-verification.json')['actual_cost_counts']
    assert costs['document_posts'] == costs['query_posts'] == 4
    assert sum(row['response_status'] == 500 for row in http) == 3
    assert summary['paid_cost'] is None and summary['synthetic_reported_tokens_are_not_real_cost']


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-root', type=Path, help='optionally check the exact fixed inputs in this checkout')
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    index = json.loads((here / 'archive-index.json').read_text())
    archive = here / checked_relative(index['archive']['path'])
    assert digest(archive.read_bytes()) == index['archive']['sha256']
    assert archive.stat().st_size == index['archive']['size_bytes']
    blobs = {}
    with tarfile.open(archive, 'r:gz') as handle:
        assert len(handle.getmembers()) == len(index['archive']['members'])
        for member in handle.getmembers():
            checked_relative(member.name)
            assert member.isfile() and member.name not in blobs
            expected = index['archive']['members'][member.name]
            data = handle.extractfile(member).read()
            assert digest(data) == expected['sha256']
            assert len(data) == member.size == expected['size_bytes']
            blobs[member.name] = data
    for name, expected in index['direct_files'].items():
        checked_relative(name)
        data = (here / name).read_bytes()
        assert digest(data) == expected['sha256'] and len(data) == expected['size_bytes'], name

    def raw(name):
        checked_relative(name)
        return blobs[name] if name in blobs else (here / name).read_bytes()

    def read(name):
        return json.loads(raw(name))

    for name, expected in index['captured_original_files'].items():
        data = raw(name)
        assert digest(data) == expected['sha256'] and len(data) == expected['size_bytes'], name
    current = read('source-sha256.json')
    extra = read('literal-source-sha256.json')
    assert len(current) == 770 and len(extra) == 10
    baseline = read('gc-config-baseline-rebuilt-source-sha256.json')
    wiring = 'crates/cc-server/src/semantic_wiring.rs'
    fixture = 'crates/cc-server/tests/p7_gc_retention_config.rs'
    assert baseline.keys() == current.keys()
    assert {name for name in current if baseline[name] != current[name]} == {wiring}
    assert digest(raw('attempt-sources/baseline/semantic_wiring.rs')) == baseline[wiring]
    assert current[fixture] == baseline[fixture]
    assert b'error[E0432]' in raw('gc-config-fixed.log')
    assert b'left: String("timeout")' in raw('deadline-current-http.log')
    captured = {}
    for name in index['command_receipts']:
        receipt = read(name)
        label, command = receipt['label'], receipt['command']
        log = raw(receipt['log'])
        assert digest(log) == receipt['log_sha256'] and len(log) == receipt['log_bytes']
        manifest = raw(receipt['source_manifest'])
        assert digest(manifest) == receipt['source_manifest_sha256']
        assert receipt['source_count'] == 770 and receipt['source_unchanged']
        assert receipt['source_commit_after'] == receipt['source_commit']
        assert receipt['status_before'] == receipt['status_after'] == ''
        red = label in ('gc-config-baseline', 'gc-config-baseline-rebuilt')
        assert json.loads(manifest) == (baseline if red else current)
        assert receipt['source_commit'] == (index['fixture_source'] if red else index['source'])
        if 'extra_literal_inputs' in receipt:
            assert receipt['extra_literal_inputs'] == extra and receipt['extra_literal_inputs_unchanged']
        assert receipt['capture_script_sha256'] in {digest(raw('capture-run-v1.py')), digest(raw('capture-run.py'))}
        for name, expected in receipt['raw_artifacts'].items():
            assert digest(raw(name)) == expected['sha256'] and len(raw(name)) == expected['bytes']
        package = command[command.index('-p') + 1] if '-p' in command else None
        rows = test_rows(log.decode(), package)
        observed = [sum(row['result'] == status for row in rows) for status in ('ok', 'FAILED', 'ignored')]
        expected = [sum(row[key] for row in receipt['test_summaries']) for key in ('passed', 'failed', 'ignored')]
        assert observed == expected, label
        assert receipt['exit_code'] == (101 if red or label in ('gc-config-fixed', 'deadline-current-http') else 0)
        events = []
        for line in log.decode().splitlines():
            if line.startswith('{'):
                try:
                    events.append(json.loads(line))
                except ValueError:
                    pass
        artifact_events = {event['executable']: event for event in events if event.get('reason') == 'compiler-artifact' and event.get('executable')}
        for path, executable in receipt['executables'].items():
            event = artifact_events[path]
            for key in ('target', 'profile', 'manifest_path', 'features', 'fresh'):
                assert executable[key] == event[key], (label, path, key)
            assert re.fullmatch('[0-9a-f]{64}', executable['sha256']) and executable['bytes'] > 0
        if label == 'gc-config-baseline-rebuilt':
            for target in ('cc_semantic', 'cc_server'):
                assert any(event.get('reason') == 'compiler-artifact' and event['target']['name'] == target and event['fresh'] is False for event in events)
        captured[label] = rows

    p13, p14 = read('p7-013-receipt.json'), read('p7-014-receipt.json')
    assert not p13['full_task_accepted'] and not p14['full_task_accepted']
    for task in (p13, p14):
        for row in task['primary_matrix']:
            assert row['test_cases'] == captured[row['label']]
    primary13 = [case for row in p13['primary_matrix'] for case in captured[row['label']]]
    assert len(primary13) == len({case['identity'] for case in primary13}) == 58
    assert sum(row['result'] == 'ok' for row in primary13) == 57
    assert sum(row['result'] == 'FAILED' for row in primary13) == 1
    assert len(captured['deadline-isolated-diagnostic']) == 2
    assert all(row['result'] == 'ok' for row in captured['deadline-isolated-diagnostic'])
    assert '--test-threads=1' in read('deadline-isolated-diagnostic-receipt.json')['command']
    primary14 = [case for row in p14['primary_matrix'] for case in captured[row['label']]]
    unique14 = {case['identity']: case['result'] for case in primary14}
    assert sum(row['result'] == 'ok' for row in primary14) == 63
    assert len(unique14) == 48 and sum(status == 'ok' for status in unique14.values()) == 46
    assert sum(status == 'ignored' for status in unique14.values()) == 2
    assert len(captured['gc-config-fixed-rebuilt']) == 4
    assert sum(row['result'] == 'ok' for row in captured['gc-config-baseline-rebuilt']) == 3
    lifecycle(read, raw)

    identity = read('source-identity.json')
    review = read('p7-014-retention-independent-review.json')
    assert identity['source'] == review['source_commit'] == index['source']
    assert identity['source_paths_sha256'] == review['source_paths_sha256']
    assert not review['independent_rust_execution'] and review['review'] == 'accepted_scoped'
    if args.source_root is not None:
        for name, expected in {**current, **extra, **identity['source_paths_sha256'], **identity['authority_sha256']}.items():
            assert digest((args.source_root / checked_relative(name)).read_bytes()) == expected, name
    print(json.dumps({
        'verified': True, 'source': index['source'],
        'captured_original_files': len(index['captured_original_files']),
        'archive_members': len(blobs), 'direct_files': len(index['direct_files']),
        'P7_013_primary': {'passed': 57, 'failed': 1, 'ignored': 0},
        'P7_014_canonical_new_regression': {'passed': 4, 'failed': 0},
        'P7_014_profile_executions': {'passed': 63, 'ignored': 2, 'distinct_passed': 46},
        'actual_lifecycle_checkpoints': 13,
        'source_inputs_checked': 770 if args.source_root is not None else 0,
        'literal_inputs_checked': 10 if args.source_root is not None else 0,
        'rust_or_product_rerun': False, 'full_task_acceptance': False,
    }, sort_keys=True))


if __name__ == '__main__':
    main()
