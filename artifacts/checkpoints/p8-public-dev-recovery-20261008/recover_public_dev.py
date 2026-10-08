#!/usr/bin/env python3
"""Public DEV recovery only. No provider, scorer run, source execution, or holdout access."""
import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path, PurePosixPath
import subprocess
import sys

HERE = Path(__file__).resolve().parent
AUTHOR = 'b3565a61cea32a67e96963902aa89caa48e35f86'
REVIEW_IMPORT = 'f61b33a2ff2ccfb412f92b9aba70ac1b6673ee8d'
FAMILY_REVIEW = '37b0a99402750651caa84e171e71cccb1d47daa2'
ARTIFACT = 'artifacts/benchmarks/p8-public-dev-20261008'
INTAKE = 'crates/cc-eval/benchmarks/public-v19/{repo}/intake/public-dev-20261008/'
MAX_BLOB = 4 * 1024 * 1024
MAX_TOTAL = 64 * 1024 * 1024

def require(condition, message):
    if not condition:
        raise ValueError(message)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def git_oid(kind, data):
    return hashlib.sha1(kind.encode() + b' ' + str(len(data)).encode() + b'\0' + data).hexdigest()

def relative(value):
    path = PurePosixPath(value)
    require(value and not path.is_absolute() and '..' not in path.parts and
            value == path.as_posix() and '\\' not in value, 'unsafe relative path')
    return path

def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()

def deterministic_split(family):
    raw = hashlib.sha256(b'codecortex-public-v19-split-v1\n' + family.encode()).digest()
    return 'holdout' if int.from_bytes(raw[:8], 'big') < 2**62 else 'dev'

def check_tree(tree, expected):
    require(tree['sha'] == expected and tree.get('truncated') is False, 'tree identity/truncation')
    groups = {'': []}
    seen = set()
    for entry in tree['tree']:
        path = relative(entry['path'])
        require(entry['path'] not in seen, 'duplicate tree entry')
        seen.add(entry['path'])
        require(entry['type'] in ('blob', 'tree', 'commit'), 'unknown Git entry type')
        require(len(entry['sha']) == 40, 'invalid child oid')
        parent = '' if str(path.parent) == '.' else str(path.parent)
        groups.setdefault(parent, []).append(entry)
        if entry['type'] == 'tree':
            groups.setdefault(entry['path'], [])
    computed = {}
    for directory in sorted(groups, key=lambda p: len(PurePosixPath(p).parts), reverse=True):
        body = bytearray()
        entries = sorted(groups[directory], key=lambda e:
                         (PurePosixPath(e['path']).name + ('/' if e['type'] == 'tree' else '\0')).encode())
        for entry in entries:
            if entry['type'] == 'tree':
                require(computed.get(entry['path']) == entry['sha'], 'child tree mismatch')
            mode = '40000' if entry['mode'] == '040000' else entry['mode']
            body.extend((mode + ' ' + PurePosixPath(entry['path']).name).encode() + b'\0')
            body.extend(bytes.fromhex(entry['sha']))
        computed[directory] = git_oid('tree', bytes(body))
    require(computed[''] == expected, 'root tree mismatch')
    return {'entries': len(seen), 'trees_including_root': len(computed), 'root': computed['']}

