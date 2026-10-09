#!/usr/bin/env python3
"""Preserve existing scale review bytes only; never execute validation or native work."""
from pathlib import Path
import datetime
import gzip
import hashlib
import io
import json
import re
import stat
import subprocess
import tarfile

BASE = Path(__file__).resolve().parent
WORK = Path('/workspace/scratch/a217aaae3bde/scale-combined-E-review')
RAM = Path('/dev/shm/a217aaae3bde/scale-combined-E-review')
ROOT = Path('/dev/shm/a217aaae3bde/codecortex-combined-guard-view')
OLD = Path('/dev/shm/a217aaae3bde/scale-round5-review')
E = 'a23bb72d3c954f385b99fe81ce9189885c208557'
CUTOFF = '2026-10-09T04:49:39.966Z'
CUTOFF_MS = 1791521379966
entries = {}
source_records = []

def sha(data):
    return hashlib.sha256(data).hexdigest()

def git_blob(data):
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()

def add(member, source):
    source = Path(source)
    assert source.is_file() and not source.is_symlink(), source
    assert member not in entries, member
    assert not member.startswith('/') and '..' not in Path(member).parts
    data = source.read_bytes()
    assert not data.startswith(b'\x7fELF') and source.suffix != '.zip', source
    assert not re.search(rb'file_00000000[0-9a-f]+', data), source
    assert not re.search(rb'library://[A-Za-z0-9_/-]+', data), source
    assert not re.search(rb'https?://[^\s"<>]*(?:[?&](?:sig|X-Amz-Signature|X-Amz-Credential)=)', data, re.I), source
    entries[member] = (source, data, stat.S_IMODE(source.stat().st_mode))

add('README.md', BASE / 'README.md')
add('package-maker.py', Path(__file__))
for name in ['review_scale_fixed_source.py','execution-pin.json','prebuild-exact-E-source-review.json','original-run-binding.json','prebuild-exact-E-runtime-observer-snapshot.json','prebuild-exact-E-source-snapshot.json','prebuild-exact-E-driver-snapshot.json','pending-state-000.json','pending-state-001.json','pending-state-002.json','new-scale-run-discovery.json','monitor-once.js']:
    add('E-prebuild-and-pin/' + name, WORK / name)
for name in ['storage-location-addendum.json','build-initialization-execution.json','build-initialization.stdout','build-initialization.stderr','admitted-shards-002.json','admitted-shards-003.json','admitted-shards-004.json','initial-five-capacity-observations.json','exec-recovery-snapshot-preservation.json','E-100k-in-progress-log-attempt.json','monitor-once-ram-output.js','monitor-once-ram-memory-fallback.js','monitor-aux8e-once.js']:
    add('E-review/' + name, RAM / name)
for aid in [11591367982,11591464502,11591693031,11593548201]:
    name = f'shard-{aid}-execution.json'
    add('E-review/' + name, RAM / name)
    for suffix in ['stdout','stderr']:
        p = RAM / f'shard-{aid}-execution.{suffix}'
        if p.exists():
            add('E-review/' + p.name, p)
    for name in ['review.json','validated-shard.json']:
        add(f'E-state-context/shards/{aid}/{name}', RAM / 'validated/shards' / str(aid) / name)
for name in ['initialization-review.json','state.json','expected-engine.json','source-snapshot.json','engine-source-entries.json']:
    add('E-state-context/' + name, RAM / 'validated' / name)
for name in ['source-before.json','source-after.json','cargo.stderr','cargo.jsonl','build.json']:
    add('E-state-context/build-small/' + name, RAM / 'validated/build-validation' / name)
for original_base, label in [(WORK,'initial-workspace-observations'),(RAM,'later-ram-observations')]:
    for dirname in ['official-collection-snapshots','aux8e-observations']:
        for p in sorted((original_base / dirname).glob('*.json')):
            stamp = int(p.name.split('-')[0])
            if stamp <= CUTOFF_MS:
                add(f'{label}/{dirname}/{p.name}',p)
for aid in [11591482043,11591367982,11591464502,11591693031,11593548201,11592230167,11591807078,11591237661,11591232618,11591128037]:
    p = Path('/workspace/scratch/a217aaae3bde/ci-artifacts') / str(aid)
    for name in ['artifact-metadata.json','original-zip-local-verification.json']:
        add(f'official-artifact-metadata/{aid}/{name}', p / name)
for name in ['streaming-helper-root-review.json','streaming-review-controls.json','check_streaming_review_controls.py','partial-matrix-control.json','controls-partial-matrix/independent-review.json','combined254-scale-protocol-independent-review-v2.json']:
    add('historical-helper-controls/' + name, OLD / name)
