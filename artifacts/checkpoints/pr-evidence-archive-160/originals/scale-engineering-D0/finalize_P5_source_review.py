#!/usr/bin/env python3
"""Generate R4 only after exact P5 and independently checked D0 prerequisites exist.

This reads immutable Git objects and retained evidence. It cannot publish, run
Cargo/native workloads, edit a checkout, install guard pins, or close a TODO.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import subprocess
import sys
import tarfile

BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
G3 = '24b0cba53178d14ef6c02355ee271fa0067be3e0'
P4 = 'f5518ca6d21f54cfd324e33edc3b739608ae28b8'
R3 = '5463095d9650402161a23a332c2505b15c4d88b6'
D0 = 'd0cb69c601e530dcef738af0c2dffb8d3b8bcf28'
OLD_REVIEW_PATH = 'artifacts/checkpoints/p8-completion-20261009/independent-source-review.json'
OLD_REVIEW_SHA = 'd4e920f6baa0b82bb277987faa86820c31e2f2855c30f9b8a925a51eb0dd2952'
PROSPECTIVE_SHA = '50f621b5a12f6e09308181c2b61fd6e66a0ddaef81d379c6c7e3f2b0c40a6ec5'
MANIFEST_SHA = '496d5443c0ba28d8293307dc9a2cbb6cff25e86d1b94c35b8d52ac327867561e'
ARCHIVE_SHA = '2e518473d5edc22dc5654a57b73e81ae59d50a604a5a10dd8944b46a4970f5aa'
PUBLIC_DIRECTORY = 'artifacts/checkpoints/p8-completion-20261009/scale-engineering-D0'
HERE = Path(__file__).resolve().parent
EXCLUSIONS = {'scripts/verify_reviewed_source_v15.py',
              'scripts/reviewed-source-registry-v15.json', '.github/workflows/ci.yml'}
FROZEN = ('scripts/verify_reviewed_source_v14.py', 'scripts/reviewed-source-registry-v14.json',
          'tests/source_integrity/test_reviewed_source_v14.py', 'scripts/v14_historical_context.py',
          'tests/source_integrity/v14_historical_test_adapter.py',
          'tests/source_integrity/test_v14_historical_context.py')
EVIDENCE_PINS = {
    'prefix_non_author_review': '75cc5f288fbebf931bef52b16083ed06eae8b969252dd236e67a5fc00c951d0b',
    'ddl_non_author_review': '5acabc058d7a34121445fc132ea83ffd2ee30a72d3e93fd9c5d8a2bd2ceb2635',
    'oracle_non_author_review': 'ad0f97754f7a8cfef7c64c25c01a5f628363cf7a21e87d69e62421737e7a3a8d',
    'D0_controls_reception': '39cb5fa78be96b511d81141d082e6d18f607c0b75e99230299f9fa23dda4142e',
    'D0_build_reception': 'f757e039945829badd7e6e07a71f12c35d35d4567247def381ff97361a910ea6',
    'D0_source_bridge': '50ab5c0a07eff5923a4e2b1a62ac4fd39fe711be5988fd362f43d4e933048a6d',
    'D0_1k_diagnostic_reception': '6a72c52478b00f4d756c2a8a4b8297ce0c62ef8b68a70b1c022e3da60c7b9889',
    'D0_10k_diagnostic_reception': 'f432d7a6742573d155c3cde5d239c268178d06ba7dd2bf5562e349d79d1e7bbd',
}
EVIDENCE_MEMBERS = {
    'prefix_non_author_review': 'prefix-independent-review/audit.json',
    'ddl_non_author_review': 'doc-key-index-independent-review/audit.json',
    'oracle_non_author_review': 'oracle-independent-pr-review/audit.json',
    'D0_controls_reception': 'original-controls-review/audit.json',
    'D0_build_reception': 'original-build-validation/review.json',
    'D0_source_bridge': 'source-bridge-preparation/D0-study-feasibility-review.json',
    'D0_1k_diagnostic_reception': 'original-diagnostic-validation/11582323631/review.json',
    'D0_10k_diagnostic_reception': 'original-diagnostic-validation/11582922326/review.json',
}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def encode(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def checked(path, digest):
    raw = Path(path).read_bytes()
    require(sha(raw) == digest, 'retained evidence digest differs: ' + str(path))
    return json.loads(raw)


def is_validation(path):
    return (path.startswith(('scripts/', '.github/workflows/', 'tests/source_integrity/'))
            and path not in EXCLUSIONS
            and not ('__pycache__' in PurePosixPath(path).parts and path.endswith('.pyc')))


class Git:
    def __init__(self, root):
        self.root = str(Path(root).resolve(strict=True))

    def run(self, *args):
        return subprocess.check_output(['git', '-C', self.root, *args])

    def tree(self, source):
        result = {}
        for row in self.run('ls-tree', '-r', '-z', source).split(b'\0'):
            if row:
                metadata, path = row.decode().split('\t', 1)
                mode, kind, oid = metadata.split()
                require(path not in result, 'duplicate Git tree path')
                result[path] = {'mode': mode, 'kind': kind, 'oid': oid}
        return result

    def hashes(self, tree, paths):
        paths = sorted(paths)
        for path in paths:
            require(tree[path]['kind'] == 'blob' and tree[path]['mode'] in ('100644', '100755'),
                    'unsupported source mode: ' + path)
        refs = ''.join(tree[p]['oid'] + '\n' for p in paths).encode()
        raw = subprocess.check_output(['git', '-C', self.root, 'cat-file', '--batch'], input=refs)
        offset = 0
        result = {}
        for path in paths:
            end = raw.index(b'\n', offset)
            oid, kind, size = raw[offset:end].decode().split()
            require(oid == tree[path]['oid'] and kind == 'blob', 'Git batch object identity differs')
            offset = end + 1
            body = raw[offset:offset + int(size)]
            require(len(body) == int(size) and raw[offset + int(size):offset + int(size) + 1] == b'\n',
                    'truncated Git batch object')
            require(hashlib.sha1(b'blob ' + size.encode() + b'\0' + body).hexdigest() == oid,
                    'Git blob content differs')
            result[path] = sha(body)
            offset += int(size) + 1
        require(offset == len(raw), 'unexpected Git batch tail')
        return result


def intake(git, source, expected_tree, prospective, old):
    require(re.fullmatch('[0-9a-f]{40}', source) and source not in (BASE, G3, P4, R3, D0),
            'P5 must be a distinct immutable product commit')
    require(git.run('rev-parse', source + '^{commit}').decode().strip() == source, 'P5 commit differs')
    require(git.run('show', '-s', '--format=%P', source).decode().strip() == G3, 'P5 parent must be exactly G3')
    require(git.run('rev-parse', source + '^{tree}').decode().strip() == expected_tree, 'P5 tree differs')
    current, prior, base = (git.tree(ref) for ref in (source, G3, BASE))
    require(set(current) == set(prior), 'P5 adds or deletes a tree path')
    changed = {p for p in current if current[p] != prior[p]}
    expected_changes = {row['path'] for row in prospective['five_changes']}
    require(changed == expected_changes, 'P5 full tree differs beyond five reviewed inputs')
    for row in prospective['five_changes']:
        require(current[row['path']] == {'mode': row['proposed']['git_mode'], 'kind': 'blob',
                                       'oid': row['proposed']['git_blob']}, 'P5 adopted blob differs')
    source_paths = {p for p in current if p in ('Cargo.toml', 'Cargo.lock') or p.startswith('crates/')}
    validation_paths = {p for p in current if is_validation(p)}
    complete = git.hashes(current, source_paths)
    validation = git.hashes(current, validation_paths)
    require(complete == prospective['complete_inputs'] and len(complete) == 1087, 'P5 complete source map differs')
    require(validation == prospective['validation_inputs'] == old['validation_inputs'] and len(validation) == 136,
            'P5 validation map differs')
    modes = {p: current[p]['mode'] for p in complete}
    vmodes = {p: current[p]['mode'] for p in validation}
    require(modes == prospective['source_modes'] == old['source_modes'], 'P5 source modes differ')
    require(vmodes == prospective['validation_modes'] == old['validation_modes'], 'P5 validation modes differ')
    delta_paths = {p for p in source_paths | {p for p in base if p in ('Cargo.toml', 'Cargo.lock') or p.startswith('crates/')}
                   if current.get(p) != base.get(p)}
    require(delta_paths == set(prospective['paths']) and len(delta_paths) == 26, 'P5 historical delta paths differ')
    base_hashes = git.hashes(base, delta_paths & set(base))
    delta = {p: {'before_sha256': base_hashes.get(p), 'sha256': complete[p]} for p in sorted(delta_paths)}
    require(delta == prospective['paths'], 'P5 exact BASE-to-product before/after pairs differ')
    require(sum(complete[p] == old['complete_inputs'][p] for p in complete) == 1082, 'P5 inherited source count differs')
    require(sum(delta.get(p) == pair for p, pair in old['paths'].items()) == 21, 'P5 inherited delta count differs')
    for path in FROZEN:
        require(current[path] == base[path], 'historical frozen file changed: ' + path)
    for path in EXCLUSIONS:
        require(current[path] == prior[path], 'guard/registry/original CI changed before R4: ' + path)
    return {'source': source, 'source_tree': expected_tree, 'base': BASE, 'paths': delta,
            'complete_inputs': complete, 'validation_inputs': validation, 'source_modes': modes,
            'validation_modes': vmodes, 'complete_input_count': 1087, 'changed_source_input_count': 26,
            'validation_input_count': 136, 'heldout_corpus_bodies_read': False}


def evidence(manifest_path, archive_path):
    manifest_raw = manifest_path.read_bytes()
    require(sha(manifest_raw) == MANIFEST_SHA, 'fixed engineering manifest differs')
    manifest = json.loads(manifest_raw)
    archive_raw = archive_path.read_bytes()
    require(sha(archive_raw) == ARCHIVE_SHA == manifest['archive_sha256']
            and len(archive_raw) == manifest['archive_bytes'] == 2613992, 'fixed engineering archive differs')
    require(manifest['public_directory'] == PUBLIC_DIRECTORY and manifest['measured_source'] == D0,
            'archive publication or measurement identity differs')
    members = {}
    total = 0
    with tarfile.open(archive_path, 'r:gz') as archive:
        for item in archive:
            relative = PurePosixPath(item.name)
            require(item.isfile() and not item.issym() and not item.islnk() and not relative.is_absolute()
                    and '..' not in relative.parts and item.name not in members, 'unsafe archive member')
            require(item.name in manifest['files'], 'unexpected archive member')
            original = archive.extractfile(item).read()
            expected = manifest['files'][item.name]
            require(len(original) == item.size == expected['bytes'] and item.mode == expected['mode']
                    and sha(original) == expected['sha256'], 'archived original bytes/mode changed')
            members[item.name] = original
            total += len(original)
    require(set(members) == set(manifest['files']) and len(members) == manifest['file_count'] == 83
            and total == manifest['original_bytes'] == 4662761, 'incomplete engineering archive')
    require(set(EVIDENCE_MEMBERS) == set(EVIDENCE_PINS), 'public review evidence roles differ')
    result = {}
    loaded = {}
    for role, pin in EVIDENCE_PINS.items():
        require(pin is not None, 'actual independent diagnostic review is not pinned: ' + role)
        member = EVIDENCE_MEMBERS[role]
        require(sha(members[member]) == pin, 'review evidence pin differs: ' + role)
        loaded[role] = json.loads(members[member])
        result[role] = {'archive': PUBLIC_DIRECTORY + '/' + manifest['archive'],
                        'member': member, 'sha256': pin}
    controls = loaded['D0_controls_reception']
    require(controls['source_commit'] == D0 and controls['source_before_after_equal'] is True
            and controls['passed_executions'] == 49 and controls['unique_passed_cases'] == 48
            and controls['failed_executions'] == 0 and controls['ignored_executions'] == 1
            and len(controls['results']) == 12 and all(r['exit_code'] == 0 for r in controls['results']),
            'actual D0 engineering controls differ')
    for role, scale, artifact, samples in [('D0_1k_diagnostic_reception', 1000, 11582323631, 14),
                                           ('D0_10k_diagnostic_reception', 10000, 11582922326, 9)]:
        review = loaded[role]
        require(review['schema'] == 'p8-D0-original-diagnostic-independent-validation-v1'
                and review['status'] == 'accepted_scoped_one_original_engineering_diagnostic'
                and review['reviewer'] == '/root/acceptance_audit'
                and review['source'] == D0 and review['scale'] == scale and review['artifact_id'] == artifact
                and review['sample_count'] == samples and review['native_exit'] == review['native_worker_exit'] == 0
                and review['native_status'] == 'measurement_complete'
                and review['original_validate_build_passed'] is True and review['original_validate_shard_passed'] is True
                and review['source_driver_ZIP_shard_and_build_all_unchanged_after_validation'] is True
                and review['fixture_cleanup']['error'] is None
                and review['plan']['repetitions'] == 30 and review['plan']['shard'] == {'index': 0, 'count': 30}
                and review['plan']['files'] == [scale] and review['plan']['dirty_budget'] == 200
                and review['plan']['max_resume_builds'] == 1024 and review['plan']['deadline_ms'] == 18000000
                and review['plan']['max_output_bytes'] == 536870912, 'original diagnostic scope differs')
        require(sha(members['original-upstream-zips/' + str(artifact) + '.zip']) == review['zip_sha256'],
                'original diagnostic ZIP differs from independent validation')
    archive_record = {'path': PUBLIC_DIRECTORY + '/' + manifest['archive'],
                      'bytes': len(archive_raw), 'sha256': sha(archive_raw),
                      'manifest_path': PUBLIC_DIRECTORY + '/D0-engineering-manifest.json',
                      'manifest_sha256': sha(manifest_raw), 'original_files': len(members),
                      'original_bytes': total, 'all_members_independently_rehashed': True,
                      'binary_reference': manifest['binary_reference']}
    return result, archive_record


def main():
    require(not sys.flags.optimize, 'review finalization requires Python without -O')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, required=True)
    parser.add_argument('--source', required=True)
    parser.add_argument('--tree', required=True)
    parser.add_argument('--evidence-manifest', type=Path, required=True)
    parser.add_argument('--archive', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    # Fail before producing a review while either original diagnostic remains unreviewed.
    require(all(EVIDENCE_PINS.values()), 'D0 actual diagnostic reviews remain unpinned; no accepted review can be produced')
    git = Git(args.repo)
    old_raw = git.run('show', R3 + ':' + OLD_REVIEW_PATH)
    require(sha(old_raw) == OLD_REVIEW_SHA, 'inherited public R3 review differs')
    old = json.loads(old_raw)
    require(old['source'] == P4 and old['verdict'] == 'accepted_scoped', 'inherited review scope differs')
    prospective = checked(HERE / 'prospective-inputs.json', PROSPECTIVE_SHA)
    current = intake(git, args.source, args.tree, prospective, old)
    publications, archive_record = evidence(args.evidence_manifest, args.archive)
    review = dict(current)
    review.update(
        schema_version=1, verdict='accepted_scoped', scope='independently_reviewed_source_and_validation_inputs',
        independent_reviewers=['/root/pr_audit'], unresolved_blockers=[],
        prior_accepted_review={'source': P4, 'review_commit': R3, 'path': OLD_REVIEW_PATH,
                              'sha256': OLD_REVIEW_SHA, 'complete_source_inputs_unchanged': False,
                              'source_inputs_unchanged': 1082, 'source_delta_pairs_unchanged': 21,
                              'validation_inputs_unchanged': 136,
                              'scope': 'Only exact unchanged input bytes/modes and21 historical delta pairs inherit prior acceptance'},
        author_independence={
            '/root/pr_audit': 'Non-author of all five adopted product/test blobs. Independently reviewed fixed prefix-window, doc-key index/schema tests and oracle batches, and reconstructed actual whole P5 tree and complete source/validation inventories. Authorship of this review generator does not imply authorship of product changes.',
            '/root/scale_engineering': 'Author of prefix-window and oracle batch changes; engineering implementation and reception evidence are identified without self-approval.',
            'peer fixed1b9e846db340cf9a5297ef44ab548a0816712029': 'Author source for the three adopted doc-key index/schema test blobs. Their exact bytes and parent compatibility were independently reviewed.',
            'inherited R3 contributors': 'Historical source/validation author and reviewer boundaries remain as recorded in immutable R3; unchanged136 validation inputs do not receive invented fresh execution.'},
        validation_delta_from_prior={}, review_evidence=publications, engineering_archive=archive_record,
        findings=[
            'The complete actual P5 tree differs from G3 in exactly five fixed reviewed crate files, with no path or mode additions/deletions. All1087 production inputs and136 validation inputs are enumerated;1082 source inputs,21 old BASE-to-product delta pairs and all136 validation inputs retain exact prior accepted bytes. Oracle streaming updates one prior pair and four other paths add new historical pairs, for26 total.',
            'Within one prepare, the reconciliation planner reuses the materialized initial B+1 dependency window after at most B newly completed promotions. Overflow of that proof condition falls back to the original SQL. Events and database have not been written by that prepare; no cross-prepare cache, held lease or transaction is added. Replacement visibility, pool1 and stale prepared-epoch rejection remain intact. This removes only a redundant tail lookup and does not claim all quadratic scan costs are removed.',
            'The nonunique chunk_symbol_identity(doc_key) index is present in fresh schema25 and idempotent current-schema maintenance. The original version mismatch, schema24 rebuild, nonadjacent version and read-only contracts remain. Logical rows, generation and epoch rules are unchanged; actual SQLite index/cascade controls retain the original four schema assertions and add the new current-schema case.',
            'The oracle spool batches at64/8/1 rows with at most64KiB pending serialized data. A legal larger row flushes pending data and uses the original single-row insert. All side/value/checked-i64-ordinal bindings, duplicate multiplicity, original per-row admission checks,15-table projection, typed Value comparison, canonical/scratch limits and named profile boundaries are retained.',
            'The actual D0 engineering run completed12 original commands with all exit0:49 passed test executions represent48 unique cases because the49-file case runs twice; one original optional actual-MCP case remains ignored. Source snapshots match before/after. Six oracle units, eight original oracle integration cases and the full twelve-case P8 scale target are included. These are actual D0 engineering controls, not complete P5 CI or a150-shard matrix.',
            'The independently received original D0 release build and two diagnostic shards retain their D0 source, binary, driver, raw and environment identities. They support this exact-input implementation review, while future all150 results must remain a separately registered D0 study and cannot be renamed P5 measurements.',
            'All original workflows, original CI body/selector, six frozen v14 files, v15 guard logic and existing validation inputs are unchanged. The separate150-cell controller/registration/engineering workflow is not adopted into P5. Runtime, recovery, platform, scale, quality and task acceptance remain separate execution obligations.'
        ],
        limitations=[
            'This accepts the fixed source and validation input set only; it closes no TODO and grants no whole P5 CI, formal performance matrix, quality or release approval.',
            'D0 and P5 have equal1087 production input bytes and equal original scale driver/helper bytes, but different Git identities. Input equality is not a claim of identical newly compiled P5 binary bytes, or a substitution for final P5/G4 source guards.',
            'Original G matrix observations, original failed local G primary cell7, prior diagnostic attempts and failed builds retain their original identities and outcomes. No best-of replacement or retrospective relabeling is authorized.',
            'Deferred batch SQL can change which error appears first when competing independent faults coexist. Original source budget-check ordering and fail-closed scratch discard are retained; no physical rollback guarantee for the original private journal-OFF scratch is invented.',
            'No same-host causal speedup, stable tail latency,100k deadline feasibility or full150 completion follows from these controls and two diagnostic shards.',
            'Previously accepted cache/observer logic is byte-identical, but no earlier G/G2/G3 execution is silently relabeled as P5. The new source requires its appropriate original execution gates.',
            'Only subsequent PRODUCT/REVIEW/REGISTRY_SHA256 pin updates may install this R4 after its own immutable commit; full original source proof and historical gates must actually execute on the final G4.'
        ])
    require(not args.output.exists() and not args.output.is_symlink(), 'refusing to overwrite review output')
    raw = encode(review)
    with args.output.open('xb') as stream:
        stream.write(raw)
    print(json.dumps({'source': args.source, 'source_tree': args.tree, 'sha256': sha(raw),
                      'verdict': review['verdict'], 'complete_inputs': 1087, 'delta': 26,
                      'validation_inputs': 136, 'TODO_closed': 0}))


if __name__ == '__main__':
    main()
