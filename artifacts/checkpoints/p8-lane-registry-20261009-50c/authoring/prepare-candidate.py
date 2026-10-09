#!/usr/bin/env python3
"""Prepare two isolated Rust files; never edit the shared repository."""
from pathlib import Path
import datetime
import difflib
import hashlib
import json
import os
import subprocess
import tomllib

D = Path(__file__).resolve().parent
REPO = D.parents[1] / 'codecortex'
ENV = dict(os.environ, GIT_NO_LAZY_FETCH='1', GIT_OPTIONAL_LOCKS='0')
P4 = '31a42daeb12da6936695abb08eb3912d4f3c6064'
MAIN = 'b9412406e11422d7cf914458a8bfbbd58cf94eaa'
PATHS = ('crates/cc-search/src/lanes.rs', 'crates/cc-search/src/query_policy.rs')


def git(*args):
    return subprocess.run(['git', *args], cwd=REPO, env=ENV, check=True, capture_output=True).stdout


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def blob(raw):
    return hashlib.sha1(b'blob ' + str(len(raw)).encode() + b'\0' + raw).hexdigest()


original = {p: git('show', P4 + ':' + p) for p in PATHS}
for p, raw in original.items():
    assert git('rev-parse', P4 + ':' + p).decode().strip() == blob(raw)
    assert git('show', MAIN + ':' + p) == raw
    target = D / 'original' / p
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        assert target.read_bytes() == raw
    else:
        target.write_bytes(raw)

old_registry = '''/// Registry order is execution, fusion-bill, annotation, and tie-break order.
pub(crate) fn default_lanes() -> Vec<&'static dyn RetrievalLane> {
    vec![
        &ExactSymbolLane,
        &PathLane,
        &LexicalLane,
        &GrepLane,
        &GraphLane,
    ]
}
'''
new_registry = '''/// Shared registration order for execution, policy obligations and fusion.
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
}
'''
lanes = original[PATHS[0]].decode()
assert lanes.count(old_registry) == 1
lanes = lanes.replace(old_registry, new_registry)
policy = original[PATHS[1]].decode()
old_iteration = '        for lane_id in ["exact_symbol", "path", "lexical", "grep", "graph"] {\n'
new_iteration = '        for lane in crate::lanes::default_lane_registry() {\n            let lane_id = lane.lane_id();\n'
assert policy.count(old_iteration) == 1
policy = policy.replace(old_iteration, new_iteration)

tests = r'''

    #[test]
    fn obligations_follow_execution_order_with_intent_roles_and_budget_caps() {
        use RetrievalStrategy::{Auto, Local, Semantic};

        let execution_lanes = crate::lanes::default_lanes();
        let execution_ids: Vec<_> = execution_lanes.iter().map(|lane| lane.lane_id()).collect();
        // These roles are the published intent contract, in the existing
        // execution/fusion order guarded by the engine registry test.
        let intent_cases = [
            (Intent::Locate, ["identity", "identity", "source", "source", "supporting"]),
            (Intent::Fix, ["supporting", "supporting", "source", "source", "structural"]),
            (Intent::Refactor, ["supporting", "supporting", "source", "source", "structural"]),
            (Intent::Trace, ["supporting", "supporting", "source", "source", "structural"]),
            (Intent::Test, ["supporting", "supporting", "source", "source", "structural"]),
            (Intent::Patch, ["supporting", "supporting", "source", "source", "supporting"]),
            (Intent::Explain, ["supporting", "supporting", "source", "source", "supporting"]),
            (Intent::Default, ["supporting", "supporting", "source", "source", "supporting"]),
        ];
        // None exercises the configured strategy; explicit Local must suppress
        // a configured semantic port, while unconfigured Auto stays local.
        let strategy_cases = [
            (None, false, Local, "not_configured", false),
            (None, true, Auto, "configured", true),
            (Some(Local), false, Local, "disabled", false),
            (Some(Local), true, Local, "disabled", false),
            (Some(Auto), false, Local, "not_configured", false),
            (Some(Auto), true, Auto, "configured", true),
            (Some(Semantic), true, Semantic, "configured", true),
        ];
        for (lane_budget, semantic_budget, expected_local_cap, expected_semantic_cap) in
            [(250, 40, 100, 40), (40, 250, 40, 100)]
        {
            let config = QueryConfig {
                strategy: Auto,
                deadline_ms: 100,
                lane_timeout_ms: lane_budget,
                semantic_timeout_ms: semantic_budget,
                semantic_top_k: 7,
            };
            for (intent, expected_roles) in intent_cases {
                for (requested, configured, effective, semantic_state, has_semantic) in strategy_cases {
                    let request = SearchRequest {
                        retrieval_strategy: requested,
                        intent: Some(intent),
                        ..Default::default()
                    };
                    let policy = QueryPolicy::resolve(&config, &request, configured).unwrap();
                    assert_eq!(policy.effective, effective);
                    assert_eq!(policy.semantic_state, semantic_state);
                    assert_eq!(policy.lane_timeout_ms, expected_local_cap);
                    assert_eq!(policy.semantic_timeout_ms, expected_semantic_cap);
                    assert_eq!(
                        policy.obligations.len(),
                        execution_ids.len() + usize::from(has_semantic)
                    );
                    let local = &policy.obligations[..execution_ids.len()];
                    assert_eq!(
                        local.iter().map(|lane| lane.lane_id).collect::<Vec<_>>(),
                        execution_ids
                    );
                    assert_eq!(
                        local.iter().map(|lane| lane.role).collect::<Vec<_>>(),
                        expected_roles
                    );
                    assert!(local.iter().all(|lane| lane.timeout_ms == expected_local_cap));
                    if has_semantic {
                        let semantic = policy.obligations.last().unwrap();
                        assert_eq!(semantic.lane_id, "semantic");
                        assert_eq!(semantic.role, "optional_recall");
                        assert_eq!(semantic.timeout_ms, expected_semantic_cap);
                    }
                }
            }
        }
    }

    #[test]
    fn registry_cleanup_preserves_policy_wire_and_fingerprint() {
        // Frozen complete serialization from the pre-cleanup policy contract.
        // Do not build expected fields or IDs from the new registry: order,
        // names, roles and capped budgets are part of this fingerprint.
        const LOCAL: &str = __LOCAL_WIRE__;
        const AUTO: &str = __AUTO_WIRE__;
        let config = QueryConfig {
            strategy: RetrievalStrategy::Auto,
            deadline_ms: 100,
            lane_timeout_ms: 250,
            semantic_timeout_ms: 40,
            semantic_top_k: 7,
        };
        for (requested, expected) in [(Some(RetrievalStrategy::Local), LOCAL), (None, AUTO)] {
            let request = SearchRequest {
                retrieval_strategy: requested,
                intent: Some(Intent::Trace),
                ..Default::default()
            };
            let policy = QueryPolicy::resolve(&config, &request, true).unwrap();
            assert_eq!(serde_json::to_string(&policy).unwrap(), expected);
            assert_eq!(policy.fingerprint(), cc_model::identity::bytes_hash(expected.as_bytes()));
        }
    }
'''

