# Prospective temporal v2 artifact operator

This is prepared code, not an execution receipt, a fresh test corpus, authoring authorization, or task acceptance. The six files in this directory form one separately pinned artifact helper domain. No product, original Rust runner/schema/normalizer/scorer, old v1 checker, old corpus, or task ledger is modified.

Keep this directory outside the immutable candidate checkout while executing. It contains exactly: temporal.py, README.md, PROTOCOL.md, protocol.json, locks.template.json, schedule.row-indices.json. The four protocol files are exact revision-2 bytes. The complete map is sorted relative path → {bytes,mode,sha256}; mode is the actual integer permission (normally420). Its SHA256 is over UTF-8 JSON with sorted keys, indent2, ensure_ascii=False, final newline. Pin that digest independently; the operator never supplies its own expected digest.

## Actual inputs and dependency closure

Use Linux64-bit, Python3.10+ standard library, fixed git/rustc/cargo/rg. Run Python with -I -S. The one Python file imports only standard-library modules. before.json/after.json record the actual interpreter binary, version, unicodedata and loaded module files, plus normalized dependency_fingerprint suitable for an independently reviewed seal. Built-in/frozen modules are recorded as such; the interpreter hash binds them. The fingerprint strips deployment paths but keeps every module's bytes/mode/hash. Every mode first uses the actual HTTPS opener to fetch the exact candidate commit metadata from GitHub, retaining its raw response and matching commit/tree. Thus TLS/IDNA/HTTP lazy imports enter the actual pre-author fingerprint instead of being silently ignored during formal authorization. It is not a claim of hermetic OS isolation.

The actual same-source release package must come from the previously reviewed scripts/p8_external_candidate.py. Pass its manifest and independently observed manifest SHA256 plus its candidate-tools.zip. Extraction checks every declared member and the complete ZIP inventory. Actual compiler artifacts, default features, optimized Cargo profile, successful raw build-finished event, original source-input inventory and product/evaluator hashes are checked again. The candidate source checkout must be at its original Cargo manifest path because cc-eval embeds that checkout path for engine provenance. HEAD/clean Git bytes/modes and complete crates/Cargo plus scripts/workflow/tests/source_integrity maps are checked before/after. These expanded map hashes are explicitly different named representations from the builder's plain path→SHA source map; the builder's original plain-map SHA is separately verified against its receipt.

The child environment is an explicit allowlist with a new HOME/config/cache and no inherited provider credential/config variables. rg is resolved from a dedicated first PATH directory and must equal the pinned absolute executable. No syscall-zero network evidence is claimed. Candidate features=[] plus the exact local-text-hidden suite config select the local arm; model/lane fields from actual responses remain in raw reports. The stable arm ID candidate_local_default means local retrieval with default Cargo features=[]; its indexing configuration is the explicit local-text-hidden profile. It is not a measurement of the old default indexing configuration. The baseline is the unchanged rg adapter: a literal whole-query fixed-string search, not token/BM25 retrieval.

## First run: invented controls, before any fresh author

The following arguments are shared by synthetic-control, prepare-source, execute and replay:

    --source-root ORIGINAL_CLEAN_BUILD_CHECKOUT
    --expected-source FULL40_CANDIDATE_SHA
    --package-manifest ACTUAL_PACKAGE/manifest.json
    --package-manifest-sha256 INDEPENDENT_ACTUAL_SHA256
    --archive ACTUAL_PACKAGE/candidate-tools.zip
    --helper-map-sha256 INDEPENDENT_SIX_FILE_MAP_SHA256
    --rg /absolute/path/to/rg
    --output NEW_DISJOINT_DIRECTORY

Run:

    python3 -I -S temporal.py synthetic-control [shared arguments]

This writes explicitly invented Python controls and48 synthetic rows outside the product tree. It does not select a new repository or author actual held-out tasks. The synthetic review labels are definitions for a software fixture, not a human natural-language/source-gold approval.

