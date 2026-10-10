#!/usr/bin/env python3
"""Record independent source conclusions and exact bytes; do not import candidates."""
import hashlib
import json
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path('/workspace/scratch/bfccb8494ba0')
OUT = Path(__file__).resolve().parent

def identity(path):
    data = path.read_bytes()
    return dict(local_path=str(path), bytes=len(data), sha256=hashlib.sha256(data).hexdigest(),
                git_blob=hashlib.sha1(b'blob '+str(len(data)).encode()+b'\0'+data).hexdigest())

def load(relative):
    return json.loads((ROOT / relative).read_text())

def check(row, expected):
    for key in ('bytes', 'sha256', 'git_blob'):
        assert row[key] == expected[key], (row['local_path'], key)

integration = load('round40-PR192-main-integration-candidate/source-manifest.json')
native = load('round40-profile-task-descriptive-native-candidate/v2/source-manifest.json')
driver = load('round40-task-descriptive-driver-candidate/source-manifest.json')
sources = {}
for item in integration['files']:
    path = ROOT / 'round40-PR192-main-integration-candidate/candidate' / item['path']
    value = identity(path)
    check(value, item['after'])
    sources[item['path']] = dict(value, status=item['status'], author='/root/todo_audit', layer='PR192 three-way integration')
for item in native['files']:
    path = ROOT / 'round40-profile-task-descriptive-native-candidate/v2/candidate' / item['path']
    value = identity(path)
    check(value, item['after'])
    sources[item['path']] = dict(value, status='M', author='/root/todo_audit', layer='explicit descriptive scope v2')
for item in driver['source_leaves']:
    value = identity(Path(item['local_path']))
    check(value, item)
    sources[item['repository_path']] = dict(value, status='A', author='/root/execution_recon', layer='new task driver/protocol')
capture_receipt = load('round40-task-profile-workflow-candidate/actual-host-controls-attempt01/actual-controls-and-workflow-review.json')
for relative in ('scripts/p8_task_profile_capture.py', 'scripts/tests/test_p8_task_profile_capture.py', '.github/workflows/p8-task-profile.yml'):
    value = identity(ROOT / 'round40-task-profile-workflow-candidate/candidate' / relative)
    check(value, capture_receipt['exact_inputs_before'][relative])
    check(value, capture_receipt['exact_inputs_after'][relative])
    sources[relative] = dict(value, status='A', author='/root', layer='new task capture/workflow')
assert len(sources) == 17
assert sum(value['status'] == 'A' for value in sources.values()) == 12
assert sum(value['status'] == 'M' for value in sources.values()) == 5
assert capture_receipt['exit_code'] == 0 and capture_receipt['inputs_unchanged'] is True
core = identity(ROOT / 'round32-capture-context-admission/guard-view/scripts/p8_scale_diagnostic_capture.py')
check(core, capture_receipt['exact_inputs_before']['scripts/p8_scale_diagnostic_capture.py'])
assert core['git_blob'] == '0ce885a45e6373e2c80f4e65d70dfa08fdaf4cb1'

