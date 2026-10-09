#!/usr/bin/env python3
"""Read-only, fixed-object review of the Round10 three-file candidate."""
from pathlib import Path
from datetime import datetime, timezone
import hashlib
import json
import os
import re
import subprocess

ROOT = Path('/workspace/scratch/50c364fd60b1')
REPO = ROOT / 'codecortex'
DIR = ROOT / 'validation/lane-ownership-round10'
OUT = DIR / 'reviewer'
M = 'b9412406e11422d7cf914458a8bfbbd58cf94eaa'
P5 = 'fa6562ad70c6a6f2a7a1e65594df23f807e62f18'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0', GIT_TERMINAL_PROMPT='0')

def git(*args):
    return subprocess.check_output(['git', '-C', str(REPO), *args], env=ENV)

def sha(data):
    return hashlib.sha256(data).hexdigest()

def oid(data):
    return hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest()

def record(path):
    b = path.read_bytes()
    return dict(path=str(path.relative_to(ROOT)), bytes=len(b), sha256=sha(b))

def inventory(source):
    result = {}
    for row in git('ls-tree', '-r', '-z', source).split(b'\0'):
        if row:
            meta, path = row.split(b'\t', 1)
            mode, typ, obj = meta.decode().split()
            result[path.decode()] = dict(mode=mode, type=typ, blob=obj)
    return result

manifest = json.loads((DIR / 'changed-files-v2.json').read_text())
assert sha((DIR / 'changed-files-v2.json').read_bytes()) == '9c216e17b7d3f5586d692e5e12f4be367f402e7bd7d2c6be71ae1f22cd820db9'
assert sha((DIR / 'lane-ownership-v2.patch').read_bytes()) == 'adc12ac8b13558b99b90c180abc463df5a8177e9f4b26e47fe854873b174c10b'
assert manifest['base_source'] == M
assert git('show', '-s', '--format=%T%n%P', P5).decode().splitlines() == ['0e6fdcfd2ac7a727184daf5a0e290cf00c55abbc', M]
mt, pt = inventory(M), inventory(P5)
files = [x['path'] for x in manifest['changed_files']]
assert sorted(k for k in mt.keys() | pt.keys() if mt.get(k) != pt.get(k)) == sorted(files)
old, new = {}, {}
file_receipts = []
for item in manifest['changed_files']:
    p = item['path']
    a = git('show', M + ':' + p)
    b = git('show', P5 + ':' + p)
    assert a == (DIR / item['original_relative_path']).read_bytes()
    assert b == (DIR / item['candidate_relative_path']).read_bytes()
    assert (len(a), sha(a), oid(a)) == (item['before_bytes'], item['before_sha256'], item['before_git_blob'])
    assert (len(b), sha(b), oid(b)) == (item['after_bytes'], item['after_sha256'], item['after_git_blob'])
    assert mt[p]['mode'] == pt[p]['mode'] == '100644'
    old[p], new[p] = a.decode(), b.decode()
    file_receipts.append(dict(path=p, before=mt[p] | dict(bytes=len(a), sha256=sha(a)), after=pt[p] | dict(bytes=len(b), sha256=sha(b))))

lanes = 'crates/cc-search/src/lanes.rs'
policy = 'crates/cc-search/src/query_policy.rs'
control = 'crates/cc-eval/src/benchmark/ablation/mechanism_controls.json'
oldblock = '''/// Registry order is execution, fusion-bill, annotation, and tie-break order.
pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    vec![
        &ExactSymbolLane,
        &PathLane,
        &LexicalLane,
        &GrepLane,
        &GraphLane,
    ]
}'''
newblock = '''/// Shared registration order for execution, policy obligations and fusion.
/// The lanes are stateless; inspecting this view allocates nothing and runs no lane.
pub(crate) fn default_lane_registry() -> &'static [&'static dyn RetrievalLane] {
    &[
        &ExactSymbolLane,
        &PathLane,
        &LexicalLane,
        &GrepLane,
        &GraphLane,
    ]
}

/// Keep the owned execution-list interface backed by the same policy registry.
pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    default_lane_registry().to_vec()
}'''
assert old[lanes].count(oldblock) == 1
assert old[lanes].replace(oldblock, newblock, 1) == new[lanes]
assert new[lanes] == (DIR / 'candidate' / lanes).read_text()
oldloop = '        for lane_id in ["exact_symbol", "path", "lexical", "grep", "graph"] {'
newloop = '        for lane in crate::lanes::default_lane_registry() {\n            let lane_id = lane.lane_id();'
assert old[policy].count(oldloop) == 1
expected = old[policy].replace(oldloop, newloop, 1)
assert expected.endswith('}\n')
assert new[policy].startswith(expected[:-2])
assert new[policy] == (DIR / 'candidate' / policy).read_text()
assert new[policy].split('#[cfg(test)]')[0] == expected.split('#[cfg(test)]')[0]
old_tests = re.findall(r'#\[test\]\s*fn (\w+)', old[policy])
new_tests = re.findall(r'#\[test\]\s*fn (\w+)', new[policy])
assert new_tests == old_tests + ['obligations_follow_execution_order_with_intent_roles_and_budget_caps', 'registry_cleanup_preserves_policy_wire_and_fingerprint']

