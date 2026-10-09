#!/usr/bin/env python3
"""Read fixed CI ZIPs as data. Never import or execute any artifact code.

Writes only this review's report; temporary SQLite copies use the caller's TMPDIR.
Original archives and the owner's checkout are opened read-only.
"""
from pathlib import Path, PurePosixPath
from collections import Counter
import datetime
import hashlib
import json
import math
import os
import sqlite3
import stat
import subprocess
import tempfile
import zipfile

HERE = Path(__file__).resolve().parent
REPO = Path('/workspace/scratch/28fef0db5e01/codecortex')
ORIGINALS = Path('/workspace/scratch/a217aaae3bde/ci-artifacts')
TABLES = ['document_manifest', 'resolution_frontier', 'semantic_edges', 'dispatch_sites',
          'resolution_manifests', 'resolution_dependencies', 'public_surfaces', 'files',
          'symbols', 'imports', 'symbol_refs', 'call_edges', 'chunks', 'test_edges', 'routes']
ACTIONS = ['bounded_symbol_churn', 'add', 'rename', 'delete', 'real_git_branch_switch', 'restore_api']
REPORT = {'schema_version': 1, 'reviewer': '/root/runtime_review',
          'state': 'incomplete', 'scope': 'Independent read-only original-artifact audit; not task completion or release approval',
          'original_artifact_code_executed': False, 'original_artifact_files_modified': False,
          'checks': [], 'artifacts': {}}


def write_report():
    path = HERE / 'independent-originals-report.json'
    path.write_text(json.dumps(REPORT, ensure_ascii=False, sort_keys=True, indent=2) + '\n')


def check(name, value, detail=None):
    row = {'check': name, 'passed': bool(value)}
    if detail is not None:
        row['detail'] = detail
    REPORT['checks'].append(row)
    return bool(value)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def canonical(data):
    return (json.dumps(data, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=REPO, input=data, stderr=subprocess.PIPE)


def member(z, name, limit=512 * 1024 * 1024):
    info = z.getinfo(name)
    if info.file_size > limit:
        raise ValueError('review read bound exceeded for ' + name)
    data = z.read(info)
    if len(data) != info.file_size:
        raise ValueError('short archive member ' + name)
    return data


def js(z, name):
    return json.loads(member(z, name))


def archive(a):
    path = HERE / '11588677565-F5-soak.zip'
    digest = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
    check(str(a['id']) + '/api_size_digest', path.stat().st_size == a['size_in_bytes']
          and 'sha256:' + digest.hexdigest() == a['digest'])
    check(str(a['id']) + '/api_repository', a['workflow_run']['repository_id'] == 1249213794
          and a['workflow_run']['head_repository_id'] == 1249213794 and not a['expired'])
    z = zipfile.ZipFile(path)
    infos = z.infolist()
    if len(infos) > 4096 or sum(i.file_size for i in infos) > 512 * 1024 * 1024:
        raise ValueError('archive review input bound exceeded')
    names = [i.filename for i in infos]
    regular = all(not i.is_dir() and not i.flag_bits & 1 and
                  (stat.S_IFMT(i.external_attr >> 16) in (0, stat.S_IFREG)) and
                  not PurePosixPath(i.filename).is_absolute() and
                  all(c not in ('', '.', '..') for c in i.filename.split('/')) and '\\' not in i.filename
                  for i in infos)
    check(str(a['id']) + '/archive_regular_unique_members', regular and len(set(names)) == len(names))
    hashes = {}
    for info in infos:
        h = hashlib.sha256()
        actual = 0
        with z.open(info) as stream:
            for block in iter(lambda: stream.read(1024 * 1024), b''):
                actual += len(block)
                if actual > info.file_size:
                    raise ValueError('unexpected expanded member length')
                h.update(block)
        if actual != info.file_size:
            raise ValueError('short expanded member')
        hashes[info.filename] = {'bytes': actual, 'sha256': h.hexdigest()}
    result = {'original_zip': str(path), 'zip_bytes': path.stat().st_size,
              'zip_sha256': digest.hexdigest(), 'api_identity': a,
              'zip_members_read_and_crc_sha256_checked': len(infos),
              'uncompressed_bytes_read': sum(i.file_size for i in infos)}
    REPORT['artifacts'][str(a['id'])] = result
    return z, hashes, result


def seal(z, hashes, prefix):
    data = js(z, prefix + 'seal.json')
    expected = data['artifact_inventory']
    actual = {name[len(prefix):]: entry for name, entry in hashes.items()
              if name.startswith(prefix) and name != prefix + 'seal.json'}
    check(prefix + 'complete_seal_inventory', actual == expected,
          {'expected': len(expected), 'actual': len(actual),
           'different': sorted(k for k in set(actual) | set(expected) if actual.get(k) != expected.get(k))})
    return len(expected)


def source(snapshot, expected_head, label):
    inputs = snapshot['inputs']
    tree = git('ls-tree', '-r', '-z', expected_head, '--', 'crates', 'Cargo.toml', 'Cargo.lock')
    paths = {}
    for item in tree.split(b'\0'):
        if item:
            meta, path = item.split(b'\t', 1)
            mode, kind, oid = meta.decode().split()
            if kind != 'blob' or mode not in ('100644', '100755'):
                raise ValueError('nonregular Git source entry')
            paths[path.decode()] = oid
    objects = list(dict.fromkeys(paths.values()))
    raw = git('cat-file', '--batch', data=('\n'.join(objects) + '\n').encode())
    offset, object_hash = 0, {}
    for expected in objects:
        end = raw.index(b'\n', offset)
        oid, kind, length = raw[offset:end].decode().split()
        length, start = int(length), end + 1
        if oid != expected or kind != 'blob' or raw[start + length:start + length + 1] != b'\n':
            raise ValueError('invalid Git batch response')
        object_hash[oid] = sha(raw[start:start + length])
        offset = start + length + 1
    actual = {p: object_hash[oid] for p, oid in paths.items()}
    check(label + '/complete_git_source', actual == inputs and snapshot['input_count'] == len(actual)
          and snapshot['source_commit'] == expected_head
          and snapshot['source_tree'] == git('rev-parse', expected_head + '^{tree}').decode().strip()
          and snapshot['manifest_sha256'] == sha(canonical(inputs)), {'input_count': len(actual)})
    return {'head': expected_head, 'source_tree': snapshot['source_tree'], 'input_count': len(actual),
            'manifest_sha256': snapshot['manifest_sha256']}


def observer(z, hashes, snapshot, head, prefix, label):
    mismatch = []
    for path, entry in snapshot['files'].items():
        blob = git('show', head + ':' + path)
        mode_blob = git('ls-tree', head, '--', path).decode().split('\t', 1)[0].split()
        actual = {'bytes': len(blob), 'sha256': sha(blob), 'git_mode': mode_blob[0], 'git_blob': mode_blob[2]}
        if entry != actual or hashes[prefix + path] != {'bytes': len(blob), 'sha256': sha(blob)}:
            mismatch.append(path)
    check(label + '/observer_git_and_retained_bytes', not mismatch and len(snapshot['files']) == 9
          and snapshot['source_commit'] == head and snapshot['manifest_sha256'] == sha(canonical(snapshot['files'])), mismatch)
    return {'files': len(snapshot['files']), 'manifest_sha256': snapshot['manifest_sha256']}


def nearest(values, q):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * q) - 1)] if values else None


