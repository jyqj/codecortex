#!/usr/bin/env python3
"""Capture one invocation of unchanged b107; never run a workload or retry."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib, json, os, shutil, subprocess, sys, time, zipfile

BASE = Path(__file__).resolve().parent
SOURCE = Path('/workspace/scratch/a217aaae3bde/scale-original-c8be')
HEAD = 'c8be5afaac568ffd40ef86d3795423c3b73c9f39'
RUN = 37902429727
EXPECTED_HELPER = 'b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee'
ORIGINAL_HELPER = Path('/workspace/scratch/a217aaae3bde/scale-streaming-helper-original.py')
HELPER = BASE / 'review_scale_fixed_source.py'
STATE = BASE / 'validated'

def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        for data in iter(lambda: stream.read(1024 * 1024), b''):
            h.update(data)
    return h.hexdigest()

def info(path):
    return {'path': str(path), 'size': path.stat().st_size, 'sha256': digest(path)}

def new_json(path, value):
    with path.open('x', encoding='utf-8') as output:
        json.dump(value, output, indent=2, sort_keys=True, ensure_ascii=False, allow_nan=False)
        output.write('\n')

def now():
    return datetime.now(timezone.utc).isoformat()

operation, artifact = sys.argv[1:]
assert operation in ('init', 'shard') and artifact.isdecimal()
assert digest(ORIGINAL_HELPER) == EXPECTED_HELPER
if not HELPER.exists():
    with HELPER.open('xb') as output:
        output.write(ORIGINAL_HELPER.read_bytes())
assert digest(HELPER) == EXPECTED_HELPER
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=SOURCE, text=True).strip() == HEAD
archive = BASE / 'artifacts' / artifact / (artifact + '.zip')
metadata_path = archive.parent / 'artifact-metadata.json'
metadata = json.loads(metadata_path.read_bytes())
assert metadata['id'] == int(artifact)
assert metadata['workflow_run']['id'] == RUN and metadata['workflow_run']['head_sha'] == HEAD
assert not archive.is_symlink() and archive.stat().st_size == metadata['size_in_bytes']
assert 'sha256:' + digest(archive) == metadata['digest']
with zipfile.ZipFile(archive) as zipped:
    size = sum(row.file_size for row in zipped.infolist() if not row.is_dir())
    members = len(zipped.infolist())
free = shutil.disk_usage(BASE).free
# The unmodified helper separately enforces all path, member and byte predicates.
assert size <= 600 * 1024 * 1024 and free >= size + 128 * 1024 * 1024
if operation == 'init':
    assert artifact == '11603522248' and not STATE.exists()
else:
    assert artifact in ('11603378825', '11603634172', '11603917265')
    assert not (STATE / 'shards' / artifact).exists()
executions = BASE / 'executions'
executions.mkdir(exist_ok=True)
name = operation + '-' + artifact
stdout, stderr = executions / (name + '.stdout'), executions / (name + '.stderr')
receipt = executions / (name + '.json')
assert not stdout.exists() and not stderr.exists() and not receipt.exists()
command = [sys.executable, '-B', str(HELPER), operation]
if operation == 'init':
    command += ['--root', str(SOURCE), '--source', HEAD, '--run-id', str(RUN)]
command += ['--state', str(STATE), '--archive', str(archive), '--metadata', str(metadata_path)]
before = {'zip': info(archive), 'metadata': info(metadata_path), 'helper': info(HELPER), 'wrapper': info(Path(__file__))}
start, clock = now(), time.monotonic()
with stdout.open('xb') as out, stderr.open('xb') as err:
    result = subprocess.run(command, stdout=out, stderr=err, cwd=BASE, env={**os.environ, 'PYTHONDONTWRITEBYTECODE': '1'})
record = {'schema': 'p8-original-helper-single-invocation-v1', 'source': HEAD, 'run_id': RUN,
          'operation': operation, 'artifact_id': int(artifact), 'command': command, 'cwd': str(BASE),
          'started_at': start, 'completed_at': now(), 'elapsed_seconds': time.monotonic()-clock,
          'returncode': result.returncode, 'before': before,
          'after': {'zip': info(archive), 'metadata': info(metadata_path), 'helper': info(HELPER), 'wrapper': info(Path(__file__))},
          'stdout': info(stdout), 'stderr': info(stderr),
          'zip_inventory_preflight': {'members': members, 'expanded_bytes': size, 'available_bytes': free, 'reserve_bytes': 128*1024*1024},
          'workload_started': False, 'compile_started': False, 'combine_called': False,
          'task_statuses_changed': False, 'formal_completion': False}
review = STATE / 'initialization-review.json' if operation == 'init' else STATE / 'shards' / artifact / 'review.json'
if review.is_file():
    record['original_review'] = info(review)
    record['original_status'] = json.loads(review.read_bytes()).get('status')
new_json(receipt, record)
print(json.dumps({'receipt': str(receipt), 'returncode': result.returncode, 'status': record.get('original_status'), 'elapsed_seconds': record['elapsed_seconds']}))
assert record['before'] == record['after'], 'immutable original inputs changed'
raise SystemExit(result.returncode)