add('helper-pin-independent-review.json',Path('/dev/shm/a217aaae3bde/runtime-review/combined-candidate-plan-v1/scale-helper-pin-independent-review.json'))
protocol = ['.github/workflows/p8-scale.yml','scripts/p7_build_identity.py','scripts/p8_scale_matrix.py','scripts/p8_runner_capacity.py','scripts/tests/test_p8_scale_matrix.py','scripts/tests/test_p8_runner_capacity.py','crates/cc-eval/src/benchmark/p8_scale.rs','crates/cc-eval/src/benchmark/oracle/streaming.rs','docs/roadmap/code-index-v2/P8-SCALE.md','docs/roadmap/code-index-v2/tasks.json']
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip() == E
for relative in protocol:
    p = ROOT / relative
    git_data = subprocess.check_output(['git','show',E + ':' + relative],cwd=ROOT)
    input_origin = 'existing_readonly_projection_file'
    if p.exists():
        data = p.read_bytes()
        assert git_data == data
    else:
        # Documentation omitted by the frozen projection is copied from the
        # exact Git object into this new package directory, never into ROOT.
        p = BASE / 'fixed-source-protocol-inputs' / relative
        p.parent.mkdir(parents=True,exist_ok=True)
        with p.open('xb') as handle:
            handle.write(git_data)
        git_mode = subprocess.check_output(['git','ls-tree',E,'--',relative],cwd=ROOT,text=True).split()[0]
        p.chmod(int(git_mode,8) & 0o777)
        data = p.read_bytes()
        assert data == git_data
        input_origin = 'exact_E_Git_object_copied_into_package_only'
    add('fixed-E-protocol/' + relative,p)
    source_records.append({'repository_path':relative,'sha256':sha(data),'git_blob_sha1':git_blob(data),'origin':input_origin})
assert sha((WORK / 'review_scale_fixed_source.py').read_bytes()) == 'b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee'
assert sha((RAM / 'validated/state.json').read_bytes()) == '5f7f983bba55dd474296e03c35770c8caa86ed0d988c560ce005c3e26c8fcd7a'
assert not (RAM / 'admitted-shards-001.json').exists()
admitted = json.loads((RAM / 'admitted-shards-004.json').read_text())
assert (admitted['accepted_shards'],admitted['accepted_samples']) == (4,41)
zero_observations = [m for m,(_,data,_) in entries.items() if 'observations/' in m and len(data)==0]

def serialize():
    output = io.BytesIO()
    with gzip.GzipFile(fileobj=output,filename='',mode='wb',compresslevel=9,mtime=0) as gz:
        with tarfile.open(fileobj=gz,mode='w',format=tarfile.USTAR_FORMAT) as tar:
            for name,(_,data,mode) in sorted(entries.items()):
                item = tarfile.TarInfo(name)
                item.size=len(data); item.mode=mode
                item.uid=item.gid=item.mtime=0; item.uname=item.gname=''
                tar.addfile(item,io.BytesIO(data))
    return output.getvalue()

archive_data = serialize()
assert serialize() == archive_data
archive = BASE / 'scale-four-shard-audit-v1.tar.gz'
with archive.open('xb') as handle:
    handle.write(archive_data)
with tarfile.open(archive,'r:gz') as tar:
    assert tar.getnames() == sorted(entries)
    for item in tar:
        source,data,mode = entries[item.name]
        assert item.isfile() and item.mode==mode and item.uid==item.gid==item.mtime==0
        assert item.uname==item.gname=='' and not item.pax_headers
        assert tar.extractfile(item).read() == data == source.read_bytes()
        assert stat.S_IMODE(source.stat().st_mode) == mode
members=[]
for name,(source,data,mode) in sorted(entries.items()):
    members.append({'member':name,'source_path':str(source),'source_mode':mode,'archive_mode':mode,'bytes':len(data),'sha256':sha(data),'git_blob_sha1':git_blob(data)})
manifest={'schema':'fixed-E-four-scale-shard-audit-custody-v1','cutoff_utc':CUTOFF,'source_commit':E,'source_tree':'58147c952505c44da1f41eb4b9c31643f2303b96','source_manifest_sha256':'4e0aa6bbfd4d5bea00416bca2cd99318d865ffc526fdcb45897c4d300de3ac00','run_id':37872522779,'run_attempt':1,'helper_sha256':'b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee','state_sha256':'5f7f983bba55dd474296e03c35770c8caa86ed0d988c560ce005c3e26c8fcd7a','accepted_shards':4,'accepted_samples':41,'required_shards':150,'required_samples':1500,'registered_repetitions':30,'minimum_extract_reserve_bytes':128*1024*1024,'source_records':source_records,'archive':{'path':str(archive),'bytes':len(archive_data),'sha256':sha(archive_data),'git_blob_sha1':git_blob(archive_data)},'member_count':len(members),'original_bytes':sum(x['bytes'] for x in members),'format':'sorted regular USTAR; exact source mode/bytes; uid/gid/mtime0,empty uname/gname; gzip empty filename,mtime0,level9','members':members,'independent_second_serialization_identical':True,'all_members_reopened_equal_source_bytes_and_modes':True,'actual_private_file_identifiers_signed_urls_or_library_uris_included':False,'zero_byte_failed_write_placeholders_not_valid_observations':zero_observations,'admitted_001':'never_created; individual first and second command receipts exist and first cumulative index is002','historical_helper_controls_credit_to_E_samples':0,'aux8e_credit_to_E_samples':0,'excluded_categories':['native originalZIPs (separately preserved)','28MB build ELF','derived workload extraction','all private-transfer responses and file handles','unrelated PR/runtime/gates audits','snapshots after this fixed cutoff'],'source_view_scope':'Selected protocol bytes and full original product manifest, not a full checkout','native_workload_or_validator_replay_executed_by_packager':False,'original_files_or_state_modified':False,'formal_task_completion':False,'task_counts':{'done':163,'remaining':29},'publication_status':'prepared_not_published'}
with (BASE / 'payload-manifest.json').open('x') as handle:
    json.dump(manifest,handle,ensure_ascii=False,indent=2);handle.write('\n')
print(json.dumps({'archive':manifest['archive'],'manifest_sha256':sha((BASE/'payload-manifest.json').read_bytes()),'member_count':len(members),'original_bytes':manifest['original_bytes'],'zero_byte_observation_placeholders':len(zero_observations)}))
