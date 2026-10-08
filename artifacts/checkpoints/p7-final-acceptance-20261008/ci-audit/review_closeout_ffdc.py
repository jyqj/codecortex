#!/usr/bin/env python3
"""Read-only artifact replay; never runs Cargo or modifies product/source/fixtures."""
import ast
import argparse
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile

ROOT = Path(os.environ['SOURCE_ROOT']).resolve()
ARCHIVE = Path(os.environ['CLOSEOUT_ZIP']).resolve()
COMMIT = 'ffdc6f0f97db78cc25a6c026904e7c2adde05d14'
ZIP_SHA = '1bef831e4dbce560987c09a5745b82708b4f9cf5b0fd8efb426af07f7f9ede23'
DRIVER_PATH = 'artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/lifecycle_stdio.py'
DRIVER_SHA = '39630b6be2bba3319584af74521863ef5d53a4e7fd723de8c3a454b29e34259c'
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, required=True)
parser.add_argument('--expected-members', type=int, required=True)
parser.add_argument('--expected-events', type=int, required=True)
parser.add_argument('--expected-mcp-pairs', type=int, required=True)
parser.add_argument('--expected-original-rpcs', type=int, required=True)
args = parser.parse_args()


def digest(data):
    return hashlib.sha256(data).hexdigest()


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=ROOT, input=data)


def fixed(path):
    return git('show', COMMIT + ':' + path)


def jbytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def one(rows):
    assert len(rows) == 1, ('expected exactly one', len(rows))
    return rows[0]


def span_hash(stream):
    h = hashlib.sha256()
    for data in iter(lambda: stream.read(1048576), b''):
        h.update(data)
    return h.hexdigest()


def tool_value(response):
    result = response['result']
    assert not result.get('isError'), result
    value = result['structuredContent']
    return value.get('result', value)


with ARCHIVE.open('rb') as source:
    assert span_hash(source) == ZIP_SHA