# Fixture values are independently transcribed from the fixed pre-cleanup
# structure/serde declarations, not generated by the new Rust implementation.
# Their future Rust test execution is owned by root and not claimed here.
def golden_policy(strategy):
    doc = {
        'version': 'query-policy-local-semantic-canonical-path-domain-v4',
        'graph_source_mapping': 'uid_byte_declaration_document; complete_mapping_is_not_whole_symbol_body_coverage',
        'path_source_domain': 'canonical_scoped_existing_path_docs; else_bounded_tokens; not_whole_file_coverage',
        'requested': strategy, 'effective': strategy, 'intent': 'trace', 'deadline_ms': 100,
        'lane_timeout_ms': 100, 'semantic_timeout_ms': 40, 'semantic_top_k': 7,
        'semantic_state': 'disabled' if strategy == 'local' else 'configured',
        'obligations': [
            {'lane_id': 'exact_symbol', 'role': 'supporting', 'timeout_ms': 100},
            {'lane_id': 'path', 'role': 'supporting', 'timeout_ms': 100},
            {'lane_id': 'lexical', 'role': 'source', 'timeout_ms': 100},
            {'lane_id': 'grep', 'role': 'source', 'timeout_ms': 100},
            {'lane_id': 'graph', 'role': 'structural', 'timeout_ms': 100},
        ],
        'completeness': 'all_executed_lane_limits_remain_visible; obligation_role_does_not_erase_partial',
        'no_answer': 'empty_partial_is_not_absence; no_semantic_absence_claim_from_weak_rank',
    }
    if strategy == 'auto':
        doc['obligations'].append({'lane_id': 'semantic', 'role': 'optional_recall', 'timeout_ms': 40})
    return doc


def rust_wire(doc):
    # One literal per top-level field, in the actual old struct/serde order.
    fields = [json.dumps(k) + ':' + json.dumps(v, ensure_ascii=False, separators=(',', ':')) for k, v in doc.items()]
    chunks = ['{' + fields[0] + ','] + [field + ',' for field in fields[1:-1]] + [fields[-1] + '}']
    assert all('"#' not in chunk for chunk in chunks)
    return 'concat!(\n' + ''.join('            r#"' + chunk + '"#,\n' for chunk in chunks) + '        )'


fixtures = {key: golden_policy(key) for key in ('local', 'auto')}
for strategy, doc in fixtures.items():
    tests = tests.replace('__' + strategy.upper() + '_WIRE__', rust_wire(doc))
assert '__LOCAL_WIRE__' not in tests and '__AUTO_WIRE__' not in tests
assert policy.endswith('\n}\n')
policy = policy[:-3] + tests + '\n}\n'
candidate = {PATHS[0]: lanes.encode(), PATHS[1]: policy.encode()}
for path, raw in candidate.items():
    target = D / 'candidate' / path
    target.parent.mkdir(parents=True, exist_ok=True)
    assert not target.exists(), 'Do not silently overwrite a reviewed candidate'
    target.write_bytes(raw)

