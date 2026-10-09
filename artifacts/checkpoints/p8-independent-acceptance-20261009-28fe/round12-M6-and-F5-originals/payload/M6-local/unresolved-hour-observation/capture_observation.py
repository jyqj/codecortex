#!/usr/bin/env python3
"""Capture original text/prefix only; never open an SQLite database or signal a process."""
import datetime, gzip, hashlib, json, os, stat
from pathlib import Path

OUT = Path(__file__).resolve().parent
LIVE = Path('/tmp/codecortex-p8-m6-28fe-20261009')
def now(): return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(data): return hashlib.sha256(data).hexdigest()
def snap(path):
    if not path.exists(): return {'exists': False}
    s = path.lstat()
    return {'exists': True, 'bytes': s.st_size, 'mtime_ns': s.st_mtime_ns,
            'inode': s.st_ino, 'mode': oct(stat.S_IMODE(s.st_mode))}
started = now()
records = []
for relative in ['runtime-parent-recovery/receipt.json',
                 'runtime-parent-recovery/original-execution-observation.json',
                 'runtime-parent-recovery/soak-run.log', 'soak/runtime/plan.json']:
    source = LIVE / relative
    before = snap(source)
    data = source.read_bytes()
    target = OUT / 'original-text' / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    records.append({'source': str(source), 'copy': str(target), 'before': before,
                    'after': snap(source), 'bytes': len(data), 'sha256': sha(data)})
source = LIVE / 'soak/runtime/raw.jsonl'
before = snap(source)
target = OUT / 'raw-observed-prefix.jsonl.gz'
digest = hashlib.sha256()
read = 0
last = b''
with source.open('rb') as input_file, target.open('wb') as output_file:
    with gzip.GzipFile(filename='', mode='wb', fileobj=output_file, mtime=0, compresslevel=6) as compressed:
        while read < before['bytes']:
            block = input_file.read(min(1024 * 1024, before['bytes'] - read))
            if not block: raise RuntimeError('original prefix shortened while captured')
            compressed.write(block); digest.update(block); read += len(block); last = block[-1:]
after = snap(source)
records.append({'source': str(source), 'copy': str(target), 'encoding': 'gzip exact captured bytes',
                'before': before, 'after': after, 'bytes': read, 'sha256': digest.hexdigest(),
                'compressed_bytes': target.stat().st_size,
                'compressed_sha256': sha(target.read_bytes()), 'ends_with_newline': last == b'\n'})
state = {name: snap(LIVE / 'soak/runtime' / name) for name in ['plan.json', 'raw.jsonl', 'report.json', 'seal.json']}
receipt = json.loads((OUT / 'original-text/runtime-parent-recovery/receipt.json').read_text())
result = {'schema_version': 1, 'kind': 'read_only_unknown_execution_session_observation',
          'source_commit': 'e95fd750d2f5052d0308c970cd5032c48b765cde',
          'capture_started_at': started, 'capture_finished_at': now(),
          'reported_root_observation': {'at': '2026-10-09T00:48:49Z', 'session_id': 26554,
            'poll_result': 'Unknown process id', 'last_successful_poll': '2026-10-09T00:28:54Z',
            'process_inspection_result': 'fatal library error, lookup self',
            'attribution': 'parent message; not an independently successful process-table probe'},
          'captured_receipt_status': receipt.get('status'), 'records': records, 'soak_output_state': state,
          'stable_during_each_capture': all(r['before'] == r['after'] for r in records),
          'prior_resource_gap_report': '/workspace/scratch/28fef0db5e01/runtime-review/round11-m6-soak-prefix/resource-gap-prefix-review.json',
          'scope': 'Only text files and an exact bounded raw prefix captured. No database opened. No process execution, signal, cancellation, seal or receipt mutation.',
          'verdict': 'No terminal runtime exit, original verify, report or seal is available for this hour. The running receipt remains unchanged. Missing session does not prove native termination or its cause. Earlier resource coverage gaps remain valid prefix evidence, not an observed final CLI exit.',
          'todo': {'new_original_completed': 0, 'remaining': 29}}
(OUT / 'interruption-observation.json').write_text(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2) + '\n')
print(json.dumps({'report': str(OUT / 'interruption-observation.json'), 'stable': result['stable_during_each_capture'],
                  'raw_bytes': read, 'raw_sha256': digest.hexdigest(), 'compressed_bytes': target.stat().st_size,
                  'report_sha256': sha((OUT / 'interruption-observation.json').read_bytes())}, sort_keys=True))