It actually calls original cc-eval freeze and validate, runs query/source lock drifts through original validate, checks8 malformed temporal contract controls, then runs original mcp-stdio and rg adapters at top10/repetitions3/warmup0/timeout30000/seed20261003. Each arm starts with original runner's newly materialized source/index; OS page caches are not called cold. The144-row sequence for each arm must exactly match the precomputed repetition-outer shuffle. Actual64-bit ELF identities and the real288 result schedule are checked. The original gates must both exit0 for synthetic capability verification. In addition, the fixed f0001.en_original control must return units/unit01.py with evidence_valid=True in all3 repetitions of both arms: six real positive witnesses. No rank1/native-score/symbol-name threshold is imposed. A disposable normalized observation copy changed to all no_match/empty hits must fail this witness predicate; no extra product request occurs. Each original raw run is replayed in a new copy with unchanged cc-eval; all9 derived outputs must be byte-identical. Separate copied-raw corruption controls must produce original exit2 and raw response drift. First failures and partial outputs are retained. The real local HTTP redirect control sends a non-secret synthetic Authorization header to a302 source, observes zero sink calls, then directly reaches the sink without a header; the actual NoRedirect opener refuses before forwarding. All GitHub success and HTTP-error bodies and per-arm ref-check responses are retained. NoRedirect is not a claim of overall network isolation.

The actual preflight exports rg executable/version/hash, Python/unicodedata/runtime evidence, complete helper/source maps, literal fixture/query bytes, original run manifests, actual288 status/order/timing observations, native no-answer scores, normalized and raw-empty observations, original gate exits and original replay byte comparisons. None is filled with expected values and called observed. Original report::replay verifies retained raw/normalized hashes and recomputes scores; it does not rerun the normalizer over raw MCP. This limitation is explicit.

Synthetic controls may be revised prospectively with preserved failures before the candidate/source/measurement seal. After that seal and fresh authoring, changing any measurement helper requires a genuinely future test rather than rescuing the same exposed data.

## Metadata/source preparation without new test questions

After independent metadata-only selection, clone/fetch the exact authorized public upstream commit into a clean separate checkout. This helper does not clone arbitrary URLs, choose repositories, evaluate novelty/license, or read old held-out bodies.

    python3 -I -S temporal.py prepare-source [shared arguments] \
      --upstream-root CLEAN_FIXED_UPSTREAM --upstream-commit FULL40_UPSTREAM_SHA

The generic policy classifies every tracked upstream file by Git blob/mode and actual SHA/bytes. Protected product/system/cache/environment paths are excluded; admitted text is UTF-8, NUL-free, at most512000bytes (the unchanged default product cap). Unresolved LFS is excluded. Symlink/gitlink/submodule shapes fail explicitly. Every exclusion and its reason is retained. No inclusion decision can use a query, gold path or retrieval result. All same-rule license/doc/hidden text is included. Source is later materialized without .git, so this policy does not claim to apply checkout Git-ignore rules to a non-Git benchmark workspace.

The original Rust freeze API needs a nonempty query input even to compute a SourceLock. The helper supplies one fixed invented digest-only control record, never dispatches it, runs original freeze/validate, and exports the real BLAKE3 source-lock digest plus full admission inventory. This is not a new test body. It does not establish actual index readiness. License/novelty/copy relationships and exclusion metadata remain independent review requirements before authoring.

## Data format and distinct temporal validation

No real data is supplied by this implementation. Later fresh author/reviewer produce a separate immutable data directory:

- data.json references suite.json, source-admission.json, gold-review.json, the exact query_record, reserved_repository_id, reserved_family_ids, synthetic_control_only=false, review_status=accepted_independent, and review_evidence_files.
- queries.jsonl uses the unchanged original native Query schema:48 rows in family then variant order, split=holdout. Categories remain the original ten enums. Coverage strata are separate source_task_facet labels; no_answer is the original boolean.
- Each annotations.prospective_temporal_v2 includes protocol_id, split_scheme=prospective_temporal_test_only, variant, global_component, query_language.
- gold-review.json has distinct author_id/reviewer_id, status=accepted_source_only, and families keyed by the12 exact reserved IDs. Each contains global_component, source_task_facet, variants_semantically_equivalent, natural_language_accepted, scope, absence_evidence, no_answer_scope_reviewed, gold_spans, hard_negatives, zero_lexical_english_original.
- gold_spans is ordered exactly like the flattened answer groups/alternatives: path/span/symbol/span_sha256. Every alternative needs its actual UTF-8 byte range, not a fabricated line offset.
- hard_negatives entries contain path/span/span_sha256/symbol_name/why_wrong/independent_wrongness_reviewed. No-answer source/scope/version absence requires source-only independent evidence.
- review_evidence_files maps public_reference_allowlist_sha256, leakage_review_sha256, independent_source_gold_review_sha256 and quarantine_history_sha256 to local evidence paths.
- source-admission.json must exactly equal the same preauthor generic admission recomputed at the fixed source commit. Arbitrary post-gold exclusions are rejected.
- locks.json preserves the exact revision-2 template keys. Formal state is DATA_SEALED/data.status=SEALED/execution.status=NOT_RUN/attempt_consumed=false. All nonfuture nulls block execution. Its author/reviewer role IDs must equal those in the actual gold review, repository canonical ID must equal the reserved family prefix, native_suite_lock is the complete actual Suite object, and variant_schedule is exactly the four named variants. Concurrency is an integer1, not a truthy boolean. execution fields and first_scheduled_query_record are necessarily future, never fabricated.

    python3 -I -S temporal.py validate-data --data DATA

This is explicitly the new temporal contract. It is not old check.py --custodian, v1 quota compliance, ACL certification, or a substitute for the unchanged Rust validate or non-author semantic review. Both original Rust validation and this contract must succeed before dispatch. Derived data commitments use canonical JSON for the flattened span records, negative map, global-component map and zero-overlap token receipt. The operator emits their actual SHA values for sealing.

## Ordinary Actions authorization with a known actual run/job

Do not put a yet-unknown Actions run number into a placeholder and later change executable code. Freeze this wait/execute logic before authoring, use independent utility/data checkouts, and follow this sequence:

1. An ordinary workflow starts at the presealed utility commit. It has a contents:read token and no product query step before authorization. Its candidate checkout remains the immutable candidate commit. It prints GITHUB_RUN_ID, GITHUB_JOB, GITHUB_SHA and run attempt. Run:

       python3 -I -S temporal.py wait-authorization --output NEW_AUTH_DIRECTORY

2. The helper waits at most600seconds for the unique branch refs/heads/work/ptv2-permit-<run_id>. Root independently verifies service chronology, the actual run/job, dependency acceptance, unchanged seals, actual preflight and prior same-seal execution history. Root creates a data-only commit containing files beneath artifacts/benchmarks/prospective-temporal-v2/authorizations/<run_id>/ and first creates that branch. No candidate/helper source changes are needed. These are ordinary authorized GitHub blob/tree/commit/create_branch operations; no tag API is assumed.

