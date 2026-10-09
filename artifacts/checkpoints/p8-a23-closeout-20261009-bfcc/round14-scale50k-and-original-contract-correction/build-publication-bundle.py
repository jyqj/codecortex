from pathlib import Path, PurePosixPath
import hashlib, json, zipfile, stat, datetime, re

workspace = Path('/workspace/scratch/bfccb8494ba0')
output = Path('/dev/shm/pr-triage-round14-publication')
prefix = 'artifacts/checkpoints/p8-a23-closeout-20261009-bfcc/round14-scale50k-and-original-contract-correction'
def sha(data):
    return hashlib.sha256(data).hexdigest()
def identity(path, repository_path=None):
    data = path.read_bytes()
    result = dict(local_path=str(path), bytes=len(data), sha256=sha(data), git_blob=hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest(), git_mode='100644')
    if repository_path is not None:
        result['repository_path'] = repository_path
    return result
def safe(name):
    path = PurePosixPath(name)
    assert not path.is_absolute() and '..' not in path.parts
    assert name == path.as_posix() and '\x00' not in name and '\\' not in name
def write_json(name, value):
    with (output / name).open('x') as f:
        json.dump(value, f, ensure_ascii=False, indent=2); f.write('\n')
originals = {}
def frozen(path, expected_bytes=None, expected_sha=None):
    assert path.is_file() and not path.is_symlink()
    data = path.read_bytes()
    if expected_bytes is not None:
        assert len(data) == expected_bytes
    if expected_sha is not None:
        assert sha(data) == expected_sha
    originals[str(path)] = dict(bytes=len(data), sha256=sha(data))
    return data
def selected(selection_path, group, source_base=None):
    selection_data = frozen(selection_path)
    selection = json.loads(selection_data)
    entries = []
    for row in selection['files']:
        path = Path(row['local_path']) if 'local_path' in row else Path(selection.get('base', selection.get('root', str(source_base)))) / row['path']
        data = frozen(path, row['bytes'], row['sha256'])
        if 'local_path' in row:
            relative = path.relative_to(selection_path.parent).as_posix()
        else:
            relative = row['path']
            if source_base is not None:
                relative = path.relative_to(source_base).as_posix()
        member = group + '/' + relative
        safe(member)
        entries.append((member, data, str(path)))
    if 'file_count' in selection:
        assert len(entries) == selection['file_count']
    if 'total_bytes' in selection:
        assert sum(len(row[1]) for row in entries) == selection['total_bytes']
    selection_member = group + '/' + (selection_path.relative_to(source_base).as_posix() if source_base is not None else selection_path.name)
    entries.append((selection_member, selection_data, str(selection_path)))
    return entries
def zip_receipt(path, expected_entries=None):
    rows = []
    with zipfile.ZipFile(path) as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        assert len(names) == len(set(names))
        if expected_entries is not None:
            assert names == [row[0] for row in expected_entries]
        for i, info in enumerate(infos):
            safe(info.filename.rstrip('/'))
            assert info.orig_filename == info.filename
            assert not stat.S_ISLNK(info.external_attr >> 16)
            assert not (info.flag_bits & 1)
            data = archive.read(info)
            if expected_entries is not None:
                assert data == expected_entries[i][1]
            row = dict(path=info.filename, bytes=len(data), sha256=sha(data), crc32='%08x' % info.CRC)
            if expected_entries is not None:
                row['original_local_path'] = expected_entries[i][2]
            rows.append(row)
    return dict(member_count=len(rows), uncompressed_bytes=sum(row['bytes'] for row in rows), complete_crc_and_sha_read=True, members=rows)
def make_zip(name, entries):
    path = output / name
    assert len(entries) == len({row[0] for row in entries})
    with zipfile.ZipFile(path, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for name, data, source in entries:
            safe(name)
            info = zipfile.ZipInfo(name, (2026, 10, 9, 0, 0, 0))
            info.create_system = 3
            info.external_attr = (stat.S_IFREG | 0o644) << 16
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, data, compresslevel=9)
    return path, zip_receipt(path, entries)

scale_selection = workspace / 'scale-intake-primary/third-shard-50k-selection.json'
scale_entries = selected(scale_selection, 'scale-intake-primary', source_base=workspace / 'scale-intake-primary')
assert len(scale_entries) == 33
assert sum(len(row[1]) for row in scale_entries[:-1]) == 12668696
scale_zip, scale_receipt = make_zip('scale50k-selected-originals.zip', scale_entries)