assert ARCHIVE.stat().st_size == 20094853
with zipfile.ZipFile(ARCHIVE) as archive:
    members = archive.namelist()
    assert len(members) == len(set(members)) == args.expected_members
    assert all(not Path(p).is_absolute() and '..' not in Path(p).parts for p in members)
    member_receipts = []
    for info in archive.infolist():
        with archive.open(info) as stream:
            checksum = span_hash(stream)
        member_receipts.append({'member': info.filename, 'bytes': info.file_size, 'sha256': checksum})
    member_hashes = {r['member']: r['sha256'] for r in member_receipts}
    read = archive.read
    def obj(name):
        return json.loads(read(name))
    before, after = obj('source-before.json'), obj('source-after.json')
    assert before == after
    assert before['source_commit'] == COMMIT
    assert before['source_tree'] == git('rev-parse', COMMIT + '^{tree}').decode().strip()
    assert before['input_count'] == len(before['inputs']) == 795
    assert before['manifest_sha256'] == digest(jbytes(before['inputs']))
    paths = sorted(before['inputs'])
    actual_paths = git('ls-tree', '-r', '--name-only', '-z', COMMIT, '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().split('\0')
    assert paths == sorted(p for p in actual_paths if p)
    raw = git('cat-file', '--batch', data=('\n'.join(COMMIT + ':' + p for p in paths) + '\n').encode())
    offset = 0
    for path in paths:
        end = raw.index(b'\n', offset)
        header = raw[offset:end].split()
        assert len(header) == 3 and header[1] == b'blob'
        size = int(header[2])
        data = raw[end + 1:end + 1 + size]
        assert len(data) == size and digest(data) == before['inputs'][path], path
        offset = end + 1 + size
        assert raw[offset:offset + 1] == b'\n'
        offset += 1
    assert offset == len(raw)
    receipt = obj('http-build-receipt.json')
    assert receipt == obj('fault-lifecycle/build-receipt.json')
    assert receipt['source_before'] == receipt['source_after'] == before
    assert receipt['build_exit_code'] == 0
    assert receipt['binary_sha256'] == member_hashes['http-codecortex']
    assert receipt['cargo_log_sha256'] == member_hashes['http-product-build.jsonl']
    assert receipt['lifecycle_driver_sha256'] == digest(fixed(DRIVER_PATH)) == DRIVER_SHA
    artifact = receipt['cargo_artifact']
    assert artifact['reason'] == 'compiler-artifact' and artifact['target']['name'] == 'codecortex'
    assert artifact['target']['kind'] == ['bin'] and sorted(artifact['features']) == ['semantic', 'semantic-http']
    assert artifact['profile']['opt_level'] == '0' and artifact['profile']['debug_assertions'] is True
    assert artifact['profile']['test'] is False
    cargo = [json.loads(line) for line in read('http-product-build.jsonl').splitlines()]
    assert one([r for r in cargo if r.get('reason') == 'compiler-artifact' and r.get('target', {}).get('name') == 'codecortex' and r.get('executable')]) == artifact
    driver = fixed('scripts/p7_fault_lifecycle_stdio.py')
    tools = one([ast.literal_eval(n.value) for n in ast.parse(driver).body if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == 'TOOLS' for t in n.targets)])

    original = obj('lifecycle/summary.json')
    stdio = obj('lifecycle/stdio-raw.json')
    http13 = obj('lifecycle/http-raw.json')
    expected_cases = ['unconfigured', 'disabled', 'authority_absent', 'assembly_failed', 'backfilling', 'ready_cold_then_warm', 'clean_restart_document_reuse_query_cold_then_warm', 'query_authority_revoked', 'network_authority_revoked', 'disabled_after_ready', 'provider_error_retry_pending_1', 'provider_error_retry_pending_2', 'terminal_provider_http500']
    assert original['passed'] is True and original['binary_sha256'] == receipt['binary_sha256']
    assert [r['case'] for r in original['cases']] == expected_cases
    assert [r['sequence'] for r in stdio] == list(range(len(stdio)))
    assert len([r for r in stdio if r['kind'] == 'rpc']) == args.expected_original_rpcs
    assert original['cases'] == [{k:v for k,v in r.items() if k not in ('sequence','kind')} for r in stdio if r['kind'] == 'checkpoint']
    initialized = [r for r in stdio if r['kind'] == 'rpc' and r['request']['method'] == 'initialize']
    closes = [r for r in stdio if r['kind'] == 'close']
    assert len(initialized) == len(closes) == 10
    assert {r['session'] for r in initialized} == {r['session'] for r in closes}
    for close in closes:
        assert close['exit_code'] == 0
        assert close['stderr_sha256'] == member_hashes['lifecycle/' + close['session'] + '-stderr.log']
    listed = [r for r in stdio if r['kind'] == 'rpc' and r['request']['method'] == 'tools/list']
    assert len(listed) == 10
    for row in listed:
        assert {t['name'] for t in row['response']['result']['tools']} == tools
    assert len(http13) == 8 and [r['sequence'] for r in http13] == list(range(8))
    for row in http13:
        assert row['only_loopback'] is True and row['dummy_authorization_matched'] is True
    assert {k:sum(r[k] for r in http13) for k in original['costs']} == original['costs']
    assert original['costs']['document_posts'] == original['costs']['query_posts'] == 4
    assert sum(r['response_status'] == 500 for r in http13) == 3
    cases = {r['case']:r for r in original['cases']}
    for name in ['unconfigured','disabled','authority_absent','assembly_failed','query_authority_revoked','network_authority_revoked','disabled_after_ready']:
        assert cases[name]['delta']['document_posts'] == cases[name]['delta']['query_posts'] == 0
    assert cases['backfilling']['status']['query_pins'] == 1
    assert cases['backfilling']['status']['semantic_pending'] == 1
    assert cases['ready_cold_then_warm']['status']['semantic_state'] == 'ready'
    assert cases['clean_restart_document_reuse_query_cold_then_warm']['delta']['document_posts'] == 0
    assert cases['clean_restart_document_reuse_query_cold_then_warm']['delta']['query_posts'] == 2
    for attempt in [1,2]:
        row=cases[f'provider_error_retry_pending_{attempt}']
        assert row['status']['semantic_pending'] == 1 and row['status']['semantic_failed'] == 0
        assert row['delta']['document_posts'] == attempt
    terminal=cases['terminal_provider_http500']
    assert terminal['status']['semantic_pending'] == 0 and terminal['status']['semantic_failed'] == 1
    assert terminal['delta']['document_posts'] == 3
    assert original['paid_cost'] is None and original['hot_reload'] == 'not_run'

    summary=obj('fault-lifecycle/summary.json')
    assert summary['passed'] is True and summary['complete_declared_matrix'] is True
    assert summary['declared_seeds'] == summary['executed_seeds'] == [197,199,211]
    assert summary['script_sha256'] == digest(driver)
    assert summary['binary_sha256'] == receipt['binary_sha256']
    events=[json.loads(line) for line in read('fault-lifecycle/events.jsonl').splitlines()]
    assert [e['sequence'] for e in events] == list(range(len(events)))
    assert len(events) == args.expected_events
    assert all(b['monotonic'] >= a['monotonic'] and b['unix_time'] >= a['unix_time'] for a,b in zip(events,events[1:]))
    requests={}
    calls=[]
    for event in events:
        if event['kind'] == 'mcp_request':
            request=event['request']; key=(event['session'],request['id'])
            assert key not in requests
            requests[key]=event
        elif event['kind'] == 'mcp_response':
            response=event['response']; key=(event['session'],response['id'])
            req=requests.pop(key)
            assert req['sequence'] < event['sequence'] and 'error' not in response
            if req['request']['method'] == 'tools/list':
                assert {t['name'] for t in response['result']['tools']} == tools
            if req['request']['method'] == 'tools/call':
                calls.append({'request_event':req,'response_event':event,'name':req['request']['params']['name'],'arguments':req['request']['params']['arguments'],'value':tool_value(response)})
    assert not requests
    assert len([e for e in events if e['kind']=='mcp_request']) == args.expected_mcp_pairs
    seeds=[]
    for seed in [197,199,211]:
        result=obj(f'fault-lifecycle/seed-{seed}/result.json')
        assert one([r for r in summary['results'] if r['seed']==seed]) == result
        assert result['passed'] is True
        config=result['configuration']['semantic']
        assert config['worker_lease_secs'] == 2 and config['gc_min_retention_secs'] == 1 and config['retry_max_attempts'] == 1
        assert config['allow_query_network'] is False
        checkpoints={r['phase']:r for r in result['checkpoints']}
        expected_phases=['initial_ready','clean_restart_zero_document_calls','rebuild_delete_http500_pending_and_automatic_gc','sigkill_lease_reclaimed_ready','final_restart_cache_reuse_and_read_only_status']
        assert list(checkpoints) == expected_phases
        phases={e['phase']:e for e in events if e['kind']=='phase' and e['seed']==seed}
        db={e['phase']:e for e in events if e['kind']=='database_read_only' and e['seed']==seed}
        pending=one(db['http500_pending']['snapshot']['outbox'])
        claimed=one(db['held_before_sigkill']['snapshot']['outbox'])
        done=one(db['recovered_ready']['snapshot']['outbox'])
        assert [r['attempt_count'] for r in [pending,claimed,done]] == [1,2,3]
        assert [r['state'] for r in [pending,claimed,done]] == ['pending','claimed','done']
        assert len({(r['task_id'],r['doc_key'],r['doc_version'],r['input_digest']) for r in [pending,claimed,done]}) == 1
        sessions={e['session']:e for e in events if e['kind']=='spawn' and e['session'].startswith(f'seed-{seed}-')}
        exits={e['session']:e for e in events if e['kind']=='product_exit' and e['session'].startswith(f'seed-{seed}-')}
        assert len(sessions) == len(exits) == 4 and set(sessions)==set(exits)
        for name,exit in exits.items():
            assert sessions[name]['command'][0] == receipt['binary_path']
            assert exit['cleanup'] is False and exit['sequence'] > sessions[name]['sequence']
            assert exit['stderr_sha256'] == member_hashes['fault-lifecycle/' + name + '.stderr.log']
            assert exit['exit_code'] == (-9 if name.endswith('warm-restart') else 0)
            assert exit['requested_sigkill'] == name.endswith('warm-restart')
        killed=exits[f'seed-{seed}-warm-restart']
        deadline=claimed['lease_expires_at']
        assert db['held_before_sigkill']['unix_time'] <= killed['unix_time'] < deadline
        assert checkpoints['sigkill_lease_reclaimed_ready']['kill_time'] <= killed['unix_time']
        assert checkpoints['sigkill_lease_reclaimed_ready']['lease_deadline'] == deadline
        elapsed={e['phase']:e for e in events if e['kind']=='real_deadline_elapsed' and e['seed']==seed}
        for name,e in elapsed.items():
            assert e['finished_at'] >= e['deadline'] and e['sequence'] > phases[name]['sequence']
        assert elapsed['age_real_cache']['finished_at']-elapsed['age_real_cache']['started_at'] >= 1.2
        leasewait=elapsed['real_lease_expiry_after_sigkill']
        assert leasewait['started_at'] >= killed['unix_time']
        assert leasewait['deadline'] == deadline + .05
        assert sessions[f'seed-{seed}-crash-recovery']['unix_time'] > leasewait['finished_at'] > deadline
        http=result['http']
        assert len(http)==result['observed_document_posts']==result['observed_document_inputs']==5
        assert [r['request_number'] for r in http] == [1,2,3,4,5]
        assert [r['phase'] for r in http] == ['initial','initial','rebuilt-provider-500','held_retry_before_sigkill','recovered_after_sigkill']
        assert [r['response_status'] for r in http] == [200,200,500,200,200]
        assert [r['response_write_completed'] for r in http] == [True,True,True,False,True]
        assert http[3]['disconnect'] == 'BrokenPipeError'
        assert all(r['product_response_consumption']=='unknown' and r['actual_paid_cost'] is None for r in http)
        http_events=[e for e in events if e['kind']=='http_request' and e['seed']==seed]
        assert len(http_events)==5
        assert [e['received_at'] for e in http_events] == [r['received_at'] for r in http]
        assert pending['available_at']-http[2]['received_at'] >= 29
        assert http[3]['received_at'] > elapsed['unchanged_30_second_backoff']['finished_at'] > pending['available_at']
        index_retry=one([c for c in calls if c['name']=='index' and c['request_event']['session']==f'seed-{seed}-warm-restart' and elapsed['unchanged_30_second_backoff']['sequence'] < c['request_event']['sequence'] < http_events[3]['sequence']])
        stable=[c for c in calls if c['name']=='status' and phases['rebuild_delete_http500_pending_and_automatic_gc']['sequence'] < c['request_event']['sequence'] < index_retry['request_event']['sequence']]
        assert len(stable)>=6
        pending_generation=checkpoints['rebuild_delete_http500_pending_and_automatic_gc']['status']['generation']
        assert all(c['value']['retrieval']['generation']==pending_generation and c['value']['retrieval']['semantic_pending']==1 for c in stable)
        assert http[4]['received_at'] > sessions[f'seed-{seed}-crash-recovery']['unix_time']
        for session,phase in [('warm-restart','clean_restart_zero_document_calls'),('final-cache-reuse','final_restart_cache_reuse_and_read_only_status')]:
            start=sessions[f'seed-{seed}-{session}']['unix_time']; end=phases[phase]['unix_time']
            assert not any(start <= row['received_at'] <= end for row in http)
            assert checkpoints[phase]['document_posts_delta']==0
        assert [r['observed_document_posts'] for r in result['checkpoints']] == [2,2,3,5,5]
        initial=checkpoints['initial_ready']; rebuild=checkpoints['rebuild_delete_http500_pending_and_automatic_gc']; recovered=checkpoints['sigkill_lease_reclaimed_ready']; final=checkpoints['final_restart_cache_reuse_and_read_only_status']
        assert initial['status']['generation']['incarnation'] != rebuild['status']['generation']['incarnation']
        assert initial['status']['dense_published']==2 and rebuild['status']['dense_published']==0
        assert recovered['status']['semantic_state']==final['status']['semantic_state']=='ready'
        assert recovered['status']['dense_published']==final['status']['dense_published']==1
        assert len(result['cache_initial'])==2 and rebuild['cache_after_gc']=={} and len(result['cache_final'])==1
        assert not set(result['cache_initial']).intersection(result['cache_final'])
        for name,value in result['cache_final'].items():
            assert member_hashes[f'fault-lifecycle/seed-{seed}/semantic-cache/{name}'] == value['sha256']
        stderr=read(f'fault-lifecycle/seed-{seed}-warm-restart.stderr.log').decode()
        stderr=re.sub(r'\x1b\[[0-9;]*m','',stderr)
        gc=one([line for line in stderr.splitlines() if 'semantic worker GC page completed' in line and 'deleted_objects=2' in line])
        gc_time=datetime.datetime.fromisoformat(gc.split()[0].replace('Z','+00:00')).timestamp()
        assert http[2]['received_at'] < gc_time < db['http500_pending']['unix_time']
        source=read(f'fault-lifecycle/seed-{seed}/project/keep.rs').decode()
        local_calls=[c for c in calls if c['request_event']['session'].startswith(f'seed-{seed}-') and c['name'] in ['search','context']]
        assert len(local_calls)==12
        for c in local_calls:
            if c['arguments'].get('mode')=='symbol':
                assert c['value']==[]
            else:
                assert c['arguments']['retrieval_strategy']=='local'
                hits=c['value']['machine_pack']['hits']; assert hits
                assert all(h['file_path']=='keep.rs' and h['text'] and h['text'] in source for h in hits)
                assert all(s['file_path']=='keep.rs' for s in c['value']['spans'])
        for phase in ['initial_ready','sigkill_lease_reclaimed_ready','final_restart_cache_reuse_and_read_only_status']:
            previous=[c for c in calls if c['name']=='status' and c['response_event']['sequence'] < phases[phase]['sequence']][-7:]
            assert len(previous)==7
            assert all(c['value']['retrieval']['generation']==checkpoints[phase]['status']['generation'] for c in previous)
        assert result['paid_cost'] is None and result['billing_unknown_after_crash'] is True and result['manual_database_or_clock_changes'] is False
        seeds.append({'seed':seed,'actual_exit_sequence':killed['sequence'],'actual_exit_unix_time':killed['unix_time'],'claimed_sequence':db['held_before_sigkill']['sequence'],'lease_expires_at':deadline,'exit_precedes_expiry_seconds':deadline-killed['unix_time'],'recovery_spawn_unix_time':sessions[f'seed-{seed}-crash-recovery']['unix_time'],'retry_scheduled_delay_seconds':pending['available_at']-http[2]['received_at'],'actual_http_retry_gap_seconds':http[3]['received_at']-http[2]['received_at'],'observed_document_posts':5,'zero_post_warm_restarts':2,'readonly_pending_status_observations':len(stable),'local_query_observations':len(local_calls),'gc_deleted_objects':2,'gc_log_unix_time':gc_time,'final_cache_objects':1,'exits':{name:e['exit_code'] for name,e in exits.items()},'attempts':[1,2,3],'result_sha256':member_hashes[f'fault-lifecycle/seed-{seed}/result.json']})

    logs=['db-rebuild-faults.log','db-lifecycle.log','semantic-fault-matrix.log','default-wiring.log','semantic-wiring.log','http-runtime.log','http-wiring.log','semantic-config.log','self-resources.log','worker-strategy.log']
    test_summaries={}
    for name in logs:
        lines=read(name).decode().splitlines()
        summaries=[line for line in lines if line.startswith('test result:')]
        assert summaries and all('test result: ok.' in line and '; 0 failed;' in line for line in summaries), name
        test_summaries[name]=summaries
    semantic=read('semantic-fault-matrix.log').decode()
    db_log=read('db-rebuild-faults.log').decode()
    required_semantic=['killed_mutation_owner_releases_cross_process_lock','publisher_keeps_namespace_lease_between_verified_bytes_and_manifest_cas','namespace_lock_spans_mark_and_unlink_before_concurrent_publish','independently_observed_sigkill_boundaries','isolated_sigkill_reopen_replay_preparation','delayed_corrupt_report_never_quarantines_a_republished_healthy_object','already_absent_halves_are_idempotent_and_never_count_as_unlinks','publish_committing_between_collect_and_sweep_is_protected_by_the_snapshot','dead_letters_are_counted_but_never_resurrected','call_layer_exhaustion_hands_back_and_the_db_budget_dead_letters','rebuild_reconcile_reuses_paid_vectors_with_zero_provider_calls','expired_reclaimed_lease_cannot_publish_but_successor_can']
    required_db=['live_checkpoint_busy_preserves_committed_wal_and_refuses_replacement','staging_checkpoint_busy_preserves_wal_and_refuses_replacement','owned_process_kill_between_sidecar_cleanup_rename_and_reopen_preserves_database','writer_mutex_spans_rename_reopen_and_connection_installation','failed_writer_reopen_leaves_no_writable_ghost_or_stale_read_pool','failed_read_pool_reopen_leaves_no_partially_installed_writer_or_stale_reads']
    for name in required_semantic:
        assert re.search(r'test [^\n]*\b'+name+r' \.\.\. ok',semantic), name
    for name in required_db:
        assert re.search(r'test [^\n]*\b'+name+r' \.\.\. ok',db_log), name
    wal_rows=[]
    for line in db_log.splitlines():
        if '{' not in line: continue
        try: row=json.loads(line[line.index('{'):])
        except json.JSONDecodeError: continue
        if isinstance(row,dict) and row.get('seed') in [163,167,173] and row.get('point') in ['sidecars-removed','renamed']:
            assert row['integrity']=='ok' and row['foreign_key_errors']==0 and row['recovered'] is True
            wal_rows.append(row)
    assert len(wal_rows)==6
    assert {(r['seed'],r['point']) for r in wal_rows}=={(s,p) for s in [163,167,173] for p in ['sidecars-removed','renamed']}
    required_raw=[n for n in members if n!='http-codecortex' and not '/project/.codecortex/' in n and '/semantic-cache/' not in n and not n.startswith('worker-contention/')]
    report={'schema_version':1,'review_kind':'actual_fixed_CI_artifact_replay','reviewer':'/root/todo_acceptance','reviewer_note':'This agent authored the L3 driver and GC/WAL fixes; non-author source reviews remain separately bound in the root evidence. This replay independently verifies archived execution data and does not self-approve task-ledger changes.','source_commit':COMMIT,'source_tree':before['source_tree'],'source_manifest_sha256':before['manifest_sha256'],'verified_source_inputs':795,'workflow_run_id':37729686665,'job_id':113155755227,'artifact_id':11529028468,'artifact_sha256':ZIP_SHA,'artifact_bytes':ARCHIVE.stat().st_size,'product_sha256':receipt['binary_sha256'],'product_bytes':next(x['bytes'] for x in member_receipts if x['member']=='http-codecortex'),'actual_profile':'dev; opt_level=0, debug_assertions=true, test=false','original_driver_sha256':DRIVER_SHA,'l3_driver_sha256':digest(driver),'original_http_13':{'passed':True,'checkpoints':expected_cases,'mcp_rpc_count':args.expected_original_rpcs,'normal_child_exit_count':10,'observed_http_posts':8,'costs':original['costs'],'hot_reload':'not_run','SIGKILL_matrix':'original driver not_run; separately verified below'},'l3':{'passed':True,'event_count':len(events),'mcp_request_response_pairs':args.expected_mcp_pairs,'seed_results':seeds,'all_actual_exits_precede_real_lease_expiry':True,'actual_sigkill_count':3,'normal_exit_count':9,'observed_document_posts':15,'observed_query_posts':0,'zero_post_warm_restarts':6,'paid_cost':None,'crash_response_consumption_and_billing':'unknown; preserved','manual_database_or_clock_changes':False},'internal_production_faults':{'required_semantic_tests':required_semantic,'required_db_tests':required_db,'wal_six_actual_recovery_observations':wal_rows,'old_internal_crash_raw_scope':'Fixed current test source plus actual successful target logs retained. Per-case old crash results.json/child.log were written to runner /tmp and are not claimed to be in this ZIP.'},'test_result_summaries':test_summaries,'acceptance_verdict':'PASS for original13 plus declared representative P7-016 engineering recovery execution; task closure remains subject to root independent review and P7-014/P7-015 dependency acceptance.','live_semantic_quality':'not_run; fake engineering only','task_ledger_changed':False,'audit_scope_only':True,'expected_observation_counts':{'members':args.expected_members,'l3_events':args.expected_events,'l3_mcp_pairs':args.expected_mcp_pairs,'original_mcp_rpcs':args.expected_original_rpcs},'required_raw_members':required_raw,'all_members':member_receipts,'review_script_sha256':digest(Path(__file__).read_bytes())}
    output=args.output
    with output.open('x') as target:
        json.dump(report,target,ensure_ascii=False,indent=2);target.write('\n')
    print(json.dumps({'passed':True,'receipt':str(output),'receipt_sha256':digest(output.read_bytes()),'seed_results':seeds},ensure_ascii=False,indent=2))