3. The required authorization.json has kind=independent_one_attempt_authorization, status=released_for_one_execution, protocol_id, locks_sha256, complete data_inventory, candidate_commit, helper_inventory_sha256, candidate_package_manifest_sha256, archive_sha256, product_binary_sha256, eval_binary_sha256, actual_python_dependency_fingerprint, and attempt_id. Both product/evaluator SHA fields must match locks.candidate as well as the actual unpacked binaries. The actual executable bytes/modes are checked before each arm and after execution/replay/source preparation, not only on ZIP extraction. attempt_id is SHA256 of the canonical seal object constructed in permit_check; not an arbitrary run name.

   authorized_actions is exactly {run_id,run_attempt:"1",job_name,utility_sha}. service_witnesses has candidate_seal/author_start/reviewer_start/first_draft/first_engine_owner_exposure/data_seal/release. Every witness supplies raw_path, actual file record, service_event_id and independent reviewer_id. Ordinary event witnesses supply observed_utc; reviewer_start must precede data_seal. The first_engine_owner_exposure record has one of the two explicit modes below instead of inventing a timestamp for a read that has not occurred. Preserve actual service responses/messages; Git dates and self-asserted timestamps alone are insufficient. prior_attempts pins the raw service enumeration and independent review; both same_seal_dispatched_attempts and unknown_dispatch_attempts must be empty. manual_service_and_dependency_review must be accepted by the non-author integrator; this string alone is never called independent proof.

4. wait-authorization resolves the branch once to a full commit and fetches only that immutable tree's bounded data files, verifying Git blob SHA/size and retaining full API responses. Read its final JSON/binding.json to obtain the actual authorization SHA and file paths. Invoke:

       python3 -I -S temporal.py execute [shared arguments] \
         --data SEALED_DATA \
         --authorization NEW_AUTH_DIRECTORY/files/authorization.json \
         --authorization-sha256 ACTUAL_ROOT_PUBLISHED_SHA \
         --authorization-binding NEW_AUTH_DIRECTORY/binding.json

   The downloaded authorization remains subject to the full permit/seal checks. The branch is rechecked before dispatch, before each arm, and after. Movement fails; a mutable branch is not called an immutable lock. Same-account role separation is disclosed and is not an ACL claim.

The local exclusive attempt record conservatively marks dispatch possible before the first run command. It cannot prevent another machine, path or workflow from running. Final independent review must enumerate actual same-seal service attempts again and reject duplicate/unknown dispatch. No automatic retry occurs. A timeout after possible dispatch consumes the attempt. Setup failures can only be retried after independent original raw proves zero scheduled dispatch and identical seals. The first actual query's precise service/runtime timing still needs the bound actual run witness; a prelaunch timestamp is not falsely labelled actual first dispatch.

The exact original budgets and arm order never change. Original run/replay exits0/1/2/3 are retained. Quality is descriptive; original no-answer gate failures remain failures. Unknown statuses, missing rows, original invalid2/cancelled3, infrastructure failures and source/scope drift prevent complete integrity verification. Original gate1 is retained as a valid measurement with a failed quality/availability gate, including recorded Partial or row-level errors. Every original score and status remains visible; Partial is never reclassified as complete empty. Undefined optional scores propagate as null through the complete12-observation family, arm mean and inconclusive interval without discarding rows, shrinking denominators or inventing zeros. No-answer raw_explicit_no_match is unsupported/null because the original public payload exposes no documented explicit no-match field.

## Raw-only verification

    python3 -I -S temporal.py replay [shared arguments] \
      --data SEALED_DATA --runs ORIGINAL_RUNS \
      --raw-inventory-sha256 INDEPENDENT_COMPLETE_ORIGINAL_INVENTORY_SHA

Add --synthetic-control only for the explicit invented fixture. Both original run directories must be present under candidate_local_default and rg_baseline. Output must be new and disjoint. The verifier pins the full raw inventory, replays copies with the actual frozen evaluator, verifies originalderived bytes, verifies all288 schedule entries and source/scope/status, and computes the separately named paired10000-draw family summary.

Every row and variant/repetition remains in the report. Ten answerable and two no-answer components are separate. Bootstrap uses sorted component keys, Python random.Random(20261003),10000 whole-cluster paired draws and nearest-rank endpoints. Core per-row scores are exclusively original scores.jsonl validated by original replay. No new product requests occur. Source, helper, data, original raw and derived bytes remain unchanged. These are conditional exploratory intervals from one selected repo, not broad quality or release certification.