review_entries = []
for selection, group, source_base in [
    (Path('/dev/shm/pr-triage-bfcc-c8be5afa/db-lock-contract-review/archive-selection.json'), 'pr-triage/db-lock-contract-review', None),
    (Path('/dev/shm/pr-triage-bfcc-c8be5afa/db-lock-implementation-followup-round14/archive-selection.json'), 'pr-triage/db-lock-implementation-followup-round14', None),
    (workspace / 'scale-intake-primary/target-ten-closeout-db-wait-correction/archive-selection.json', 'workspace', workspace),
    (workspace / 'scale-intake-primary/pr180-native-peer/archive-selection.json', 'workspace', workspace),
]:
    review_entries.extend(selected(selection, group, source_base))
assert len(review_entries) == 23
review_zip, review_receipt = make_zip('original-contract-and-independent-reviews.zip', review_entries)

existing = [
    (Path('/dev/shm/todo-audit-pr180-safe-integration-plan.zip'), 'todo-audit-pr180-safe-integration-plan.zip', 1186137, 'e3e0ea9260dc3d3627acfa4165ce0ededee127625e237ed429e28217fc10432f'),
    (workspace / 'root-round13/current-merge-trees-transfer/official-current-merge-trees.zip', 'official-current-merge-trees.zip', 4400549, '10b86f4933112ac3f436ea955af0653701e9519cae6d35b39c66ddbf9d3d8697'),
]
existing_receipts = []
for path, name, count, digest in existing:
    frozen(path, count, digest)
    existing_receipts.append(dict(original=identity(path), original_bytes_reused_without_repack=True, inspection=zip_receipt(path)))
prep = workspace / 'root-round14/product-and-native-preparation.json'
prep_data = frozen(prep, 16198, '825379663c25a5d85f1ea423f64f7f137bc71c8d3003aefb493a44e556314e38')
prep_json = json.loads(prep_data)
assert prep_json['source_P'] == 'c92eb5ac7ece70d1f62271d7e2dacac513c285b5'
assert prep_json['recorded_at'] == '2026-10-09 10:46:04 UTC'
assert prep_json['native_controls_dispatch']['structuredContent']['output']['command_completed'] is False

files = []
split_receipts = []
for path in [scale_zip, review_zip]:
    if path.stat().st_size <= 8 * 1024 * 1024:
        files.append(identity(path, prefix + '/' + path.name))
    else:
        data = path.read_bytes()
        parts = []
        for ordinal, offset in enumerate(range(0, len(data), 8 * 1024 * 1024)):
            part = output / (path.name + '.part%02d' % ordinal)
            with part.open('xb') as f:
                f.write(data[offset:offset + 8 * 1024 * 1024])
            item = identity(part, prefix + '/' + part.name)
            files.append(item)
            parts.append(dict(ordinal=ordinal, offset=offset, **item))
        assert b''.join(Path(row['local_path']).read_bytes() for row in parts) == data
        split_receipts.append(dict(original_zip=identity(path), ordered_parts=parts, actual_rejoined_bytes_and_sha_equal=True))
for path, name, _, _ in existing:
    files.append(identity(path, prefix + '/' + name))
files.append(identity(prep, prefix + '/root-round14/product-and-native-preparation.json'))

