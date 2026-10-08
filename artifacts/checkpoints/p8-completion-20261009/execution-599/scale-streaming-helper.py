#!/usr/bin/env python3
"""Replay archived P8 shards with the unmodified, explicitly fixed Git source.

This helper never runs a scale workload, downloads an artifact, edits a checkout,
deletes an original ZIP, or changes task status. Only the preserved executable's
--hash-file operation is used. Per-shard extraction is discarded after replay;
the original ZIP and its authoritative GitHub metadata must be retained.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import sys
import zipfile

sys.dont_write_bytecode = True
SCHEMA = 'p8-fixed-source-streaming-review-v1'
MAX_EXTRACTED = 600 * 1024 * 1024


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha256(path):
    value = hashlib.sha256()
    with Path(path).open('rb') as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b''):
            value.update(block)
    return value.hexdigest()


def utc():
    return datetime.now(timezone.utc).isoformat()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, 'duplicate JSON key: ' + key)
        result[key] = value
    return result


def read_json(path):
    path = Path(path)
    require(path.is_file() and not path.is_symlink() and path.stat().st_size <= 8 * 1024 * 1024,
            'missing, nonregular, or oversized review JSON: ' + str(path))
    return json.loads(path.read_bytes(), object_pairs_hook=unique_object,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError(value)))


def write_new(path, value):
    path = Path(path)
    with path.open('x', encoding='utf-8') as handle:
        json.dump(value, handle, ensure_ascii=False, indent=2, sort_keys=True, allow_nan=False)
        handle.write('\n')


def fixed_driver(root, source):
    root = Path(root).resolve(strict=True)
    require(re.fullmatch(r'[0-9a-f]{40}', source), 'full immutable source SHA required')
    actual = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    require(actual == source, 'frozen checkout HEAD differs from explicit source')
    sys.path.insert(0, str(root / 'scripts'))
    spec = importlib.util.spec_from_file_location('p8_scale_original_fixed', root / 'scripts/p8_scale_matrix.py')
    matrix = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(matrix)
    require(hasattr(matrix, 'verify_build_origin'), 'fixed source lacks strict producer/copy binding')
    driver = matrix.driver_snapshot(root)
    snapshot = matrix.source_snapshot(root)
    require(driver['source_commit'] == snapshot['source_commit'] == source, 'source/driver differs')
    return root, matrix, snapshot, driver


def check_metadata(metadata, run_id, source, name=None):
    require(type(metadata.get('id')) is int and metadata['id'] > 0, 'invalid GitHub artifact ID')
    require(type(metadata.get('size_in_bytes')) is int and metadata['size_in_bytes'] > 0,
            'invalid GitHub ZIP size')
    require(re.fullmatch(r'sha256:[0-9a-f]{64}', metadata.get('digest', '')), 'missing GitHub ZIP digest')
    require(metadata.get('workflow_run', {}).get('id') == run_id and
            metadata.get('workflow_run', {}).get('head_sha') == source,
            'artifact workflow run/source differs')
    if name is not None:
        require(metadata.get('name') == name, 'unexpected artifact name')


def check_zip(path, metadata):
    path = Path(path)
    require(path.is_file() and not path.is_symlink(), 'original ZIP must be a regular file')
    require(path.stat().st_size == metadata['size_in_bytes'] and
            'sha256:' + sha256(path) == metadata['digest'], 'original ZIP size/digest differs from GitHub')


def safe_extract(archive, destination):
    require(not destination.exists(), 'extraction destination already exists')
    with zipfile.ZipFile(archive) as zipped:
        entries = zipped.infolist()
        require(0 < len(entries) <= 1000, 'unexpected ZIP entry population')
        seen, files, total = set(), set(), 0
        for item in entries:
            raw = item.orig_filename
            parts = raw.rstrip('/').split('/')
            require(raw and not raw.startswith('/') and '\x00' not in raw and '\\' not in raw and
                    all(part not in ('', '.', '..') and ':' not in part for part in parts), 'unsafe ZIP path')
            name = '/'.join(parts)
            require(name not in seen, 'duplicate ZIP destination')
            seen.add(name)
            kind = stat.S_IFMT(item.external_attr >> 16)
            require(kind in (0, stat.S_IFREG, stat.S_IFDIR) and not item.flag_bits & 1,
                    'special or encrypted ZIP entry')
            require(kind != stat.S_IFDIR or item.is_dir(), 'ZIP directory type mismatch')
            if not item.is_dir():
                files.add(name)
                total += item.file_size
            require(item.file_size >= 0 and total <= MAX_EXTRACTED, 'ZIP expanded byte budget exceeded')
        for name in seen:
            require(not any(str(parent) in files for parent in PurePosixPath(name).parents),
                    'ZIP file/directory collision')
        require(shutil.disk_usage(destination.parent).free >= total + 128 * 1024 * 1024,
                'insufficient local space for bounded extraction')
        destination.mkdir(mode=0o700)
        for item in entries:
            target = destination.joinpath(*item.orig_filename.rstrip('/').split('/'))
            if item.is_dir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            target.parent.mkdir(parents=True, exist_ok=True)
            copied = 0
            with zipped.open(item) as source, target.open('xb') as output:
                for block in iter(lambda: source.read(1024 * 1024), b''):
                    copied += len(block)
                    require(copied <= item.file_size, 'ZIP entry exceeded its declared size')
                    output.write(block)
            require(copied == item.file_size, 'ZIP entry byte count mismatch')
            target.chmod(0o600)
    return {'entries': len(entries), 'uncompressed_bytes': total}


def bundle_root(extracted, receipt_name):
    candidates = list(extracted.rglob(receipt_name))
    require(len(candidates) == 1, 'expected one original ' + receipt_name)
    root = candidates[0].parent
    require(all(root in path.parents for path in extracted.rglob('*') if path.is_file()),
            'ZIP contains files outside the sealed bundle')
    return root


def expected_engine(root, matrix, snapshot, binary, output):
    # Match benchmark/runner.rs::engine_provenance exactly: all .rs/.toml
    # below crates, sorted relative paths, compact JSON tuple array, BLAKE3.
    entries = [[name, matrix.native_digest(binary, root / name)] for name in sorted(snapshot['inputs'])
               if name.startswith('crates/') and Path(name).suffix in ('.rs', '.toml')]
    path = output / 'engine-source-entries.json'
    with path.open('xb') as handle:
        handle.write(json.dumps(entries, ensure_ascii=False, separators=(',', ':')).encode())
    return {'engine_head_observed': snapshot['source_commit'],
            'source_files_digest': matrix.native_digest(binary, path),
            'cargo_lock_digest': matrix.native_digest(binary, root / 'Cargo.lock'),
            'source_entry_count': len(entries)}


def initialize(args):
    output = args.state.resolve()
    output.mkdir(parents=False)
    (output / 'shards').mkdir()
    review = {'schema': SCHEMA, 'operation': 'initialize', 'observed_utc': utc(), 'status': 'failed'}
    try:
        root, matrix, snapshot, driver = fixed_driver(args.root, args.source)
        metadata = read_json(args.metadata)
        check_metadata(metadata, args.run_id, args.source, f'p8-scale-build-{args.run_id}')
        check_zip(args.archive, metadata)
        review['safe_extraction'] = safe_extract(args.archive, output / 'build-validation')
        build_root = bundle_root(output / 'build-validation', 'build.json')
        binary = build_root / 'p8-scale'
        binary.chmod(0o700)  # only this derived copy's mode; original ZIP bytes stay unchanged
        built, binary = matrix.validate_build(build_root, root)
        require(matrix.read_json(build_root / 'source-before.json') == snapshot,
                'full original source inventory differs from exact Git source')
        engine = expected_engine(root, matrix, snapshot, binary, output)
        require(matrix.source_snapshot(root) == snapshot and matrix.driver_snapshot(root) == driver,
                'frozen source changed during independent hashing')
        check_zip(args.archive, metadata)
        write_new(output / 'source-snapshot.json', snapshot)
        write_new(output / 'expected-engine.json', engine)
        state = {'schema': SCHEMA, 'source_commit': args.source, 'run_id': args.run_id,
                 'root': str(root), 'build_directory': str(build_root), 'build_artifact': metadata,
                 'build_archive': str(args.archive.resolve()), 'build_receipt_sha256': sha256(build_root / 'build.json'),
                 'source_snapshot_sha256': sha256(output / 'source-snapshot.json'),
                 'expected_engine_sha256': sha256(output / 'expected-engine.json'),
                 'source_manifest_sha256': snapshot['manifest_sha256'], 'driver_source': driver,
                 'binary_sha256': built['binary_sha256'], 'binary_blake3': built['binary_blake3'],
                 'helper_sha256': sha256(Path(__file__)), 'created_utc': utc(), 'task_statuses_changed': False}
        write_new(output / 'state.json', state)
        review.update(status='passed_fixed_source_build_admission', state_sha256=sha256(output / 'state.json'),
                      source_commit=args.source, source_manifest_sha256=snapshot['manifest_sha256'],
                      input_count=snapshot['input_count'], expected_engine=engine)
    except Exception as error:
        review['error'] = type(error).__name__ + ': ' + str(error)
    write_new(output / 'initialization-review.json', review)
    return review


def context(state_directory):
    state_directory = state_directory.resolve(strict=True)
    state = read_json(state_directory / 'state.json')
    initialization = read_json(state_directory / 'initialization-review.json')
    require(initialization.get('status') == 'passed_fixed_source_build_admission' and
            initialization['state_sha256'] == sha256(state_directory / 'state.json'), 'review state seal differs')
    require(state['schema'] == SCHEMA and state['helper_sha256'] == sha256(Path(__file__)), 'review helper changed')
    root, matrix, snapshot, driver = fixed_driver(state['root'], state['source_commit'])
    require(sha256(state_directory / 'source-snapshot.json') == state['source_snapshot_sha256'] and
            snapshot == read_json(state_directory / 'source-snapshot.json') and driver == state['driver_source'],
            'full frozen source or driver inventory changed')
    build_root = Path(state['build_directory'])
    require(sha256(build_root / 'build.json') == state['build_receipt_sha256'], 'build receipt changed')
    built, binary = matrix.validate_build(build_root, root)
    require(sha256(state_directory / 'expected-engine.json') == state['expected_engine_sha256'], 'engine expectation changed')
    return state_directory, state, matrix, snapshot, built, binary


def review_shard(args):
    state_dir, state, matrix, snapshot, built, binary = context(args.state)
    metadata = read_json(args.metadata)
    check_metadata(metadata, state['run_id'], state['source_commit'])
    match = re.fullmatch(r'p8-scale-shard-(1000|5000|10000|50000|100000)-(\d+)-' + str(state['run_id']), metadata['name'])
    require(match is not None and 0 <= int(match[2]) < 30, 'unexpected scale/shard artifact name')
    output = state_dir / 'shards' / str(metadata['id'])
    output.mkdir()
    extracted = output / metadata['name']
    review = {'schema': SCHEMA, 'status': 'failed', 'observed_utc': utc(), 'artifact': metadata,
              'archive_path': str(args.archive.resolve()), 'source_commit': state['source_commit'],
              'state_sha256': sha256(state_dir / 'state.json'), 'task_statuses_changed': False}
    try:
        check_zip(args.archive, metadata)
        review['safe_extraction'] = safe_extract(args.archive, extracted)
        shard_root = bundle_root(extracted, 'shard.json')
        validated = matrix.validate_shard(shard_root, built, binary, state['build_receipt_sha256'])
        require(matrix.read_json(shard_root / 'source-before.json') == snapshot,
                'shard complete source inventory differs from exact Git source')
        require(validated['plan']['files'] == [int(match[1])] and
                validated['plan']['shard']['index'] == int(match[2]), 'artifact name/plan differs')
        expected = read_json(state_dir / 'expected-engine.json')
        for field in ('engine_head_observed', 'source_files_digest', 'cargo_lock_digest'):
            require(validated['engine'].get(field) == expected[field], 'raw engine differs from exact source: ' + field)
        require(validated['plan']['capacity_profile'] == matrix.CAPACITY_PROFILE and
                validated['plan']['repetitions'] == validated['plan']['shard']['count'] == 30,
                'not the registered 150-slice capacity matrix')
        check_zip(args.archive, metadata)
        write_new(output / 'validated-shard.json', validated)
        review.update(status='passed_original_validate_shard', sample_count=len(validated['measurements']),
                      compact_sha256=sha256(output / 'validated-shard.json'),
                      shard_receipt_sha256=validated['receipt_sha256'],
                      native_report=matrix.read_json(shard_root / 'native/report.json'))
    except Exception as error:
        review['error'] = type(error).__name__ + ': ' + str(error)
        if extracted.exists():
            reports = list(extracted.rglob('report.json'))
            if len(reports) == 1:
                try:
                    review['native_report'] = matrix.read_json(reports[0])
                except Exception:
                    pass
    # Only a derived extraction is removed, and only while the original ZIP
    # still matches its recorded external digest. This helper never deletes ZIPs.
    if extracted.exists():
        try:
            check_zip(args.archive, metadata)
            shutil.rmtree(extracted)
            review['derived_extraction_removed_original_zip_retained'] = True
        except Exception as error:
            review['local_cleanup_error'] = type(error).__name__ + ': ' + str(error)
    write_new(output / 'review.json', review)
    return review


def combine(args):
    state_dir, state, matrix, snapshot, built, binary = context(args.state)
    args.output.mkdir(parents=False)
    review = {'schema': SCHEMA, 'status': 'failed', 'observed_utc': utc(),
              'source_commit': state['source_commit'], 'task_statuses_changed': False}
    try:
        shards, lineage = [], []
        reviews = [read_json(path) | {'review_path': str(path)} for path in sorted((state_dir / 'shards').glob('*/review.json'))]
        reviews.sort(key=lambda row: row['artifact']['name'])
        require(len(reviews) == 150, 'exactly 150 original shard reviews required')
        require(len({row['artifact']['id'] for row in reviews}) == 150, 'duplicate artifact identity')
        for row in reviews:
            require(row['schema'] == SCHEMA and row['status'] == 'passed_original_validate_shard' and
                    row['state_sha256'] == sha256(state_dir / 'state.json'), 'failed or differently bound shard review')
            check_metadata(row['artifact'], state['run_id'], state['source_commit'])
            compact_path = Path(row['review_path']).parent / 'validated-shard.json'
            require(sha256(compact_path) == row['compact_sha256'], 'validated compact changed after replay')
            compact = matrix.read_json(compact_path)
            require(compact['receipt_sha256'] == row['shard_receipt_sha256'], 'original shard receipt differs')
            shards.append(compact)
            lineage.append({'artifact': row['artifact'], 'archive_path': row['archive_path'],
                            'review_sha256': sha256(Path(row['review_path'])),
                            'compact_sha256': row['compact_sha256'], 'shard_receipt_sha256': row['shard_receipt_sha256']})
        result = matrix.combine(shards, repetitions=30, shard_count=30, capacity_profile=matrix.CAPACITY_PROFILE)
        require(result['passed'] is True and result['sample_count'] == 1500 and len(result['groups']) == 50,
                'original complete matrix coverage differs')
        # These are the same provenance additions made by original aggregate().
        result.update(build_receipt_sha256=state['build_receipt_sha256'],
                      replay_driver_sha256=sha256(Path(state['root']) / 'scripts/p8_scale_matrix.py'),
                      driver_source=built['driver_source'])
        write_new(args.output / 'matrix.json', result)
        write_new(args.output / 'original-archive-lineage.json', lineage)
        review.update(status='passed_original_combine', sample_count=1500, shard_count=150, group_count=50,
                      matrix_sha256=sha256(args.output / 'matrix.json'),
                      archive_lineage_sha256=sha256(args.output / 'original-archive-lineage.json'),
                      scope='Unmodified original validate_shard was executed per raw ZIP; unmodified original combine consumes its sealed return values. This is measurement coverage, not G8/release certification.')
    except Exception as error:
        review['error'] = type(error).__name__ + ': ' + str(error)
    write_new(args.output / 'independent-review.json', review)
    return review


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(dest='command', required=True)
    init = subs.add_parser('init')
    init.add_argument('--root', type=Path, required=True)
    init.add_argument('--source', required=True)
    init.add_argument('--run-id', type=int, required=True)
    for command in (init, subs.add_parser('shard')):
        command.add_argument('--state', type=Path, required=True)
        command.add_argument('--archive', type=Path, required=True)
        command.add_argument('--metadata', type=Path, required=True)
    aggregate = subs.add_parser('combine')
    aggregate.add_argument('--state', type=Path, required=True)
    aggregate.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = {'init': initialize, 'shard': review_shard, 'combine': combine}[args.command](args)
    except Exception as error:
        result = {'status': 'failed_before_output_admission', 'error': type(error).__name__ + ': ' + str(error)}
    print(json.dumps({key: result[key] for key in ('status', 'sample_count', 'shard_count', 'error') if key in result}, ensure_ascii=False))
    return 0 if result.get('status', '').startswith('passed_') else 1


if __name__ == '__main__':
    raise SystemExit(main())