report = dict(
    schema='independent-fresh45-combination-source-review-v1',
    recorded_utc=datetime.now(timezone.utc).isoformat(),
    reviewer='/root/pr_triage',
    source_authors=['/root/todo_audit', '/root/execution_recon', '/root'],
    result='accepted_scoped_candidate_source_pending_actual_product_identity_and_engineering_execution',
    actual_P=None, actual_R=None, actual_G=None,
    base_main=integration['base'], donor=integration['donor'], common_base=integration['common_base'],
    scope='Seventeen candidate leaves, five modified and twelve added relative to fixed main. Independent borrowed-sorted-cursor leaf is separately reviewed, not silently included in this seventeen-leaf record.',
    final_candidate_source_inventory=dict(sorted(sources.items())),
    original_capture_core=core,
    review_method='Read complete diffs, relevant full native validation/execution paths, complete new driver, schema, document, capture, tests and workflow; compare the final v2 test-only change; read existing author control receipts/stdout/stderr. This sealing program only hashes bytes and verifies frozen metadata. It does not import candidate modules or execute validators, tests, Rust, ELF, workloads, Actions or API requests.',
    findings=[
        dict(area='PR192 three-way integration and old behavior', conclusion='Five modified leaves combine exact donor e7f8 with current b9 and common 10ec; the retained donor workflow/document/schema/driver/test five additions stay exact. Current wide4096 implementation and its two existing controls are preserved. Original mutation_case.evaluate takes the old false retain-evidence branch; additional initial/final full evidence is explicitly requested by the isolated-profile caller. No oracle, streaming, V18, installer or payload implementation is changed by these seventeen leaves.'),
        dict(area='Fresh execution semantics', conclusion='Each selected mutation receives fresh pristine A/B setup and complete initial parity. Only its selected mutation runs, followed by original closure, full control and complete parity. Fresh batch target-zero witness is explicit; config target-one witness remains distinct. Fanout uses actual N+1 source files, original independent call-edge assertions, initial full fifteen-table evidence and final full control; files1000 is only capacity reservation.'),
        dict(area='Strict new native scope', conclusion='Only profile_task_descriptive_v1 permits release N1. It requires release mode, canonical single scale, seed0xc0ffee, repetitions1, shard0/count1, scale_capacity_v1, 18000000ms, 536870912bytes, main200/1024, original batches and fanouts. Cold identity and profile identity remain exclusive. Old isolated release still requires N30 and old full/cold/wide constructors and public validators do not acquire the N1 exception. Debug binaries remain unable to admit any registered release cell.'),
        dict(area='Shared validator hook', conclusion='The shared private raw inspector defaults to profile_isolated_v1; the new task driver must supply both an explicit selected profile and the exact new scope. It does not infer permission from input plan text. Old public full/cold/isolated inspector paths reject new task records. The original native supervisor, source/build checks, physical oracle tag, limits, raw EOF, closure and parity predicates remain in use.'),
        dict(area='Final forty-five-plan release control', conclusion='Final native v2 only nests the existing positive/negative plan/serde/range control over all five RELEASE_SCALES for eight profiles, retaining the five fanout controls. Thus its release build checks forty mutation plans plus five fanouts. The workflow selects this exact existing named test with --release/--locked/--exact after the fresh release build and registry but before build upload. It does not run any database, worker or measurement. Its actual Rust execution is still pending.'),
        dict(area='Registration and executable identity', conclusion='The new build wraps original full.build and seals task-registry.json containing exactly45 predetermined plans, source manifest, binary SHA256/BLAKE3, original build receipt, observer, run and attempt before cells. validate_build reconstructs that registration and checks full original native build/ELF/source origin and inventory. Rehashing a modified plan cannot convert it into the registered plan. The later release plan test uses the same Cargo target but does not replace the copied sealed measurement ELF or erase its original fresh-producer receipt.'),
        dict(area='Population and rejection', conclusion='Expected slots are exactly eight profiles times five scales plus five fanouts, all repetition0. combine requires one exact source/binary/observer/build/registry/run/attempt, unique exact slots, same pristine input digest per scale, original measured environment for each cell, all85 records and actual fanout first-build-incomplete coverage. No-op setups are selected by predeclared profile name for the five cold points; all forty setups remain and no times influence selection. No quantiles, pooled means, tail inference or automatic task update is computed.'),
        dict(area='Failure and aggregation', conclusion='aggregate processes every supplied complete input and retains original task receipts, inventories and errors; any failed input or missing/duplicate/extra slot prevents passed. Workflow aggregate executes after matrix completion even when cells failed, provided build succeeded. It downloads only p8-task-shard-* artifacts, never p8-task-progress prefixes. An absent shard is explicit missing coverage, never substituted with old studies or diagnostics.'),
        dict(area='Capture instance and nested interface', conclusion='The new wrapper privately loads the unchanged core and changes only that private instance globals. It launches the actual task driver with explicit run/attempt/profile/scale/shard0. Driver build/native-build/p8-scale and cell/shard/task-shard.json plus shard/native-shard/native/report.json/raw.jsonl match workflow/wrapper paths. Temporary environment recorded at the task root equals original full.run_shard out.parent; worker root remains a child of that filesystem.'),
        dict(area='Finite schedule and failure custody', conclusion='Workflow has a single45 include matrix, fail-fastfalse/maxparallel10, no profile group dependency chain. It calls indices0 through19 sequentially at original absolute900s targets, each with previous upload ID/digest, then finish20 with upload19 ACK. Every checkpoint is always conditioned on successful launch; uploaded bundles use ready output. Native early completion makes remaining wait targets return immediately while preserving sequence. Original core keeps incomplete bytes/capture faults and never asserts native EOF. Complete task shard upload is independently always, and final verdict preserves actual driver/native failure.'),
        dict(area='Workflow source, permissions and budgets', conclusion='All checkouts bind explicit PR head or push/dispatch SHA; contents permission is read only. Trigger is workflow_dispatch or the unique labeled event; it does not trigger or modify prior workflows. Job-level env uses matrix only; runner.temp/context values occur in permitted step fields. Native five-hour budget includes setup/full-control/parity and is never reset by capture. Wrapper350min and hosted-job350min are outer failure boundaries, not extra native measurement time. Native cleanup after externally terminated wrapper remains explicitly unknown.'),
        dict(area='Sparse checkout and targets', conclusion='The workflow excludes artifacts while retaining Cargo/crates/scripts/docs/workflows. It builds only cc-eval p8-scale and selects the p8_scale integration-test target; this does not enable unrelated dependency cfg(test) modules containing historical external capability paths. There is no new cargo fmt --all command whose module traversal would require those excluded archive paths. Build/test runtime remains to be observed, not presumed.'),
    ],
    existing_execution_evidence=[
        dict(owner='/root/todo_audit', evidence='round40-PR192-main-integration-candidate/python-controls-attempt01/receipt.json', scope='Original full/cold/isolated synthetic Python modules', tests=64, exit_code=0, rerun_by_reviewer=False),
        dict(owner='/root/todo_audit', evidence='round40-profile-task-descriptive-native-candidate/python-controls-attempt01/receipt.json', scope='Same64 synthetic controls on new shared hook; two explicit-hook tests covering13 selections', old_tests=64, hook_tests=2, hook_cases=13, exits=[0,0], rerun_by_reviewer=False),
        dict(owner='/root/execution_recon', evidence='round40-task-descriptive-driver-candidate/synthetic-attempt02-final/receipt.json', scope='Final new task driver synthetic controls', tests=12, exit_code=0, inputs_unchanged=True, rerun_by_reviewer=False, prior_attempt='Earlier11-test success is retained separately and is not relabeled as the final12-test version.'),
        dict(owner='/root', evidence='round40-task-profile-workflow-candidate/actual-host-controls-attempt01/actual-controls-and-workflow-review.json', scope='Three harmless capture controls; YAML structure, embedded Python syntax and bash -n', tests=3, exit_code=0, inputs_unchanged=True, rerun_by_reviewer=False, limitations='Not actionlint, GitHub workflow execution, Rust compilation or native measurement.'),
    ],
    limitations=[
        'No actual combined P/R/G exists in this report. Exact official commit, parent/tree and complete domain maps must be bound to the later actual object, rather than guessed from candidates.',
        'Rust compilation, formatter, Clippy, three new Rust controls and the release45-plan control have not run locally. Source acceptance and Python fixture controls do not certify them.',
        'No45-cell,1350-cell or repeated native study has run for this source. N1 freshness differs from old continuous history, cannot fill old slots and does not prove statistical tails, performance gains or release certification.',
        'A wrapper hard timeout or hosted-runner loss may leave native cleanup or the last byte tail unknown. It remains failure/missing data. Neither upload success nor wrapper0 alone establishes a native pass.',
        'Historical failed studies, previous pending reports, old native N30 protocol and their original artifacts remain unchanged. New measurement results must retain all failures and missing cells.',
    ],
    blockers_found=[],
    requested_source_changes=[],
    task_credit=0,
    future_execution='Only the separately authorized final-source engineering admission and actual workflow may establish runtime results. This static review adds no new study, threshold, statistical requirement or acceptance gate.'
)
destination = OUT / 'independent-combination-source-review.json'
destination.write_text(json.dumps(report, ensure_ascii=False, indent=2)+'\n')
selection = dict(schema='scoped-static-review-selection-v1', files=[identity(destination), identity(Path(__file__))],
                 referenced_author_originals=[
                     dict(git_blob='9551c2249169c287eae1cfb7384cb2ea181c0657', role='PR192 main integration complete author originals'),
                     dict(git_blob='789aa3d1adc9a6e8970b154ca8a0ee643af57108', role='Native scope v1 history'),
                     dict(git_blob='f4e5411e59f0f8aadb6988fac0b228321d8ddd27', role='Final native v2 test-only expansion originals'),
                     dict(git_blob='59abc97551c637f5a8a5184a266fa6a0676e4d06', role='New task driver complete author originals'),
                 ], no_nested_prior_archives=True)
(OUT / 'archive-selection.json').write_text(json.dumps(selection, ensure_ascii=False, indent=2)+'\n')
print(json.dumps(dict(report=identity(destination), selection=identity(OUT/'archive-selection.json'), source_leaves=len(sources), status='recorded_static_review_only')))