def check_metadata(manifest):
    result, trees = [], {}
    for spec in manifest['upstream']:
        commit_raw = (HERE / spec['commit_file']).read_bytes()
        tree_raw = (HERE / spec['tree_file']).read_bytes()
        require(sha(commit_raw) == spec['commit_file_sha256'], 'commit metadata byte drift')
        require(sha(tree_raw) == spec['tree_file_sha256'], 'tree metadata byte drift')
        commit, tree = json.loads(commit_raw), json.loads(tree_raw)
        require(commit['sha'] == spec['commit'] and commit['tree']['sha'] == spec['tree'],
                'fixed upstream commit/tree drift')
        value = check_tree(tree, spec['tree'])
        require(value['entries'] == spec['tree_entries'], 'fixed tree inventory drift')
        dev, held = [], 0
        for n in range(1, 21):
            family = f"v19.{spec['repo_id']}.f{n:04d}"
            if deterministic_split(family) == 'dev':
                dev.append(family)
            else:
                held += 1
        require(dev == spec['reservations']['dev_families'], 'DEV reservation drift')
        require(held == spec['reservations']['holdout_reserved_count'], 'reserved count drift')
        result.append({'repo': spec['repo_id'], **value, 'public_dev_families': dev,
                       'undrafted_unread_holdout_slots': held})
        trees[spec['repo_id']] = {entry['path']: entry for entry in tree['tree']}
    for license in manifest['licenses']:
        data = (HERE / license['file']).read_bytes()
        require(sha(data) == license['sha256'] and len(data) == license['utf8_bytes'], 'license byte drift')
        require(git_oid('blob', data) == license['git_blob'], 'license Git identity')
        require(trees[license['label']][license['path']]['sha'] == license['git_blob'], 'license tree binding')
    for record in manifest['protocol']['files']:
        data = (HERE / record['file']).read_bytes()
        require(sha(data) == record['sha256'] and len(data) == record['utf8_bytes'],
                'original protocol bytes changed')
    return result, trees

def git(repo, *args, limit=MAX_TOTAL):
    process = subprocess.run(['git', '-C', str(repo), *args], capture_output=True, timeout=60)
    require(process.returncode == 0, 'required original Git object unavailable: ' + ' '.join(args[:2]))
    require(len(process.stdout) <= limit, 'Git response exceeds recovery budget')
    return process.stdout

def has_commit(repo, commit):
    return subprocess.run(['git', '-C', str(repo), 'cat-file', '-e', commit + '^{commit}'],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=15).returncode == 0

def entries(repo, commit, prefix):
    raw = git(repo, 'ls-tree', '-r', '-z', commit, '--', prefix)
    result = []
    for item in raw.split(b'\0'):
        if not item:
            continue
        fields, path = item.split(b'\t', 1)
        mode, kind, oid = fields.decode().split()
        path = path.decode()
        relative(path)
        require(path.startswith(prefix), 'Git path escaped public prefix')
        require(kind == 'blob' and mode in ('100644', '100755'), 'nonregular original file')
        result.append((path, mode, oid))
    return result

def copy_blob(repo, oid, target, expected=None):
    size = int(git(repo, 'cat-file', '-s', oid, limit=100))
    require(0 <= size <= MAX_BLOB, 'original blob exceeds recovery bound')
    data = git(repo, 'cat-file', 'blob', oid, limit=MAX_BLOB)
    require(len(data) == size and git_oid('blob', data) == oid, 'original Git blob content mismatch')
    if expected is not None:
        require(sha(data) == expected, 'recorded historical SHA256 mismatch')
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    return data

