#!/usr/bin/env python3
"""Bind non-author cache observer review to the immutable P4 Git source."""
import hashlib
import json
from pathlib import Path

from import_published_commit import git

HERE = Path(__file__).resolve().parent
P3 = 'b11addeed93b02c7b840bd3b61a452ce0e671287'
R2 = '64ea77a36b6fbb9b755a2e24e5ff9e7cd678b1de'
G2 = 'd2d492b329a5630bf4a5fa6bc4b6eac30d07b13c'
P4 = 'f5518ca6d21f54cfd324e33edc3b739608ae28b8'
BASE = '7354db236c9d9850a75f31672697ae9eab44565e'
REVIEW_PATH = 'artifacts/checkpoints/p8-completion-20261009/independent-source-review.json'
ARCHIVE_PATH = 'artifacts/checkpoints/p8-completion-20261009/cache-engineering'
OLD_SHA = '9e19bdf897af21fef00a78e3291921aa445d68f77b7115a44ff0f719580785e9'

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def checked(path, digest):
    raw = Path(path).read_bytes()
    assert sha(raw) == digest, path
    return json.loads(raw)

def main():
    old_raw = git('show', R2 + ':' + REVIEW_PATH)
    assert sha(old_raw) == OLD_SHA
    old = json.loads(old_raw)
    assert old['source'] == P3 and old['verdict'] == 'accepted_scoped'
    current = json.loads((HERE / 'intake-P4.json').read_text())
    assert current['source'] == P4 and current['base'] == BASE
    assert current['source_tree'] == '3237c7fedfe9b3f3966cf675e1e1bde232405f72'
    for key in ('complete_inputs', 'paths', 'source_modes'):
        assert current[key] == old[key], key
    assert current['complete_input_count'] == 1087
    assert current['changed_source_input_count'] == 22
    assert current['validation_input_count'] == 136
    added = set(current['validation_inputs']) - set(old['validation_inputs'])
    assert added == {'scripts/tests/test_p8_runtime_cache.py'}
    assert set(old['validation_inputs']) <= set(current['validation_inputs'])
    changed = {p for p in old['validation_inputs'] if current['validation_inputs'][p] != old['validation_inputs'][p]}
    assert changed == {'scripts/p8_runtime.py'}
    assert all(current['validation_modes'][p] == old['validation_modes'][p] for p in old['validation_modes'])
    assert current['validation_modes']['scripts/tests/test_p8_runtime_cache.py'] == '100644'
    assert set(git('diff', '--name-only', G2, P4).decode().splitlines()) == changed | added
    for path in ('scripts/verify_reviewed_source_v15.py', 'scripts/reviewed-source-registry-v15.json', '.github/workflows/ci.yml'):
        assert git('show', P4 + ':' + path) == git('show', G2 + ':' + path), path
    frozen = ('scripts/verify_reviewed_source_v14.py', 'scripts/reviewed-source-registry-v14.json',
              'tests/source_integrity/test_reviewed_source_v14.py', 'scripts/v14_historical_context.py',
              'tests/source_integrity/v14_historical_test_adapter.py', 'tests/source_integrity/test_v14_historical_context.py')
    for path in frozen:
        assert git('show', P4 + ':' + path) == git('show', BASE + ':' + path), path
    semantic = checked('/dev/shm/p8-soak-cache-independent-review/final-review.json', '0614a66b096b4726a544a3fd3093cc196a8d5a799f13e5166f58bd0a3594b6f9')
    assert semantic['author_of_delta'] is False and semantic['blockers'] == []
    assert semantic['conclusion'] == 'accepted_scoped_static_and_protocol_controls_pending_actual_product_execution'
    assert semantic['base_source'] == G2
    for path, item in semantic['fixed_files'].items():
        assert current['validation_inputs'][path] == item['sha256']
        assert git('rev-parse', P4 + ':' + path).decode().strip() == item['git_blob']
    controls = checked('/dev/shm/p8-soak-cache-candidate-G2/validation-v3-receipt.json', '2110a73e5f584dec4d16bb6425567d7170c7292de97c640eddb68e92e84f67cd')
    assert controls['exit_code'] == 0 and controls['source_before'] == controls['source_after']
    for path, digest in controls['source_before'].items():
        assert current['validation_inputs'][path] == digest
    probe = checked('/dev/shm/p8-soak-cache-real-probe-root/run-01/receipt.json', '334cb82e9e498ccb518eef3519dbba502269c91619bfcc5dee03902203fe720a')
    assert probe['status'] == 'passed_small_actual_product_probe' and probe['exit_code'] == 0 and probe['errors'] == []
    assert probe['outcomes'] == {'compound_reads': 14, 'successful_reads': 14, 'incremental_mutations': 6, 'actual_read_rpc_requests': 56}
    assert probe['cache_states'] == ['miss', 'hit'] * 6 + ['hit', 'hit']
    assert probe['plan']['formal_soak_entrypoint_called'] is False
    assert probe['observer_unchanged'] is True and probe['cleanup']['exit_code'] == 0
    checked('/dev/shm/p8-soak-cache-real-probe-independent/audit.json', 'd7e0308e7104e04f30f3fca46f80da162768b09268b1ebf7c02ead4e5032e899')
    manifest_raw = (HERE / 'cache-engineering-manifest.json').read_bytes()
    manifest = json.loads(manifest_raw)
    archive = (HERE / manifest['archive']).read_bytes()
    assert len(archive) == manifest['archive_bytes'] and sha(archive) == manifest['archive_sha256']
    assert manifest['all_members_actual_roundtrip_verified'] is True
    review = {key: current[key] for key in ('source', 'source_tree', 'base', 'paths', 'complete_inputs', 'validation_inputs',
              'source_modes', 'validation_modes', 'complete_input_count', 'changed_source_input_count', 'validation_input_count', 'heldout_corpus_bodies_read')}
    review.update(
        schema_version=1, verdict='accepted_scoped', scope='independently_reviewed_source_and_validation_inputs',
        independent_reviewers=['/root', '/root/pr_audit'], unresolved_blockers=[],
        prior_accepted_review={'source': P3, 'review_commit': R2, 'path': REVIEW_PATH, 'sha256': OLD_SHA,
                              'complete_source_inputs_unchanged': True, 'validation_inputs_unchanged': 134,
                              'scope': 'Prior independent acceptance is inherited for exact byte-identical inputs only'},
        author_independence={
            '/root/acceptance_audit': 'Author of the new cache observer and its tests; author controls are recorded without self-approval.',
            '/root/pr_audit': 'Non-author of this cache delta; independently inspected code and original product protocol, executed 39 v2 controls, verified v3 is only three additive timeout metadata lines, and independently replayed the real small-probe wire records.',
            '/root': 'Non-author of this cache delta; inspected the changes and retained controls, executed the real G-binary/v3-observer probe, mechanically checked the complete P4 Git inventory, and coordinated source approval. Probe authorship is distinguished from observer authorship.'},
        validation_delta_from_prior={p: {'before_sha256': old['validation_inputs'].get(p), 'sha256': current['validation_inputs'][p]}
                                     for p in sorted(changed | added)},
        findings=[
            'All 1087 crate/Cargo inputs, their modes, and the 22 BASE-to-product source deltas equal the prior independently accepted P3/R2. Exactly one validation input changes and one is added; 134 prior validation inputs remain byte-identical. Original CI, all six frozen v14 paths, and every Rust source file are unchanged.',
            'Soak compound reads retain before-status, original symbol query, local hybrid query, and after-status RPCs. The original 3601-operation/2400-read denominator is unchanged; 9600 workload RPCs are declared. All RPCs, validation, and shared admission-lock wait remain in the original read timing. Mixed workload behavior is unchanged.',
            'Actual native PID, complete generation, exact cache counter deltas, current stable.py source bytes/spans/entity and pool completion are checked. A changed generation requires a cache miss; a repeated generation requires a hit, including an epoch-preserving restore. All-zero cache counters cannot pass.',
            'Quarter coverage uses actual hybrid and successful mutation completion times. Independent counterexamples exposed an incorrect status structure and offered-time coverage; both were fixed and all failed attempts are retained. Four execution quarters each require observed hits, misses and invalidations.',
            'Original one-hour duration, operation/sample counts, 1Hz resource sampling, RSS gate, compaction and branch cycle, 512MiB raw budget, worker cleanup bounds, terminal outcome rules, and endpoint full-build 15-table oracle remain unchanged. The original Rust replayer validates original outcomes and symbol responses; the additive cache evidence is validated by Python.',
            'Author v3 protocol controls actually passed 39 tests, with unchanged before/after source. The non-author independently executed 39 v2 controls and verified v3 changes only three timeout metadata lines. These are not described as one combined actual full P4 CI execution.',
            'The separately identified real engineering probe used original G artifact 11573313154 product bytes with SHA256 9ebbd85103ae3d4e88fc13d2fcf6144438a0dfbcfba44cbe6a4e0232bf8a8f7e and exact v3 observer bytes: one full build, six real mutations/incremental builds, 14 compound reads and 56 read RPCs, five generation invalidations, same native PID, and clean EOF/exit0. The non-author independently matched all request/response/wire rows and all 58 sealed file hashes. It is a 0.648-second sequential engineering probe, not a shortened formal soak.',
            'The engineering archive preserves 167 files and 5533275 original bytes, including prior failed and superseded attempts, before/after source records, raw small-probe requests/responses and process records. Every archived member was actually roundtrip-verified. The original 27MB product binary remains referenced by its immutable Actions artifact and member/hash rather than duplicated here.'
        ],
        review_evidence={
            'non_author_cache_review': {'archive': ARCHIVE_PATH + '/cache-engineering-originals.tar.gz', 'member': 'independent-review/final-review.json', 'sha256': '0614a66b096b4726a544a3fd3093cc196a8d5a799f13e5166f58bd0a3594b6f9'},
            'author_v3_controls': {'archive': ARCHIVE_PATH + '/cache-engineering-originals.tar.gz', 'member': 'candidate/validation-v3-receipt.json', 'sha256': '2110a73e5f584dec4d16bb6425567d7170c7292de97c640eddb68e92e84f67cd'},
            'actual_small_probe': {'archive': ARCHIVE_PATH + '/cache-engineering-originals.tar.gz', 'member': 'actual-small-probe/run-01/receipt.json', 'sha256': '334cb82e9e498ccb518eef3519dbba502269c91619bfcc5dee03902203fe720a'},
            'non_author_small_probe_review': {'path': ARCHIVE_PATH + '/actual-small-probe-independent-audit.json', 'sha256': 'd7e0308e7104e04f30f3fca46f80da162768b09268b1ebf7c02ead4e5032e899'}},
        engineering_archive={'path': ARCHIVE_PATH + '/' + manifest['archive'], 'bytes': len(archive), 'sha256': sha(archive),
                             'manifest_path': ARCHIVE_PATH + '/cache-engineering-manifest.json', 'manifest_sha256': sha(manifest_raw),
                             'original_files': manifest['file_count'], 'original_bytes': manifest['original_bytes']},
        limitations=[
            'This source approval closes no TODO and supplies no full P4 CI, one-hour cache, scale, quality or release certification.',
            'The real small probe retains distinct G product and v3 observer identities. No G or G2 execution is relabeled as a P4/G3 execution. Original G symbol-only soak cache counters were zero; final cache acceptance requires a new original-duration successful run.',
            'The status/symbol/hybrid/status per-RPC timeouts are 30/60/60/30 seconds. Their sequential sum is 180 seconds excluding admission wait; this is not a whole-operation 60-second guarantee or a new SLA.',
            'Shared query pool counters and the same native process do not prove individual OS-thread or semantic-worker identity. Semantic providers remain disabled for this workload; no paid or heldout data is used.',
            'Existing original G scale observations may remain under their true G identity only after separately checking all executed input invariance and every original shard. New runtime observer timings are not directly comparable with symbol-only soak timings.',
            'Product/review/registry pins must be installed in a subsequent exact-diff-reviewed commit. Original source proof behavior, frozen historical gates, CI and acceptance thresholds remain unchanged.'
        ])
    raw = (json.dumps(review, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()
    with (HERE / 'independent-source-review-P4.json').open('xb') as stream:
        stream.write(raw)
    print(json.dumps({'source': P4, 'sha256': sha(raw), 'complete_inputs': 1087, 'validation_inputs': 136,
                      'verdict': review['verdict'], 'todo_closed': 0}))

if __name__ == '__main__':
    main()