def interval(values, q):
    if not values:
        return None
    values = sorted(values)
    n = len(values)
    # Independent PMF via lgamma rather than copying the producer recurrence.
    logs = [math.lgamma(n + 1) - math.lgamma(k + 1) - math.lgamma(n - k + 1)
            + k * math.log(q) + (n - k) * math.log1p(-q) for k in range(n + 1)]
    pivot = max(logs)
    pmf = [math.exp(v - pivot) for v in logs]
    total = math.fsum(pmf)
    def count(p):
        cumulative = 0.0
        for k, mass in enumerate(pmf):
            cumulative += mass / total
            if cumulative >= p:
                return k
        return n
    lower, upper = count(.025), count(.975)
    return {'quantile': q, 'confidence': .95, 'samples': n, 'estimate_us': nearest(values, q),
            'lower_us': values[lower - 1] if lower > 0 else None,
            'upper_us': values[upper] if upper < n else None,
            'method': 'binomial_order_statistics_95pct_iid_assumption'}


def timing(values, expected):
    return {'recorded_samples': len(values), 'missing_samples': max(0, expected - len(values)),
            'distribution': {'samples': len(values), 'p50_us': nearest(values, .5),
                             'p95_us': nearest(values, .95), 'max_us': max(values) if values else None,
                             'tail_claim': 'empirical_distribution_only' if len(values) >= 200 else 'insufficient_for_tail_claim'},
            'p95_ci': interval(values, .95), 'p99_ci': interval(values, .99)}