oc, nc = json.loads(old[control]), json.loads(new[control])
assert len(oc) == len(nc) == 2
assert oc[1] == nc[1]
assert {k: v for k, v in oc[0].items() if k not in ('on_text', 'off_text')} == {k: v for k, v in nc[0].items() if k not in ('on_text', 'off_text')}
restored_control = new[control]
for field in ('on_text', 'off_text'):
    restored_control = restored_control.replace(json.dumps(nc[0][field]), json.dumps(oc[0][field]), 1)
assert restored_control == old[control]
control_receipts = []
for c in nc:
    body = new[c['path']]
    assert body.count(c['on_text']) == 1 and body.count(c['off_text']) == 0
    replaced = body.replace(c['on_text'], c['off_text'], 1)
    assert replaced.count(c['off_text']) == 1
    if c['id'] == 'local_retrieval':
        registry_only = newblock.split('\n\n/// Keep')[0]
        assert registry_only in replaced
        assert 'default_lane_registry().to_vec();\n    lanes.clear();\n    lanes' in c['off_text']
        assert oc[0]['on_text'] not in new[lanes]
    control_receipts.append(dict(id=c['id'], path=c['path'], on_occurrences=1, off_before=0, off_after_replacement=1, counterfactual_source_sha256=sha(replaced.encode()), execution=False))

ids = ['exact_symbol', 'path', 'lexical', 'grep', 'graph']
roles = ['supporting', 'supporting', 'source', 'source', 'structural']
field_order = re.findall(r'^    pub (\w+):', re.search(r'pub struct QueryPolicy \{(.*?)\n\}', old[policy], re.S)[1], re.M)
def source_const(name):
    return re.search(r'pub const ' + name + r': &str =\s*"([^"]+)";', old[policy])[1]
wire = []
for name, strategy, state in [('LOCAL', 'local', 'disabled'), ('AUTO', 'auto', 'configured')]:
    block = re.search(r'const ' + name + r': &str = concat!\((.*?)\n        \);', new[policy], re.S)[1]
    value = ''.join(re.findall(r'r#"(.*?)"#', block, re.S))
    expected_wire = dict(version=source_const('POLICY_VERSION'), graph_source_mapping=source_const('GRAPH_SOURCE_MAPPING'), path_source_domain=source_const('PATH_SOURCE_DOMAIN'), requested=strategy, effective=strategy, intent='trace', deadline_ms=100, lane_timeout_ms=100, semantic_timeout_ms=40, semantic_top_k=7, semantic_state=state, obligations=[dict(lane_id=i, role=r, timeout_ms=100) for i, r in zip(ids, roles)], completeness='all_executed_lane_limits_remain_visible; obligation_role_does_not_erase_partial', no_answer='empty_partial_is_not_absence; no_semantic_absence_claim_from_weak_rank')
    if name == 'AUTO':
        expected_wire['obligations'].append(dict(lane_id='semantic', role='optional_recall', timeout_ms=40))
    assert list(expected_wire) == field_order
    assert json.dumps(expected_wire, separators=(',', ':'), ensure_ascii=False) == value
    wire.append(dict(name=name, bytes=len(value.encode()), sha256=sha(value.encode()), matches_source_derived_prechange_wire=True, Rust_baseline_execution=False))

related_paths = ['crates/cc-search/src/engine.rs', 'crates/cc-search/src/engine_lane_tests.rs', 'crates/cc-search/src/plan.rs', 'crates/cc-search/src/execution.rs', 'crates/cc-search/src/lanes/exact_symbol.rs', 'crates/cc-search/src/lanes/path.rs', 'crates/cc-model/src/query.rs', 'crates/cc-model/src/lib.rs', 'crates/cc-model/src/identity.rs', 'crates/cc-eval/src/benchmark/ablation.rs', 'crates/cc-eval/src/benchmark/ablation/mechanism.rs', 'scripts/p7_mechanism_build.py']
for p in related_paths:
    assert mt[p] == pt[p]