readme = '''# Round 14: fixed-source 50k evidence and original DB-wait contract correction

This is an append-only evidence publication. It does not change product source, the original task ledger, any workload, or any earlier pass/failure record. The original task count remains **163 done / 29 remaining (192 total), zero newly closed here**.

## Correction to previous completion summaries

This README and the retained correction report supersede the **“only the complete scale study remains”** summary in the previous round13 soak README and task maps. The old files remain preserved as historical statements. The unchanged original `09-BENCHMARK.md` §9, line 175 requires DB lock wait observation in the actual C1/4/8/16 mixed path. **P8-007 still lacks that actual acquisition-wait evidence**, in addition to its unchanged dependencies.

The retained mixed/backfill/lifecycle/soak component validations remain passed in their demonstrated scope. Client dispatch/queue/service/end-to-end timings do not isolate internal DB acquisition wait. The original backfill once-per-seed DB availability probe ran before the held-query waves; its pool checkout and combined BEGIN/ROLLBACK timing does not replace actual mixed-path acquisition measurements. No new full-backend tracing, latency threshold, four-C one-hour workload, or formal150 gate is introduced.

## Fixed original scale evidence

`scale50k-selected-originals.zip` contains the exact **32 selected files (12,668,696 bytes)** plus the original **`third-shard-50k-selection.json`**. It is a new transport container of retained files, **not the original GitHub artifact ZIP**. Its paths retain the `scale-intake-primary/` layout. The original source is **Gc8 `c8be5afaac568ffd40ef86d3795423c3b73c9f39`**, run **37902429727**, artifact **11610565832**, 50k repetition 0. The original owner has accepted **4/150 shards and 41/1500 samples** across that fixed study. This package adds no samples, does not rerun its original validator, and does not mark the complete study accepted.

## Reviews and unchanged existing ZIPs

`original-contract-and-independent-reviews.zip` holds the exact 5-file original-contract audit, 9-file public implementation discovery, 3-file task-map correction, and 2-file existing native peer review, plus all four original selections: **23 members**. Group layouts are `pr-triage/db-lock-contract-review/`, `pr-triage/db-lock-implementation-followup-round14/`, and `workspace/scale-intake-primary/...`. The original selections retain their original bytes and target paths; this packaging manifest explicitly maps them into this new publication container.

`todo-audit-pr180-safe-integration-plan.zip` and `official-current-merge-trees.zip` are reused byte for byte, not extracted into or repacked inside another ZIP. Their SHA256, Git blob and complete CRC/member-read receipts are in the manifest. The public discovery report records that the a217 DB-observation workstream had not yet supplied a locatable published implementation commit in its checked scope; it is not a review or acceptance of that implementation.

## New product/native preparation is a pending snapshot

`root-round14/product-and-native-preparation.json` is the exact frozen **2026-10-09 10:46:04 UTC** observation. It records actual P **`c92eb5ac7ece70d1f62271d7e2dacac513c285b5`**, tree **`9ae248f8e0c42a8a7389e26e3f44710dfb029640`**, with ordered parents **4652cad11dde4b41126544a38fddf25eb2fb7474** and **55aa2bcf355441585bcf980e1d6f4fab8eebe59d**. P preserves current main and the public snapshot API-prefix repair; it is **not** the DB-observation implementation. Native preparation actually exited 0. Native controls were only started in that frozen record: **no terminal controls result, R/G acceptance, cold-build, performance, or task-closure credit is claimed here**. Any later result must be archived separately with its actual source and identity.

## Byte-preserving packaging

Every selected input was checked against its original length/SHA256, all new and reused ZIP members were fully read for CRC and SHA256, and selected inputs were rechecked unchanged after packaging. `manifest.json` holds the complete member/selection identities. `upload-index.json` lists exact repository paths, bytes, SHA256 and Git blob IDs. Each upload item is at most 8 MiB; if splitting was necessary, the manifest lists ordered offsets and verified reconstruction. This preparation performed no Git mutation, workload, validator replay or sampling.
'''
with (output / 'README.md').open('x') as f:
    f.write(readme)
manifest = dict(schema_version=1, prepared_at=datetime.datetime.now(datetime.timezone.utc).isoformat(), prefix=prefix,
                scope='Byte/CRC/manifest checks only; append-only publication preparation, no original validator or workload rerun.',
                source_separation=dict(original_study_G='c8be5afaac568ffd40ef86d3795423c3b73c9f39', original_run=37902429727, accepted_shards=4, expected_shards=150, accepted_samples=41, expected_samples=1500,
                                       new_P=prep_json['source_P'], new_P_tree=prep_json['tree'], new_P_parents=prep_json['parents'], native_preparation_exit=0, native_controls_terminal=False, original_TODO_remaining=29),
                new_archives=[dict(file=identity(scale_zip), inspection=scale_receipt), dict(file=identity(review_zip), inspection=review_receipt)],
                reused_original_archives=existing_receipts, ordered_split_archives=split_receipts,
                original_input_identities=[dict(path=path, **item) for path, item in sorted(originals.items())],
                builder=identity(Path(__file__)))
for path, expected in originals.items():
    data = Path(path).read_bytes()
    assert len(data) == expected['bytes'] and sha(data) == expected['sha256']
manifest['all_original_inputs_rechecked_unchanged_after_packaging'] = True
write_json('manifest.json', manifest)
for name in ['README.md', 'manifest.json', 'build-publication-bundle.py']:
    files.append(identity(output / name, prefix + '/' + name))
assert all(row['bytes'] <= 8 * 1024 * 1024 for row in files)
index = dict(schema_version=1, prefix=prefix, file_count=len(files), total_bytes=sum(row['bytes'] for row in files), files=files,
             scope='Ready for root-controlled append-only publication; no remote writes performed. Original selections and files unchanged.',
             original_task_counts=dict(total=192, done=163, remaining=29, newly_done=0),
             interpretation='Original component passes preserved; P8-007 actual-load DB acquisition wait remains missing in addition to formal scale. New P native snapshot records preparation success and controls started only.')
write_json('upload-index.json', index)
print(json.dumps(identity(output / 'upload-index.json', prefix + '/upload-index.json')))
print(json.dumps(dict(file_count=index['file_count'], total_bytes=index['total_bytes'], zip_sizes={path.name:path.stat().st_size for path in [scale_zip,review_zip]}, split_count=len(split_receipts))))
