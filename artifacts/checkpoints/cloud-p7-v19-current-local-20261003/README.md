# New current Local dev baseline

New run `v19-current-6f3-local-20261003`; exact source `6f3cee1f2d23b43af5271bcaeaea67b931fd4fb7`. Actual copied default product has zero features; binary SHA256 `ef339ca364f837515321a16d3438e02fb565fee1d2534629a87cab3118d31247`. Product compiler-artifact/build receipt and evaluator artifact/hash are retained; executable stays at the local path in receipt. Clean tracked source was checked before/after execution. No production/scorer/gold change or paid/live provider. This is new actual stdio execution, never recovery of missing historical306raw.

Eight unchanged locked dev suites:56questions,164rows. Native and compatibility columns are separate: all authorized manifests are native; compatibility is explicitlynot_run without a separately locked input. No gold conversion is invented. Eight validations and eight replays completed. Replay gate exits match runs (six0,two1) and every retained run file hash remains identical. Exit0 records observations, not quality certification.

| Suite | Native Top1 | Native nDCG10 | Compat | Rows | Partial | Gate exit |
|---|---:|---:|---|---:|---:|---:|
| p0-codecortex-subset | 0.857143 | 0.857143 | not_run | 42 | 39 | 1 |
| p0-python-api | 1.000000 | 1.000000 | not_run | 1 | 0 | 0 |
| p0-rust-api | 1.000000 | 1.000000 | not_run | 1 | 0 | 0 |
| p0-smoke | 0.700000 | 0.700000 | not_run | 33 | 0 | 0 |
| p1b-exact | 1.000000 | 1.000000 | not_run | 24 | 0 | 0 |
| p1c-bm25 | 1.000000 | 1.000000 | not_run | 3 | 0 | 0 |
| p1c-intents | 0.625000 | 0.656250 | not_run | 54 | 24 | 1 |
| p1c-softscope | 1.000000 | 1.000000 | not_run | 6 | 0 | 0 |

R09 gold still points to absent compute_fingerprint_for_unit in the unchanged declared source. Its threeNoMatch rows, zero native scores and original denominator remain intact; material-validity.json explicitlyclassifies it invalid/inconclusive for current quality acceptance. No replacement gold or silent deletion. Supplemental material-valid macro means are labelled separately and retain Partial, not a pass.

63Partial rows remain gate failures:54havepacking.partial;9do not. Lane candidate/path-token/graph-expansion limits are recorded per row and can overlap packing causes, so cause counts are not added. All raw source evidence has0invalid/0unverified hits. Source byte validation is not a span-gold quality oracle.

S11 current three rows are empty NoMatch; this does not rewrite earlier preserved failures or establish general no-answer calibration. KNOWN-FAILURES receives a dated new-source note only. The invalid CLI --profile baseline example returns an actual unknown-measurement-profile error; smoke is the admitted observation profile used here.

This small authored dev baseline has0holdout,0external reviewed multi-repo questions,19symbol-gold constraints,0span gold and0chain gold. Current scorer lacks chain metrics. It cannot certify approximately600public questions, holdout, facet/span/chain, macro/micro/stratified pairedbootstrap or live semantic ablation. Parent six-repo holdout/gold is not read.

PR63 original181files are byte-identical;388currentproduction/Cargo fingerprints equal reviewed83a6 baseline. Formal raw validator passed in a copied temporary evidence tree so frozen owner receipt is untouched. V05/V16/V18 literal minimum consolidation is source-bound; no new arbitrary kinds/scale requirements. P7-016 crash/SIGKILL and full P7/quality remain open.

Build receipt uses SHA256; evaluator manifest uses BLAKE3 (manifest.rs). Both are independently verified against identical actual product bytes. Initial assembler equality across algorithms was corrected before final receipt, not a product failure. PR61 exact final6f3 CI153 check/MSRV/security directly read completed/success.

PR66 c9ae303 twenty files are unchanged; verifier passed. Exact combined5de0881 HTTP original3+independent2 pass, scoped strict clippy/fmt pass; new independent consumer raw retained. Initial missing output-directory write errors corrected by mkdir only, hash retained; no assertion/owner edit. These support the three bounded V11 rows, not a full V11 or cross-version certificate.
