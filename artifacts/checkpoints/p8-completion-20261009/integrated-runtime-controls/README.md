# Integrated runtime Python controls — static draft

This auxiliary workflow is intended for `task/p8-snapshot-leaf-engineering-20261009`. It runs once on the actual pushed commit, concurrently with the separate Rust engineering workflow. It does not start a primary scale study.

The fixed proposal is main `55902428d49b09bb4a90cee6ccccf985680618bb` plus 13 reviewed native paths and 5 reviewed Python paths. Independent read-only Git byte hashing reproduced all 1,089 production inputs and manifest `d006974afc0fa4752f3ebcc5ac441c651bf18870ca6af8ae1217e135bed2730d`. The workflow checks the actual commit and complete production/observer/input snapshots before and after execution.

| Module | Expected methods | Explicit subTest observations |
| --- | ---: | ---: |
| test_p8_runtime | 14 | 0 |
| test_p8_runtime_evidence | 15 | 0 |
| test_p8_runtime_cache | 13 | 22 |
| test_p8_runtime_finalization | 8 | 2 |
| Total | 50 | 24 |

All four complete modules are scheduled. Main's three new build-receipt controls have no explicit subTest callbacks. The original 24 parameterized subTest observations therefore remain unchanged. Loops and delegated protocol fixtures are not counted as extra test methods. The actual unittest loader, test starts/stops/successes, zero skips/expected failures, method counts, and each explicit subTest parameter/outcome must all match the pinned manifest.

Nine observer files include main's private-target/actual-toolchain/fresh:false and backfill sealing contracts. Their exact bodies are pinned alongside the four complete test modules. This adapts the prior 47-method controller without changing its command sequencing, source/observer before-and-after verification, original logs, failure retention, or inventory refusal. It adds an explicit nine-observer population check.

Only Python protocol/regression controls run: Ubuntu 24.04, contents:read, first attempt, actual github.sha checkout, 20-minute job timeout. No Cargo, native product workload, paid provider, or new scale samples. The always-upload step retains records when the runner remains able to upload; hard termination can prevent receipt/upload and missing evidence never passes.

Local preparation used only Git reads, AST/YAML parsing and byte/hash comparison. No integrated tests were run locally. The unreferenced blobs do not trigger Actions and do not approve this combined source. Root must review the actual complete tree and create the intended branch separately.

Old runtime observations, review chains and the separate 47-method result keep their original source identity. New TODO completions: 0. Remaining TODOs: 29.

The upload step explicitly includes hidden files within the same two runner-temporary paths. This addresses a confirmed transport failure in the earlier, separately identified c8 47-method execution: its independent receiver found 630 sealed synthetic checkout/.git files (463,965 bytes) missing from the uploaded ZIP. Its actual 47/24 test result and incomplete archive are separate conclusions; this change neither repairs nor reruns that old evidence.

The retained checkout directories originate from ReceiptFixture inside fresh runtime-protocol temporary roots: git init, synthetic source creation, add and local commit. They are not clones of the actual checkout and have no remote or credential configuration in their constructor. Only the explicitly listed observer source files are copied into them. The actual Actions checkout, including its Git credentials, is outside both uploaded paths. Controller, manifest, test bodies and upload paths remain unchanged. Full future archive integrity still requires actual reception and receipt/member equality.