edition = tomllib.loads(git('show', P4 + ':Cargo.toml').decode())['workspace']['package']['edition']
fmt = REPO.parent / 'rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustfmt'
argv = [str(fmt), '--edition', edition, '--config', 'skip_children=true',
        *[str(D / 'candidate' / p) for p in PATHS]]
started = datetime.datetime.now(datetime.timezone.utc).isoformat()
proc = subprocess.run(argv, capture_output=True)
finished = datetime.datetime.now(datetime.timezone.utc).isoformat()
(D / 'isolated-rustfmt.stdout.log').write_bytes(proc.stdout)
(D / 'isolated-rustfmt.stderr.log').write_bytes(proc.stderr)
assert proc.returncode == 0, proc.stderr.decode()
candidate = {p: (D / 'candidate' / p).read_bytes() for p in PATHS}
patch = ''
for path in PATHS:
    patch += 'diff --git a/' + path + ' b/' + path + '\n'
    patch += ''.join(difflib.unified_diff(original[path].decode().splitlines(True),
                                        candidate[path].decode().splitlines(True),
                                        fromfile='a/' + path, tofile='b/' + path))
(D / 'lane-ownership.patch').write_text(patch)
fixture_report = {
    'source': P4, 'equivalent_main_product_source': MAIN,
    'method': 'Literal old policy/serde contract transcription; no product execution and no derivation from the new registry.',
    'struct_path': PATHS[1], 'struct_original_git_blob': blob(original[PATHS[1]]),
    'hash_contract_path': 'crates/cc-model/src/identity.rs',
    'expected_wires': {k: json.dumps(v, ensure_ascii=False, separators=(',', ':')) for k, v in fixtures.items()},
    'claimed_prechange_product_execution': False,
}
(D / 'wire-fixture-provenance.json').write_text(json.dumps(fixture_report, ensure_ascii=False, sort_keys=True, indent=2) + '\n')
manifest = {
    'schema': 'round10-isolated-lane-owner-candidate-v1',
    'author': '/root/pr_audit', 'base_source': MAIN, 'base_tree': git('rev-parse', MAIN + '^{tree}').decode().strip(),
    'original_product_read_source': P4, 'base_input_bytes_equal_P4': True,
    'created_at_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
    'scope': 'P8-017 single local lane registration owner; no task-state or source-admission changes.',
    'changed_files': [{
        'path': p, 'mode': '100644', 'before_bytes': len(original[p]), 'after_bytes': len(candidate[p]),
        'before_git_blob': blob(original[p]), 'after_git_blob': blob(candidate[p]),
        'before_sha256': sha(original[p]), 'after_sha256': sha(candidate[p]),
        'original_relative_path': 'original/' + p, 'candidate_relative_path': 'candidate/' + p,
    } for p in PATHS],
    'patch': {'file': 'lane-ownership.patch', 'bytes': len(patch.encode()), 'sha256': sha(patch.encode())},
    'production_decision': 'A static slice of the five existing stateless lane references is the sole catalog. QueryPolicy reads IDs directly from it; default_lanes retains its existing Vec return and clones only the references from that same slice. No second constant ID list, boxed lane allocation, DB read, worker construction or lane execution is added for policy.',
    'callsites_checked': [
        {'path': 'crates/cc-search/src/engine.rs', 'consumer': 'default_lanes() -> run_lanes(&lanes, context)', 'change_needed': False},
        {'path': 'crates/cc-search/src/engine_lane_tests.rs', 'consumer': 'default_lanes_registry_keeps_fusion_order', 'change_needed': False},
        {'path': 'crates/cc-search/src/query_policy.rs', 'consumer': 'QueryPolicy::resolve policy obligations', 'change_needed': True}],
    'regressions_added': [
        'Eight intents x seven valid configured/explicit strategies x two deadline-cap fixtures compare policy local order against execution and independent intent-role expectations, plus optional semantic suffix and budgets.',
        'Two fixed complete old policy serializations (explicit local and inherited configured auto) pin wire field order/labels/roles/caps and fingerprint against expected bytes. These tests have not yet executed.'
    ],
    'isolated_format': {'argv': argv, 'started_at_utc': started, 'finished_at_utc': finished,
                        'exit_code': proc.returncode, 'stdout_sha256': sha(proc.stdout), 'stderr_sha256': sha(proc.stderr)},
    'Cargo_build_test_or_Clippy_result': None,
    'project_or_source_guard_executed': False,
    'repository_worktree_index_HEAD_or_refs_changed': False,
    'GitHub_or_study_mutations': False,
    'independent_review': 'Pending /root/todo_audit; this author does not self-approve source semantics.',
    'task_status': {'TODO_done': 163, 'TODO_remaining': 29, 'TODO_closed': 0},
}
(D / 'changed-files.json').write_text(json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + '\n')
print(json.dumps({'files': list(PATHS), 'patch_bytes': len(patch.encode()), 'patch_sha256': sha(patch.encode()),
                  'manifest_sha256': sha((D / 'changed-files.json').read_bytes()),
                  'rustfmt_exit': proc.returncode, 'tests_executed': False}))