def backfill(a):
    z, hashes, out = archive(a)
    with z:
        label = 'backfill'
        out['sealed_files'] = seal(z, hashes, '')
        receipt = js(z, 'receipt.json')
        head = a['workflow_run']['head_sha']
        out['source'] = source(receipt['source_before'], head, label)
        out['observer'] = observer(z, hashes, receipt['observer_before'], head, 'observer-source/', label)
        check(label + '/unchanged_execution_identities', receipt['source_before'] == receipt['source_after'] == receipt['source_final']
              and receipt['observer_before'] == receipt['observer_after'] == receipt['observer_final']
              and receipt['toolchain_before'] == receipt['toolchain_after'] == receipt['toolchain_final']
              and js(z, 'source-before.json') == receipt['source_before'] and js(z, 'source-after.json') == receipt['source_after'])
        check(label + '/receipt_file_hashes', receipt['files'] == {n:v['sha256'] for n,v in hashes.items() if n not in ('receipt.json','seal.json')})
        events = [json.loads(x) for x in member(z, 'build.jsonl').splitlines()]
        event = receipt['cargo_artifact']
        check(label + '/fresh_semantic_release_cargo_event', event in events and event['fresh'] is False
              and event['features'] == ['semantic'] and event['profile'] == {'debug_assertions': False, 'debuginfo': 0, 'opt_level': '3', 'overflow_checks': False, 'test': True}
              and '--no-default-features' in receipt['build_command'] and '--locked' in receipt['build_command']
              and '--offline' in receipt['build_command'] and receipt['target_initially_absent'] is True
              and event['target']['src_path'].endswith('/crates/cc-eval/tests/p7_worker_contention.rs')
              and event['manifest_path'].endswith('/crates/cc-eval/Cargo.toml'))
        check(label + '/binary_copy_and_build_logs', receipt['copy_source']['path'] == event['executable']
              and {'bytes': receipt['copy_source']['bytes'], 'sha256': receipt['copy_source']['sha256']} == hashes['p7_worker_contention']
              and receipt['executable_sha256'] == receipt['executable_sha256_after'] == hashes['p7_worker_contention']['sha256']
              and receipt['cargo_log_sha256'] == hashes['build.jsonl']['sha256']
              and receipt['stderr_sha256'] == hashes['build.stderr']['sha256'])
        check(label + '/original_test_success', receipt['build_exit_code'] == receipt['execution_exit_code'] == receipt['exit_code'] == 0
              and receipt['status'] == 'passed_observation' and b'1 passed; 0 failed; 0 ignored' in member(z, 'execution.stdout')
              and 'real_slow_backfill_preserves_local_progress_and_records_every_request' in receipt['execution_command'])
        seeds = []
        for seed in (7, 19, 43):
            base = 'raw/seed-' + str(seed) + '/'
            protocol = js(z, base + 'protocol.json')
            summary = js(z, base + 'summary.json')
            before = js(z, base + 'held-before.json')
            sl = label + '/seed-' + str(seed)
            check(sl + '/original_protocol', protocol['seed'] == seed and protocol['concurrency_cells'] == [1,4,8,16]
                  and protocol['samples_per_cell'] == 32 and protocol['phases'] == ['quiet','held']
                  and protocol['query_watchdog_ms'] == 2000 and protocol['progress_watchdog_ms'] == 5000
                  and protocol['writer_busy_timeout_ms'] == 100 and protocol['worker_local_attempt_width'] == 4)
            cells = []
            for phase in ('quiet','held'):
                rows = js(z, base + phase + '-requests.json')
                check(sl + '/' + phase + '/all_cells', len(rows) == 128 and Counter(r['concurrency'] for r in rows) == {1:32,4:32,8:32,16:32})
                for c in (1,4,8,16):
                    rs = [r for r in rows if r['concurrency'] == c]
                    values = [r['offered_to_api_return_us'] for r in rs]
                    cell = {'phase': phase, 'concurrency': c, 'n': len(rs),
                            'p50_us': nearest(values,.5), 'p95_us': nearest(values,.95),
                            'p99_us': nearest(values,.99), 'max_us': max(values)}
                    claimed = next(s for s in summary[phase] if s['concurrency'] == c)
                    valid = all(any(h.get('file_path') == 'stable.rs' and h.get('symbol_name') == 'stable_signal' for h in r['hits'])
                                and 0 <= r['offered_to_api_return_us'] - sum(r[k] for k in ('caller_schedule_us','capture_admission_us','retrieval_us')) <= 2 for r in rs)
                    check(sl + '/' + phase + '/C' + str(c), sorted(r['ordinal'] for r in rs) == list(range(32))
                          and valid and max(values) < 2_000_000 and all(claimed[k] == cell[k] for k in ('concurrency','n','p50_us','p95_us','p99_us','max_us')))
                    cells.append(cell)
            db = summary['db_availability']
            q = summary['queue_final']
            check(sl + '/held_and_final_lifecycle', before['provider']['active'] == before['provider']['waiting'] == 4
                  and before['queue']['claimed'] == 4 and before['queue']['pending'] > 0 and before['queue']['uncovered'] > 0
                  and summary['old_provider']['active'] == summary['old_provider']['waiting'] == 0
                  and summary['old_provider']['maximum_active'] <= 4
                  and q['claimed'] == q['pending'] == q['uncovered'] == 0
                  and summary['old_held_input_publications'] == 0 and summary['new_provider_calls'] > 0)
            check(sl + '/database_available_without_generation_change', db['generation_before'] == db['generation_after']
                  and db['generation_unchanged'] is True and db['read_rows'] > 0
                  and db['writer_busy_timeout_ms'] == 100 and db['writer_acquire_and_rollback_us'] < 100000)
            stages = [summary['quiet_resources'], before['resources'], summary['held_after_resources'], summary['final_resources']]
            usage = [s['shared_process_owner']['usage'] for s in stages]
            check(sl + '/resource_attribution', len({u['pid'] for u in usage}) == 1
                  and all(s['resource_gate'] == 'attributed_combined_process' and s['server']['separate_pid'] is None
                          and s['server_tree']['child_process_usage'] is None for s in stages)
                  and all(u['peak_resident_bytes'] > 0 for u in usage)
                  and all([u[k] for u in usage] == sorted(u[k] for u in usage) for k in ('user_cpu_ns','system_cpu_ns')))
            seeds.append({'seed': seed, 'cells': cells, 'held_queue': before['queue'], 'held_provider': before['provider'],
                          'final_queue': q, 'old_provider_final': summary['old_provider'], 'new_provider_calls': summary['new_provider_calls'],
                          'old_held_input_publications': summary['old_held_input_publications'], 'db_availability': db,
                          'progress_us': {k:summary[k] for k in ('initial_build_us','held_build_us','write_delete_us','switch_and_drain_us')},
                          'resource_stages': len(stages), 'shared_process_pid': usage[0]['pid']})
        out.update({'receipt_sha256': hashes['receipt.json']['sha256'], 'execution_elapsed_ns': receipt['execution_elapsed_ns'],
                    'binary_sha256': hashes['p7_worker_contention']['sha256'], 'seeds': seeds,
                    'retained_requests': sum(c['n'] for s in seeds for c in s['cells']),
                    'scope_limits': ['Seeded in-process CodeIndex/fake-provider control; no external provider.',
                                     '32 samples per C/phase/seed, nearest-rank descriptive tails; no CI or new SLA.',
                                     'Combined process resources; no separate component sum or complete child-process attribution.',
                                     'Missing original Cargo target after download is not a new-build verification; only retained original evidence is audited.']})