def recover_author(repo, out, manifest, trees, check_format):
    needed = [AUTHOR, REVIEW_IMPORT]
    unavailable = [commit for commit in needed if not has_commit(repo, commit)]
    if unavailable:
        return {'status': 'blocked_original_Git_objects_unavailable', 'missing_commits': unavailable,
                'original_public_payload_recovered': False}, 2
    recovered, stored, total_bytes, rows_by_repo = [], [], 0, {}
    for spec in manifest['upstream']:
        name = spec['repo_id']
        prefix = INTAKE.format(repo=name)
        subset = entries(repo, AUTHOR, prefix)
        require(subset, 'original public intake missing')
        for path, mode, oid in subset:
            rel = path[len(prefix):]
            # This fixed prefix was authored only for the recorded DEV intake.
            # Explicit protected names and unknown query shards are rejected before blob reads.
            if not rel.startswith(('source/', 'licenses/', 'review-only/')):
                require(not any(word in rel.lower() for word in ('holdout', 'sealed', 'custody')),
                        'protected/non-DEV artifact rejected before reading')
                if rel.endswith('.jsonl'):
                    require(PurePosixPath(rel).name in
                            ('queries.native.candidate.dev.jsonl', 'queries.compat.candidate.dev.jsonl'),
                            'unknown query shard rejected before reading')
            target = out / 'restored' / path
            data = copy_blob(repo, oid, target)
            total_bytes += len(data)
            require(total_bytes <= MAX_TOTAL, 'total original recovery budget exceeded')
            record = {'path': path, 'original_mode': mode, 'git_blob': oid,
                      'bytes': len(data), 'sha256': sha(data)}
            if rel.startswith('source/'):
                source_path = rel.removeprefix('source/')
                original = trees[name].get(source_path)
                require(original and original['type'] == 'blob' and original['sha'] == oid,
                        'original indexed source differs from fixed upstream')
                record['upstream_mode'] = original['mode']
                record['kind'] = 'indexed_source'
                stored.append(record)
            elif rel.startswith(('licenses/', 'review-only/')):
                require(any(item['type'] == 'blob' and item['sha'] == oid for item in trees[name].values()),
                        'retained/excluded source differs from fixed upstream')
                record['kind'] = 'license' if rel.startswith('licenses/') else 'review_only_excluded'
                stored.append(record)
            recovered.append(record)
        base = out / 'restored' / prefix / 'revision-2'
        native_path = base / 'queries.native.candidate.dev.jsonl'
        compat_path = base / 'queries.compat.candidate.dev.jsonl'
        native = [json.loads(line) for line in native_path.read_bytes().splitlines() if line.strip()]
        compat = [json.loads(line) for line in compat_path.read_bytes().splitlines() if line.strip()]
        expected_families = set(spec['reservations']['dev_families'])
        require({row['query_family'] for row in native} == expected_families and
                len(native) == len(expected_families), 'original DEV family population mismatch')
        require(len(native) == len(compat), 'original projection count mismatch')
        for row in native + compat:
            require(row['split'] == 'dev' and deterministic_split(row['query_family']) == 'dev',
                    'non-DEV row in fixed public intake')
        require({row['id'] for row in native} == {row['id'] for row in compat}, 'projection ID mismatch')
        spans = []
        for row in native:
            for group in row['answers']:
                for alternative in group['alternatives']:
                    if alternative.get('span') is not None:
                        p = relative(alternative['path']).as_posix()
                        source = (out / 'restored' / prefix / 'source' / p).read_bytes()
                        span = alternative['span']
                        require(0 <= span['start'] < span['end'] <= len(source), 'original gold span range')
                        source[span['start']:span['end']].decode('utf-8')
                        spans.append({'id': row['id'], 'group': group['id'], 'path': p, **span,
                                      'source_sha256': sha(source)})
        rows_by_repo[name] = {'native': native_path, 'compat': compat_path, 'rows': len(native), 'spans': spans}
    counts = {kind: sum(row['kind'] == kind for row in stored)
              for kind in ('indexed_source', 'license', 'review_only_excluded')}
    require(counts == {'indexed_source': 224, 'license': 5, 'review_only_excluded': 9}, 'original238 subset counts')
    require(len(stored) == 238 and sum(row['bytes'] for row in stored) == 2555289, 'original stored byte count')
    spans = [span for group in rows_by_repo.values() for span in group['spans']]
    require(len(spans) == 32, 'revision2 exact span population')
    review = manifest['historical_checkpoint_claims']['source_gold_review']
    raw_review = git(repo, 'show', REVIEW_IMPORT + ':' + review['path'], limit=MAX_BLOB)
    require(sha(raw_review) == review['sha256'], 'original non-author source/gold receipt hash')
    review_target = out / 'restored' / review['path']
    review_target.parent.mkdir(parents=True, exist_ok=True)
    review_target.write_bytes(raw_review)
    mode_record = manifest['historical_checkpoint_claims']['snapshot_mode_disclosure']
    raw_modes = git(repo, 'show', REVIEW_IMPORT + ':' + mode_record['path'], limit=MAX_BLOB)
    require(sha(raw_modes) == mode_record['sha256'], 'original mode disclosure hash')
    mode_target = out / 'restored' / mode_record['path']
    mode_target.parent.mkdir(parents=True, exist_ok=True)
    mode_target.write_bytes(raw_modes)
    drift = [row for row in stored if row.get('upstream_mode', row['original_mode']) != row['original_mode']]
    require(len(drift) == 3 and all(row['original_mode'] == '100644' and row['upstream_mode'] == '100755'
                                  for row in drift), 'original three mode differences')
    (out / 'original-file-inventory.json').write_bytes(json_bytes(recovered))
    (out / 'original-32-gold-spans.json').write_bytes(json_bytes(spans))
    family = manifest['historical_checkpoint_claims']['global_components']
    family_state = {'status': 'original_global_review_unavailable', 'canonical_projection_executed': False}
    if has_commit(repo, FAMILY_REVIEW):
        family_prefix = 'artifacts/checkpoints/p8-public-dev-family-review-20261008/'
        for record in family['files']:
            data = git(repo, 'show', FAMILY_REVIEW + ':' + family_prefix + record['name'], limit=MAX_BLOB)
            require(sha(data) == record['sha256'], 'original global review hash')
            target = out / 'restored' / family_prefix / record['name']
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(data)
        family_state['status'] = 'original_three_global_review_files_byte_recovered_not_applied'
    checker = {'status': 'not_run', 'reason': 'explicit --check-original-format not supplied'}
    if check_format:
        require(importlib.metadata.version('jsonschema') == '4.23.0', 'explicit original jsonschema4.23.0 required')
        command = [sys.executable, str(HERE / 'protocol-reference/check.py')]
        for name, group in rows_by_repo.items():
            for profile in ('native', 'compat'):
                command += ['--shard', f"{name}:{profile}={group[profile]}"]
        command += ['--output', str(out / 'original-format-result.json')]
        process = subprocess.run(command, capture_output=True, timeout=120)
        (out / 'original-format.stdout.log').write_bytes(process.stdout)
        (out / 'original-format.stderr.log').write_bytes(process.stderr)
        checker = {'command': command, 'exit_code': process.returncode,
                   'stdout_sha256': sha(process.stdout), 'stderr_sha256': sha(process.stderr)}
        require(process.returncode == 0, 'original checker rejected recovered revision2')
        check = json.loads((out / 'original-format-result.json').read_bytes())
        require(check['status'] == 'format_checks_passed_not_source_gold_review', 'unexpected checker status')
        checker['status'] = check['status']
    return {'status': 'original_revision2_public_bytes_recovered', 'author_commit': AUTHOR,
            'stored_blobs': 238, 'source_counts': counts, 'native_rows': 26, 'compat_rows': 26,
            'native_gold_spans': 32, 'mode_differences_preserved': drift,
            'original_source_gold_review_sha256': review['sha256'],
            'global_component_review': family_state, 'original_checker': checker,
            'new_family_admission': False, 'formal600_completion': False}, 0

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--original-git', type=Path)
    parser.add_argument('--check-original-format', action='store_true')
    args = parser.parse_args()
    require(not args.output.exists(), 'output must be new; previous evidence is immutable')
    require(not args.check_original_format or args.original_git is not None,
            'original format replay requires original author objects')
    args.output.mkdir(parents=True)
    manifest_raw = (HERE / 'recovery-manifest.json').read_bytes()
    manifest = json.loads(manifest_raw)
    trees_checked, trees = check_metadata(manifest)
    result = {'schema_version': 1, 'recovery_manifest_sha256': sha(manifest_raw),
              'status': 'metadata_verified_original_public_payload_still_pending',
              'upstream_trees': trees_checked, 'exact_license_texts_verified': 5,
              'original_protocol_files_verified': 3, 'holdout_bodies_read_or_written': 0,
              'source_gold_or_family_admission_granted': False, 'tasks_changed': False,
              'source_executables_or_provider_called': False}
    exit_code = 0
    if args.original_git is not None:
        result['original_recovery'], exit_code = recover_author(
            args.original_git, args.output, manifest, trees, args.check_original_format)
        result['status'] = result['original_recovery']['status']
    (args.output / 'recovery-result.json').write_bytes(json_bytes(result))
    print(json.dumps(result, sort_keys=True, indent=2))
    return exit_code

if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (ValueError, OSError, subprocess.SubprocessError, json.JSONDecodeError,
            importlib.metadata.PackageNotFoundError) as error:
        # Do not print query or gold bodies in diagnostics.
        print('Public DEV recovery failed: ' + str(error), file=sys.stderr)
        raise SystemExit(2)