Final task disposition, real service history, chronology, source-gold correctness, original deterministic/scorer regressions and P8-002/003 dependencies remain separate non-author acceptance. No generated prepared file closes a TODO, increases v1 clean custody, or authorizes fresh real bodies.

## Revision2 preparation history

The first implementation draft and its static findings are retained. This revision adds the complete original validation domain, literal integer concurrency, exact authorization tree identity, true output ownership before failure receipts, redirect refusal before credential forwarding with raw API preservation, same-path actual HTTPS dependency warmup, formal fresh-role/source identity consistency, and the six actual positive witnesses plus all-no-match controls. It does not change the48×2×3 schedule, original Rust/schema/scorer, two no-answer controls, original query deadlines or original gates. The prepare/run/replay code still requires actual CI; this document and static inspection are not execution evidence.

## Revision3 freeze consistency and exposure interpretation

First-seal product/evaluator hashes, execution-authorization hashes and actual unpacked bytes must all be equal. The complete binary snapshot is checked around every arm and at completion. Preflight includes four single-field seal/permit hash mutations and a real byte modification of an owned copied executable; all must be detected while actual binaries remain unchanged.

The service chronology is candidate_seal < author_start < first_draft < data_seal < release, plus candidate_seal < reviewer_start < data_seal. Release must already have occurred. The source-gold author/reviewer IDs remain equal to the declared fresh roles.

The original protocol requires recording earliest exposure, not forcing the engine owner to read the bodies. Root and protocol author accepted this explicit interpretation before any new body authoring:

- witnessed_exposure: provide kind=witnessed_exposure and the real observed_utc event. candidate_seal < exposure, first_draft <= exposure, and exposure no later than actual current time are required.
- not_observed_before_execution: provide that exact kind, independent_information_flow_review=accepted_observation, actual pinned independent information-flow audit raw evidence, and observation_cutoff_utc covering at least release and no later than actual current time. This means no exposure was observed in the reviewed record through that cutoff. It is not a proof that exposure never happened, an ACL, or a replacement for unknown evidence. A kind=unknown is refused. Final review must still audit later/unknown contact and the actual first exposure if one occurs.

No deliberate null is silently filled and no event timestamp is fabricated. The non-null locks freeze_order record must reference the actual observation evidence. This is a pre-author operational interpretation of the unchanged revision-2 protocol; the original protocol files remain intact. Preflight exercises witnessed/not-observed positives and reviewer-late/exposure-before-draft/unknown/insufficient-cutoff negatives using explicitly invented timestamps, never calling them real service evidence.

A second protocol-author clarification removes an unintended helper-only all-success gate from the first drafts. Formal replay retains all288 rows with their original statuses and core scores. An original gate1 remains gate_failed, including Partial, no-answer failure or row timeout/tool error. Source/scope hard errors and original invalid2/cancelled3 remain blocked. Optional undefined metrics are null for the whole affected12-observation component and yield inconclusive complete-stratum means/intervals; no valid-subset averaging occurs. Synthetic capability still requires original gate0 plus six actual positive witnesses. This correction preserves the original scorer and does not relax any original gate.

Revision3, its negative controls, actual binary checks and the complete loaded-module closure still require ordinary CI execution before the candidate/measurement seal. Static acceptance cannot stand in for those results.

## Revision3.1 execution-contract controls

Witnessed exposure cannot be a future event; a separate invented future-exposure negative proves that check before any authoring. Synthetic preparation also runs helper-unit controls for preserved original gate0/gate1, a valid Partial observation, refusal of invalid2/cancelled3/non-integer gates, unknown statuses, source/evidence/scope violations, one-null-row propagation through the full12-observation family and10-component arm, an inconclusive paired interval with the same10 planned components, and a finite complete-family bootstrap. These controls call the same validation and averaging functions used by the real report. Their inputs are explicitly invented observations, not actual additional product results, original-normalizer tests, or a substitute for the actual288-row scorer/raw replay. They dispatch zero extra product queries and do not change the original gates or schedule.