def occupancy(rows, start, end):
    events = [(r[start],1) for r in rows] + [(r[end],-1) for r in rows]
    active = peak = 0
    for _, delta in sorted(events):
        active += delta
        peak = max(peak, active)
    return peak, active


def audit_rpc(z, prefix):
    pending, sent, wires, responses = {}, set(), {}, set()
    counts, methods, terminals = Counter(), Counter(), []
    index, tool_hashes = [], {'index': Counter(), 'search': Counter(), 'status': Counter()}
    errors = []
    with z.open(prefix + '/rpc.jsonl') as stream:
        for line in stream:
            r = json.loads(line)
            event, payload = r['event'], r.get('payload', {})
            counts[event] += 1
            if event == 'request':
                ident = payload['id']
                if ident in pending: errors.append('duplicate request')
                pending[ident] = payload
                method = payload.get('params',{}).get('name', payload['method'])
                methods[method] += 1
                if method == 'index': index.append({'id':ident,'arguments':payload['params']['arguments'],'time_ns':r['time_ns']})
            elif event == 'rpc_send':
                ident = r['request_id']
                if ident in sent or r['sent_ns'] < r['offered_ns']: errors.append('invalid send')
                sent.add(ident)
            elif event == 'stdout_wire':
                raw = r['text'].encode()
                if len(raw) != r['wire_bytes'] or sha(raw) != r['wire_sha256']: errors.append('wire mismatch')
                v = json.loads(raw)
                if v['id'] in wires: errors.append('duplicate wire')
                wires[v['id']] = sha(canonical(v))
            elif event == 'response':
                ident = payload['id']
                if ident in responses or ident not in pending or wires.get(ident) != sha(canonical(payload)):
                    errors.append('response identity/bytes mismatch')
                responses.add(ident)
                request = pending.get(ident,{})
                method = request.get('params',{}).get('name')
                if method in tool_hashes:
                    result = payload.get('result',{})
                    if result.get('isError') is True or 'error' in payload: errors.append('tool error')
                    value = result.get('structuredContent',{})
                    value = value.get('result', value)
                    tool_hashes[method][sha(canonical(value))] += 1
            elif event == 'terminal': terminals.append(r)
    check(prefix + '/all_rpc_wire_response_pairs', not errors and set(pending) == sent == set(wires) == responses,
          {'requests':len(pending),'responses':len(responses),'errors':errors[:10]})
    check(prefix + '/process_terminal', len(terminals) == 2 and {t['kind'] for t in terminals} == {'stdout_eof','process_exit'}
          and all(t.get('exit_code',0) == 0 for t in terminals))
    return {'event_counts':dict(counts),'methods':dict(methods),'index_calls':index,
            'terminals':terminals,'response_hashes':tool_hashes}


