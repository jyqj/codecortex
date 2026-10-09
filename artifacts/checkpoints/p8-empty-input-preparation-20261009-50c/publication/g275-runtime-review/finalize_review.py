#!/usr/bin/env python3
"""Recheck original seals/ZIPs, then package a small, URL-sanitized review."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
REPO = Path('/workspace/scratch/50c364fd60b1/codecortex')
SOURCE = '275e8799d4947d297329073eaa3ca675d3fd0777'
RUN = 37890757030
PUBLIC = ROOT / 'publication'
FINAL = ROOT / 'final-seal-checks'


def read(path): return json.loads(path.read_text())


def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def write(path, value):
    with path.open('x') as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, indent=2)
        stream.write('\n')


def git(path): return subprocess.check_output(['git','show',SOURCE + ':' + path], cwd=REPO)


if __name__ == '__main__':
    reuse_final = sys.argv[1:] == ['--reuse-final-seal-checks']
    assert not sys.argv[1:] or reuse_final, 'unknown arguments'
    FINAL.mkdir(exist_ok=reuse_final)
    downloads = {r['key']:r for r in read(ROOT / 'download-integrity-review.json')}
    metadata = {r['key']:r['artifact'] for r in read(ROOT / 'download-metadata.json')}
    joblogs = {r['key']:r for r in read(ROOT / 'job-log-integrity.json')}
    integrity = read(ROOT / 'integrity-and-replays.json')
    semantic = read(ROOT / 'raw-observations.json')
    assert semantic['status'] == 'passed'
    artifacts, receipts = [], []
    for key in ['c1','c4','c8','c16','soak','backfill']:
        base = ROOT / key / 'extracted'
        build = base if key == 'backfill' else base / 'p8-build'
        script = build / 'observer-source/scripts' / ('p8_backfill.py' if key == 'backfill' else 'p8_runtime.py')
        command = [sys.executable,'-B',str(script),'verify','--output',str(base if key == 'backfill' else base / 'p8-runtime')]
        if key != 'backfill': command += ['--build-output',str(build)]
        if reuse_final:
            receipt = read(FINAL / (key + '.json'))
            assert receipt['argv'] == command and receipt['original_verifier_sha256'] == sha(script)
            for channel in ['stdout','stderr']:
                assert hashlib.sha256(receipt[channel].encode()).hexdigest() == receipt[channel + '_sha256']
        else:
            start = datetime.datetime.now(datetime.timezone.utc).isoformat()
            result = subprocess.run(command, capture_output=True, timeout=60,
                                    env={**os.environ,'PYTHONDONTWRITEBYTECODE':'1'})
            receipt = dict(key=key, stage='final-original-seal-check', argv=command, started_at=start,
                           finished_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), exit_code=result.returncode,
                           stdout=result.stdout.decode(), stderr=result.stderr.decode(),
                           stdout_sha256=hashlib.sha256(result.stdout).hexdigest(),
                           stderr_sha256=hashlib.sha256(result.stderr).hexdigest(),
                           original_verifier_sha256=sha(script))
            write(FINAL / (key + '.json'), receipt)
        assert receipt['exit_code'] == 0, 'final original seal check failed: ' + key
        receipts.append(receipt)
        artifact = metadata[key]; downloaded = downloads[key]; zip_path = ROOT / key / 'original.zip'
        assert artifact['workflow_run']['id'] == RUN and artifact['workflow_run']['head_sha'] == SOURCE
        assert zip_path.stat().st_size == artifact['size_in_bytes'] == downloaded['bytes']
        assert sha(zip_path) == artifact['digest'].removeprefix('sha256:') == downloaded['sha256']
        log = joblogs[key]; log_path = Path(log['path'])
        assert sha(log_path) == log['sha256'] and log_path.stat().st_size == log['saved_bytes']
        text = log_path.read_text()
        digest_lines = re.findall(r'^.*SHA256 digest of uploaded artifact zip is ([0-9a-f]{64}).*$', text, re.M)
        upload_lines = [line for line in text.splitlines() if 'has been successfully uploaded! Final size is' in line]
        assert digest_lines == [downloaded['sha256']]
        assert len(upload_lines) == 1 and f"Final size is {downloaded['bytes']} bytes. Artifact ID is {artifact['id']}" in upload_lines[0]
        artifacts.append(dict(key=key, artifact_id=artifact['id'], name=artifact['name'],
                              bytes=downloaded['bytes'], sha256=downloaded['sha256'],
                              artifact_url=f"https://github.com/jyqj/codecortex/actions/runs/{RUN}/artifacts/{artifact['id']}",
                              artifact_api_url=f"https://api.github.com/repos/jyqj/codecortex/actions/artifacts/{artifact['id']}",
                              run_id=RUN, run_url=f'https://github.com/jyqj/codecortex/actions/runs/{RUN}',
                              job_id=log['job_id'], job_url=f"https://github.com/jyqj/codecortex/actions/runs/{RUN}/job/{log['job_id']}",
                              source_commit=SOURCE, created_at=artifact['created_at'], expires_at=artifact['expires_at'],
                              actual_curl_exit_code=downloaded['curl_exit_code'], verified_at=downloaded['verified_at'],
                              zip_members=downloaded['members'], uncompressed_bytes=downloaded['uncompressed_bytes'],
                              original_api_digest_matches=True, original_upload_log_digest_size_id_matches=True,
                              final_zip_unchanged=True, original_job_log_sha256=log['sha256'],
                              original_upload_summary_line=upload_lines[0]))
        print(json.dumps(dict(key=key, original_seal='passed', zip_unchanged=True)), flush=True)
    for result in integrity['results']:
        key = result['key']; directory = ROOT / key / 'offline-review'
        for path in sorted(directory.glob('*.receipt.json')):
            value = read(path); stem = path.name.removesuffix('.receipt.json')
            assert value['exit_code'] == 0
            assert sha(directory / (stem + '.stdout')) == value['stdout_sha256']
            assert sha(directory / (stem + '.stderr')) == value['stderr_sha256']
            receipts.append(dict(key=key, stage=stem, **value,
                                stdout=(directory / (stem + '.stdout')).read_text(),
                                stderr=(directory / (stem + '.stderr')).read_text(),
                                original_receipt_sha256=sha(path)))
    PUBLIC.mkdir(exist_ok=reuse_final)
    source = integrity['source']
    source_summary = {k:v for k,v in source.items() if k != 'inputs'}
    task_data = json.loads(git('docs/roadmap/code-index-v2/tasks.json'))
    task_records = [{k:task[k] for k in ['id','title','status','depends_on','acceptance','validations','contracts_doc','benchmark_doc']}
                    for task in task_data['tasks'] if task['id'] in ['P8-007','P8-010']]
    sample_receipt = read(ROOT / 'c1/extracted/p8-build/build-receipt.json')
    selected = ['.github/workflows/p8-runtime.yml','docs/roadmap/code-index-v2/tasks.json',
                'crates/cc-eval/src/bin/p8-runtime-statistics.rs','crates/cc-eval/src/bin/p8-oracle.rs',
                'crates/cc-eval/src/benchmark/statistics.rs','crates/cc-eval/src/benchmark/oracle.rs',
                'crates/cc-eval/src/benchmark/oracle/streaming.rs','crates/cc-eval/tests/p7_worker_contention.rs']
    selected += sorted(sample_receipt['observer_before']['files'])
    source_files = []
    for path in selected:
        data = git(path)
        source_files.append(dict(path=path, bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
                                 git_blob=hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest(),
                                 url=f'https://github.com/jyqj/codecortex/blob/{SOURCE}/{path}'))
    source_summary.update(source_url=f'https://github.com/jyqj/codecortex/tree/{SOURCE}',
                          all_six_build_input_manifests_match_fixed_git=True,
                          build_input_scope='all 1,089 recorded Cargo/crates inputs; observer files separately verified against fixed Git blobs',
                          observer_manifest=sample_receipt['observer_before'],
                          selected_original_protocol_and_verifier_files=source_files,
                          original_task_records=task_records,
                          original_toolchain=sample_receipt['toolchain_before'],
                          retained_runtime_binaries=integrity['results'][0]['binaries'],
                          backfill_binary=next(r for r in integrity['results'] if r['key']=='backfill')['binaries'],
                          integrity_review_script_sha256=sha(ROOT / 'review_integrity_and_replays.py'),
                          original_complete_integrity_result_sha256=sha(ROOT / 'integrity-and-replays.json'),
                          no_current_machine_original_compiler_claim=True)
    write(PUBLIC / 'source-manifest.json', source_summary)
    write(PUBLIC / 'artifact-manifest.json', dict(schema_version=1, artifacts=artifacts,
          transport_scope='official GitHub artifact capability; unchanged signed URL read with standard curl; no custom authentication/headers/host',
          secrets_or_temporary_download_urls_retained=False,
          prior_transfer_attempt='urllib and official materialization helper returned HTTP 403; standard curl then succeeded on every original ZIP; prior raw failure receipts remain outside this package'))
    raw = dict(semantic)
    raw.update(original_raw_review_report_sha256=sha(ROOT / 'raw-observations.json'),
               original_runtime_run_id=RUN, original_runtime_run_url=f'https://github.com/jyqj/codecortex/actions/runs/{RUN}',
               artifact_manifest='artifact-manifest.json', source_manifest='source-manifest.json',
               offline_verifier_receipts='replay-receipts.json',
               original_scope_acceptance={
                  'P8-007': {'independent_raw_subacceptance':'passed', 'covered':['mixed C1/C4/C8/C16 original 900 offered per cell',
                      'actual held fake-backfill with seeds 7/19/43 and original per-cell N32',
                      'original descriptive all-attempt latency/quantile owner replay'],
                      'whole_task_status':'in_progress', 'unsatisfied_dependency':'P8-006, transitively P8-005'},
                  'P8-010': {'independent_raw_subacceptance':'passed', 'covered':['original one-hour workload and all 3601 offered operations',
                      'actual branch/catalog/cache/resource coverage', 'complete unrepaired fifteen-table endpoint parity'],
                      'whole_task_status':'in_progress', 'unsatisfied_dependency':'P8-009/P8-008/P8-007/P8-006/P8-005'}},
               cross_source_certification=False,
               no_certification_of_pr181_or_later_candidate=True,
               final_original_seals_and_zip_bytes_unchanged=True,
               review_limits=[
                   'No full TODO closure or release certificate: P8-005 scale acceptance and downstream dependency chain remain open.',
                   'Original default runtime disables semantic provider; backfill is a separately identified in-process fake-provider control. No live-provider gate is added.',
                   'Configured C8/C16 observed maximum active client calls 7/12; no claim of 8/16 simultaneously active calls, backend thread identity, separated queue/service timing or certified stable tails.',
                   'Soak C4 retains original shared write-admission lock and observed client maximum 1; its compound-read timing is not directly comparable to symbol-only mixed reads.',
                   'Backfill held-after provider/queue invariance and stale-symbol checks are executed source assertions backed by original successful selected-test output; no separately retained final database is invented.',
                   'No study/workload/product build was rerun. Original stats and read-only oracle binaries were copied and replayed only against retained originals or database copies.'
               ])
    write(PUBLIC / 'raw-review.json', raw)
    raw_receipt = read(ROOT / 'raw-review-command-receipt.json')
    assert raw_receipt['completion_tool_result']['exit_code'] == 0
    write(PUBLIC / 'replay-receipts.json', dict(schema_version=1, source_commit=SOURCE,
          original_readonly_verifier_commands=receipts,
          counts=dict(original_seal_checks=12,original_statistics_replays=5,original_fifteen_table_oracle_replays=5),
          raw_semantic_review_command=raw_receipt,
          statistics_result='each original and original second replay byte-identical to new retained-binary offline replay',
          oracle_result='five original fifteen-table comparisons equal; replay matches every field except absolute left/right paths and elapsed_ns; input database bytes unchanged',
          source_and_binary_identity='all retained binaries match fixed Cargo receipts, copy_source size/SHA and original build messages; current machine is not relabeled as original compiler'))
    for name in ['review_integrity_and_replays.py','review_raw_observations.py','finalize_review.py']:
        shutil.copyfile(ROOT / name, PUBLIC / name)
    print(json.dumps(dict(publication=str(PUBLIC), json_files=4, scripts=3)), flush=True)