report = dict(
    schema_version=1,
    review_kind='independent_static_semantic_review_of_exact_three_file_v2',
    reviewer='/root/todo_audit', author='/root/pr_audit', reviewed_at_utc=datetime.now(timezone.utc).isoformat(),
    verdict='accepted_scoped', unresolved_blockers=[], source=P5, tree=git('rev-parse', P5 + '^{tree}').decode().strip(), source_parent=M,
    candidate_manifest=record(DIR / 'changed-files-v2.json'), candidate_patch=record(DIR / 'lane-ownership-v2.patch'),
    changed_files=file_receipts,
    independently_verified=dict(actual_P5_equals_candidate_v2=True, exactly_three_paths_changed_from_M=True, all_other_tracked_mode_type_blob_entries_unchanged=True, Rust_v2_equals_preserved_v1=True, original_tests_retained_byte_for_byte=True, controls_only_two_local_text_fields_changed=True, semantic_control_complete_entry_unchanged=True),
    semantic_findings=[
        dict(id='single_local_catalog', finding='The five existing stateless lane objects remain in exact symbol/path/lexical/grep/graph order. A static slice of Sync trait references is the sole production registration list. default_lanes keeps its crate-private Vec-returning API by cloning references from that slice; execution, fusion ordering, annotation and tie-break consumers remain unchanged.'),
        dict(id='policy_invariants', finding='The sole production policy edit obtains lane IDs from that same static registration view. Each existing lane_id method returns its unchanged static string. Config validation and error order, strategy/semantic availability, all eight intent-role rules, local and semantic caps, optional semantic suffix, wire field order, version, completeness/no-answer text and fingerprint implementation are unchanged. Policy still allocates its original obligations Vec; the view adds no intermediate catalogue Vec, boxed lane, DB/service access or lane execution.'),
        dict(id='necessary_existing_counterfactual_compatibility', finding='The v1 two-file edit no longer matched the original local_retrieval literal anchor. Existing Rust controls and the original prepare script require one exact match. v2 updates only this control on_text/off_text: the off variant clears the owned execution Vec while preserving the static registry and policy obligations. The semantic_dense control, all replacement validators, witness protocol and gate rules are unchanged. This finding was static; no failed Cargo run is asserted.'),
    ],
    meaningful_test_review=dict(
        new_test_names=new_tests[len(old_tests):], retained_test_names=old_tests,
        matrix='Eight intents x seven valid configured/explicit strategy cases x two deadline-cap fixtures = 112 combinations. Checks execution/policy ID order, independent literal roles, caps, semantic state and optional final semantic lane. The preserved older test covers explicit Semantic with an unavailable provider.',
        independent_order_oracle='The unchanged engine default_lanes_registry_keeps_fusion_order test pins the five literal lane constants; the new cross-view comparison does not stand alone.',
        wire_fixtures=wire,
        fingerprint_boundary='Complete fixed wire strings are compared before hashing. They pin policy inputs to the existing bytes_hash function, not an independently certified hash algorithm. The fixtures were independently compared with the prechange source contract, not labelled as a newly executed prechange capture.',
        test_deletions_or_weakened_assertions=False,
    ),
    original_control_text_replacements=control_receipts,
    unchanged_related_mode_blob_entries={p: pt[p] for p in related_paths},
    actual_execution_boundary=dict(reviewer_ran_Cargo_rustfmt_product_tests_or_original_prepare_script=False, reviewer_ran_source_guard=False, reviewer_network_fetch_or_git_object_index_ref_worktree_mutation=False, actual_P5_root_engineering='Separate original receipt and logs under validation/lane-ownership-round10/engineering-checks; not a result of this static review.', compiled_counterfactual_acceptance=False, performance_or_provider_or_study_acceptance=False),
    task_boundary=dict(task='P8-017', status_and_dependency_unchanged=True, dependency='P8-016', TODO_total=192, TODO_done=163, TODO_remaining=29, TODO_closed=0),
    reproduction=record(Path(__file__)),
)
out = OUT / 'semantic-independent-review-v2.json'
out.write_text(json.dumps(report, indent=2, sort_keys=True, ensure_ascii=False) + '\n')
print(json.dumps(record(out) | dict(verdict=report['verdict'], source=P5, changed_files=len(files))))