def sqlite_compare(z, out, parity):
    with tempfile.TemporaryDirectory(prefix='p8-original-db-review-') as tmp:
        conns = []
        for side in ('project','fresh-full'):
            data = member(z, 'p8-runtime/' + side + '/.codecortex/index.sqlite3', 32 * 1024 * 1024)
            p = Path(tmp) / (side + '.sqlite3')
            with p.open('xb') as stream: stream.write(data)
            c = sqlite3.connect(p.as_uri() + '?mode=ro&immutable=1', uri=True)
            c.execute('PRAGMA query_only=ON')
            c.execute('BEGIN')
            check('soak/' + side + '/sqlite_integrity_fk', c.execute('PRAGMA integrity_check').fetchall() == [('ok',)]
                  and c.execute('PRAGMA foreign_key_check').fetchall() == [])
            conns.append(c)
        tables = []
        try:
            for table in TABLES:
                columns = [[r[1] for r in c.execute('PRAGMA table_info("' + table + '")')
                            if not (table == 'files' and r[1] in ('mtime','indexed_at') or table == 'imports' and r[1] == 'id')]
                           for c in conns]
                query = 'SELECT ' + ','.join('"' + n.replace('"','""') + '"' for n in columns[0]) + ' FROM "' + table + '"'
                # Typed complete tuples preserve multiplicity; BLOB bytes are compared directly (stronger than a digest).
                rows = [Counter(tuple((type(v).__name__,v) for v in r) for r in c.execute(query)) for c in conns]
                counts = [sum(rs.values()) for rs in rows]
                claim = next(t for t in parity['comparison']['tables'] if t['table'] == table)
                eq = rows[0] == rows[1]
                check('soak/sqlite/' + table, bool(columns[0]) and columns[0] == columns[1] and eq
                      and counts == [claim['incremental_rows'],claim['full_rows']] and claim['equal'] is True and claim['different_row_count'] == 0)
                tables.append({'table':table,'incremental_rows':counts[0],'full_rows':counts[1],
                               'projected_columns':columns[0],'complete_typed_multisets_equal':eq})
        finally:
            for c in conns: c.close()
    out['independent_sqlite_comparison'] = {'method':'Read-only immutable SQLite copies; exact existing 15-table exclusions; every typed row and duplicate compared; no product/oracle executable run',
                                            'tables':tables,'rows_per_side':sum(t['incremental_rows'] for t in tables)}


def soak(a):
    z, hashes, out = archive(a)
    with z:
        out['sealed_files'] = {'build':seal(z,hashes,'p8-build/'),'runtime':seal(z,hashes,'p8-runtime/')}
        receipt, plan, report = [js(z,p) for p in ('p8-build/build-receipt.json','p8-runtime/plan.json','p8-runtime/report.json')]
        head = a['workflow_run']['head_sha']
        out['source'] = source(receipt['source_before'], head, 'soak')
        out['observer'] = observer(z,hashes,receipt['observer_before'],head,'p8-build/observer-source/','soak')
        check('soak/unchanged_source_observer_toolchain', receipt['source_before'] == receipt['source_after'] == plan['source'] == plan['build_identity']['source']
              and receipt['observer_before'] == receipt['observer_after'] == plan['build_identity']['observer']
              and receipt['toolchain_before'] == receipt['toolchain_after'] == plan['build_identity']['toolchain']
              and plan['build_identity'] == report['final_build_verification'])
        check('soak/retained_build_evidence_exact', all(hashes['p8-runtime/build-evidence/' + p] == hashes['p8-build/' + p] == entry
              for p,entry in plan['retained_build_evidence'].items())
              and plan['build_identity']['build_seal_sha256'] == hashes['p8-build/seal.json']['sha256']
              and plan['build_identity']['receipt_sha256'] == hashes['p8-build/build-receipt.json']['sha256'])
        events = [json.loads(x) for x in member(z,'p8-build/product-build.jsonl').splitlines()]
        for name, record in receipt['artifacts'].items():
            event = record['cargo_artifact']
            check('soak/build/' + name, event in events and event['fresh'] is False and event['features'] == []
                  and event['profile']['opt_level'] == '3' and event['profile']['debug_assertions'] is False
                  and event['target']['name'] == name and event['target']['kind'] == ['bin']
                  and record['binary_sha256'] == record['copy_source']['sha256'] == hashes['p8-build/' + name]['sha256']
                  and record['binary_bytes'] == record['copy_source']['bytes'] == hashes['p8-build/' + name]['bytes']
                  and record['copy_source']['path'] == event['executable'])
        check('soak/fresh_locked_offline_build', receipt['target_initially_absent'] is True and receipt['build_exit_code'] == 0
              and all(f in receipt['build_command'] for f in ('--release','--locked','--offline','--no-default-features'))
              and hashes['p8-build/product-build.jsonl']['sha256'] == receipt['cargo_log_sha256']
              and hashes['p8-build/product-build.stderr']['sha256'] == receipt['stderr_sha256'])
        for field,path in [('raw_sha256','raw.jsonl'),('plan_sha256','plan.json'),('parity_sha256','parity.json')]:
            check('soak/report_hash/' + path, report[field] == hashes['p8-runtime/' + path]['sha256'])
        rows = [json.loads(line) for line in member(z,'p8-runtime/raw.jsonl').splitlines()]
        kinds = Counter(r['kind'] for r in rows)
        ops = [r for r in rows if r['kind'] == 'operation']
        resources = [r for r in rows if r['kind'] == 'resources']
        by_id = sorted(ops,key=lambda r:r['id'])
        check('soak/all_offered_ids_and_statuses', [r['id'] for r in by_id] == list(range(3601))
              and plan['operations'] == report['offered'] == len(ops) == 3601
              and Counter(r['status'] for r in ops) == report['outcomes'] == {'success':3601}
              and plan['profile'] == report['profile'] == 'soak' and plan['files'] == 1000)
        check('soak/complete_schedule_and_ordered_timings', plan['offer_schedule'] == 'uniform' and plan['offer_interval_ms'] == 1000
              and all(r['scheduled_ns'] == by_id[0]['scheduled_ns'] + r['id'] * 1_000_000_000
                      and r['scheduled_ns'] <= r['offered_ns'] <= r['started_ns'] <= r['call_started_ns'] <= r['finished_ns']
                      and r['operation'] == ('build' if r['id'] % 3 == 0 else 'read') for r in ops))
        reads, builds = [r for r in ops if r['operation'] == 'read'], [r for r in ops if r['operation'] == 'build']
        check('soak/all_original_valid_responses', all(any(h.get('name') == 'p8_runtime_stable_signal' and h.get('file_path') == 'stable.py' for h in r['response']) for r in reads)
              and all(r['response']['parse_errors'] == [] and r['response']['resolution_freshness']['complete'] is True for r in builds))
        ordered = sorted(builds,key=lambda r:r['mutation_ordinal'])
        check('soak/all_six_mutation_cycle', [r['mutation_ordinal'] for r in ordered] == list(range(1201))
              and all(r['mutation']['action'] == ACTIONS[r['mutation_ordinal'] % 6] for r in ordered))
        branches = [r['mutation'] for r in ordered if r['mutation']['action'] == 'real_git_branch_switch']
        branch_heads = {name:member(z,'p8-runtime/project/.git/refs/heads/' + name).decode().strip() for name in ('p8-a','p8-b')}
        check('soak/real_branch_receipts', len(branches) == report['real_branch_switches'] == 200
              and all(b['exit_code'] == 0 and b['branch'] == ('p8-b' if i % 2 == 0 else 'p8-a')
                      and b['commit'] == branch_heads[b['branch']] and 'core.hooksPath=/dev/null' in b['argv']
                      and 'switch' in b['argv'] for i,b in enumerate(branches)))
        compactions = member(z,'p8-runtime/product/product-stderr.log').count(b'resolver catalog dropped for tombstone compaction')
        check('soak/actual_catalog_compaction_log', compactions == report['observed_catalog_compactions'] and compactions > 0)
        source_parts = {}
        for side in ('project','fresh-full'):
            prefix = 'p8-runtime/' + side + '/'
            source_parts[side] = {n[len(prefix):]:v for n,v in hashes.items() if n.startswith(prefix) and not n.startswith(prefix + '.codecortex/')}
        check('soak/full_control_identical_source_and_git_copy', source_parts['project'] == source_parts['fresh-full'])
        coverage = report['resource_time_coverage']
        times = [r['at_ns'] for r in resources]
        gap = max([max(0,times[0]-coverage['work_start_ns']),max(0,coverage['work_end_ns']-times[-1])] + [b-a for a,b in zip(times,times[1:])])
        rss = [r['server']['resident_bytes'] for r in resources]
        quarter = len(rss)//4
        median = lambda vs:sorted(vs)[len(vs)//2]
        warmed, tail = median(rss[quarter:quarter*2]), median(rss[quarter*3:])
        allowed = warmed + warmed//4 + 32*1024*1024
        check('soak/resource_time_coverage', times == sorted(set(times)) and gap == coverage['maximum_gap_ns'] <= 5_000_000_000
              and coverage['allowed_maximum_gap_ns'] == 5_000_000_000 and coverage['observed_samples'] == len(resources)
              and coverage['first_ns'] == times[0] and coverage['last_ns'] == times[-1] and coverage['passed'] is True)
        check('soak/original_rss_rule', all(type(v) is int and v > 0 for v in rss)
              and report['rss']['warmed_median_bytes'] == warmed and report['rss']['tail_median_bytes'] == tail
              and report['rss']['allowed_bytes'] == allowed and report['rss']['sampled_peak_bytes'] == max(rss)
              and report['rss']['observed'] == len(rss) and tail <= allowed and report['rss']['passed'] is True)
        check('soak/actual_hour', by_id[-1]['scheduled_ns']-by_id[0]['scheduled_ns'] >= 3_600_000_000_000
              and max(r['finished_ns'] for r in ops)-min(r['offered_ns'] for r in ops) >= 3_600_000_000_000
              and report['observed_work_ns'] >= 3_600_000_000_000)
        peak, active = occupancy(ops,'started_ns','finished_ns')
        waiting_peak,_ = occupancy(ops,'offered_ns','started_ns')
        calls_peak,_ = occupancy(ops,'call_started_ns','finished_ns')
        endpoint = next(r['response'] for r in rows if r['kind'] == 'endpoint_status')
        check('soak/endpoint_queue_and_pins_drained', all(endpoint['query_execution'][k] == 0 for k in ('async_admitted','async_in_flight','cpu_admitted','cpu_in_flight','rejected'))
              and endpoint['retrieval']['query_pins'] == 0 and active == 0 and calls_peak == report['actual_concurrency']['maximum'])
        rpc = audit_rpc(z,'p8-runtime/product')
        full_rpc = audit_rpc(z,'p8-runtime/full-product')
        check('soak/no_incremental_side_repair', len(rpc['index_calls']) == len(builds)+1
              and rpc['index_calls'][0]['arguments']['full'] is True
              and all(c['arguments']['full'] is False for c in rpc['index_calls'][1:])
              and len(full_rpc['index_calls']) == 1 and full_rpc['index_calls'][0]['arguments']['full'] is True)
        initial = next(r['report'] for r in rows if r['kind'] == 'initial_build')
        full = next(r['report'] for r in rows if r['kind'] == 'full_control')
        public = next(r for r in rows if r['kind'] == 'endpoint_public')
        check('soak/raw_builds_equal_actual_wire_responses', rpc['response_hashes']['index'] == Counter(sha(canonical(r['response'])) for r in builds) + Counter([sha(canonical(initial))])
              and full_rpc['response_hashes']['index'] == Counter([sha(canonical(full))]))
        check('soak/raw_queries_equal_actual_wire_responses', rpc['response_hashes']['search'] == Counter(sha(canonical(call['response'])) for r in reads for call in r['cache_probe']['requests'] if call['name']=='search') + Counter([sha(canonical(public['incremental']))])
              and full_rpc['response_hashes']['search'] == Counter([sha(canonical(public['full']))]) and public['incremental'] == public['full'])
        processes = {side:js(z,'p8-runtime/' + side + '/process.json') for side in ('product','full-product')}
        check('soak/owned_processes_closed_and_native_pid', all(p['cleanup'] == 'completed' and p['exit_code'] == p['expected_exit_code'] == 0
              and p['binary_sha256'] == hashes['p8-build/codecortex']['sha256'] for p in processes.values())
              and {r['server']['pid'] for r in resources} == {processes['product']['pid']}
              and processes['product']['pid'] != processes['full-product']['pid'])
        stats = js(z,'p8-runtime/statistics.json')
        check('soak/both_original_statistics_outputs_identical', member(z,'p8-runtime/statistics.json') == member(z,'p8-runtime/statistics-replay.json')
              and stats['raw_sha256'] == hashes['p8-runtime/raw.jsonl']['sha256']
              and stats['plan_sha256'] == hashes['p8-runtime/plan.json']['sha256']
              and stats['replay_binary_sha256'] == hashes['p8-build/p8-runtime-statistics']['sha256']
              and report['statistics']['sha256'] == hashes['p8-runtime/statistics.json']['sha256']
              and all(r['exit_code'] == 0 for r in js(z,'p8-runtime/statistics-execution.json')))
        groups = []
        for group in stats['by_operation'] + stats['by_build_mutation']:
            selected = [r for r in ops if r['operation'] == group['operation']
                        and (group['mutation_action'] is None or r.get('mutation',{}).get('action') == group['mutation_action'])]
            n = len(selected)
            all_times = [(r['finished_ns']-r['offered_ns'])//1000 for r in selected]
            successful = [(r['finished_ns']-r['offered_ns'])//1000 for r in selected if r['status'] == 'success']
            views = {'all_attempt_offered_to_terminal':(all_times,n),'successful_offered_to_terminal':(successful,len(successful)),
                     'scheduled_to_terminal':([(r['finished_ns']-r['scheduled_ns'])//1000 for r in selected],n),
                     'client_dispatch_wait':([(r['started_ns']-r['offered_ns'])//1000 for r in selected],n),
                     'write_admission_and_preparation':([(r['call_started_ns']-r['started_ns'])//1000 for r in selected],n),
                     'client_call_to_terminal_including_validation':([(r['finished_ns']-r['call_started_ns'])//1000 for r in selected],n)}
            name = group['operation'] + '/' + str(group['mutation_action'])
            check('soak/statistics_recompute/' + name, group['recorded_samples'] == group['expected_samples_in_group'] == n
                  and group['outcomes'] == dict(Counter(r['status'] for r in selected))
                  and all(group[k] == timing(v,e) for k,(v,e) in views.items()))
            groups.append({'operation':group['operation'],'mutation_action':group['mutation_action'],'n':n,
                           'all_attempt_offered_to_terminal':timing(all_times,n)})
        parity = js(z,'p8-runtime/parity.json')
        check('soak/original_complete_oracle', parity['exit_code'] == 0 and parity['tables'] == TABLES
              and parity['scope'] == 'complete_existing_15_table_diagnostic_oracle_no_repair'
              and parity['comparison']['equal'] is True and parity['comparison']['different_tables'] == []
              and next(r for r in rows if r['kind'] == 'oracle_process')['exit_code'] == 0)
        sqlite_compare(z,out,parity)
        out.update({'report_sha256':hashes['p8-runtime/report.json']['sha256'],
                    'raw_sha256':hashes['p8-runtime/raw.jsonl']['sha256'],'raw_event_counts':dict(kinds),
                    'operations':len(ops),'outcomes':dict(Counter(r['status'] for r in ops)),
                    'mutations':dict(Counter(r['mutation']['action'] for r in builds)),
                    'configured_concurrency':plan['concurrency'],'observed_executor_peak':peak,'observed_workload_call_peak':calls_peak,
                    'observed_waiting_for_executor_peak':waiting_peak,
                    'max_dispatch_wait_ns':max(r['started_ns']-r['offered_ns'] for r in ops),
                    'first_to_last_schedule_ns':by_id[-1]['scheduled_ns']-by_id[0]['scheduled_ns'],
                    'first_actual_offer_to_last_terminal_ns':max(r['finished_ns'] for r in ops)-min(r['offered_ns'] for r in ops),
                    'reported_observed_work_ns':report['observed_work_ns'],
                    'coverage':coverage,'rss':report['rss'],'branch_switches':len(branches),'catalog_compactions':compactions,
                    'endpoint_query_execution':endpoint['query_execution'],'endpoint_query_pins':endpoint['retrieval']['query_pins'],
                    'identical_copied_source_and_git_files':len(source_parts['project']),
                    'processes':processes,'rpc':{k:v for k,v in rpc.items() if k not in ('response_hashes','index_calls')},
                    'full_rpc':{k:v for k,v in full_rpc.items() if k not in ('response_hashes','index_calls')},
                    'independent_statistics_groups':groups,'statistics_output_sha256':hashes['p8-runtime/statistics.json']['sha256'],
                    'statistics_numeric_recompute_scope':'All 8 original groups x 6 timing views, distribution and both existing IID quantile intervals; no executable replay',
                    'scope_limits':['Original F5 GitHub Actions execution only; does not pool local M5/M6,599/G3 or engineering observations.',
                                    'Uniform one offered compound read/build per second under C4 cap; actual peak computed from original timing; not an assumed saturated stress run.',
                                    'Default semantic disabled; separate backfill control and broader P8 dependencies remain.',
                                    'Mixed strata and correlated time series do not certify stable tails or general leak absence.',
                                    'Original artifacts opened as data; remote original Cargo targets/toolchain paths are not current local build verification; reviewer invokes no product executable.']})


def main():
    REPORT['started_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    REPORT['review_repository_head'] = git('rev-parse','HEAD').decode().strip()
    write_report()
    api = json.loads((HERE / 'api-identity.json').read_text())
    by_id = {a['id']:a for a in api['artifacts']}
    backfill(by_id[11573017418])
    write_report()
    soak(by_id[11574556264])
    REPORT['finished_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
    REPORT['failed_checks'] = [r for r in REPORT['checks'] if not r['passed']]
    REPORT['state'] = 'scoped_original_evidence_verified' if not REPORT['failed_checks'] else 'review_findings'
    REPORT['original_tasks_completed_by_this_audit'] = 0
    REPORT['remaining_todos_authoritative_status_unchanged'] = 29
    write_report()
    print(json.dumps({'state':REPORT['state'],'checks':len(REPORT['checks']),'failed_checks':REPORT['failed_checks'],
                      'report':str(HERE/'independent-originals-report.json')},ensure_ascii=False))


if __name__ == '__main__':
    try:
        main()
    except Exception as error:
        REPORT['state'] = 'review_harness_incomplete'
        REPORT['error'] = type(error).__name__ + ': ' + str(error)
        write_report()
        raise
