# P7-017 offline runner and process evidence

The offline gate still executes the original fourteen MCP tools, source
verification, capability states, public error contracts, no semantic-cache
checks, and close/reopen persistence checks. It covers both the default product
and the `semantic` product, each with ordinary local configuration and explicit
`semantic.enabled=false`. Every product retains the original cleared child
environment with fixture-local HOME/XDG paths and no credentials.

## PID identity and network evidence

The old runner used a namespace-local child PID directly in `/proc/<pid>/status`.
When procfs exposes an ancestor namespace, that path can be missing or belong to
an unrelated process. `tests/support/p7_procfs.rs` now requires a unique match
using the observer's own `NSpid`, the child's namespace PID, the same PID
namespace inode, and the child's direct parent as represented by procfs.
Missing procfs, missing identity, a different parent/namespace, and ambiguity
remain errors. The result records both the requested PID and the procfs PID.

The explicit `SpawnPolicy` has two modes. Ordinary CI can still execute the
direct local-behavior contract, whose network scope remains unclaimed. The
isolated gate uses `scripts/p7_offline_network.py`: the parent has the existing
EPERM network preconditions, and **each real product receives a process-fatal
seccomp filter before exec**. A product cannot swallow EPERM and look offline.
The filter denies socket operations and io_uring, rejects alternate ABIs with
process termination, closes inherited socket descriptors, and rejects socket
standard streams. It changes only the launched process and its descendants.

Two sacrificial exec children must print that they reached their IPv4/IPv6
socket probe and then terminate with SIGSYS. An earlier crash or normal exit
does not pass. Their receipts and the product pre-exec receipts identify the
launcher script, Python executable and actual exec target separately. The
original product build-receipt check is unchanged and runs again after the
matrix; the launcher is never presented as the product binary.

The runner retains ownership of each actual child and requires normal exit 0
within the original five-second close deadline. Together with the verified
process-fatal filter and successful tool responses, this supports zero network
socket attempts through shutdown. It is not a syscall trace, machine-wide
isolation claim, or a claim about enabled semantic networking. Raw observations
include both exits, both pre-exec security receipts, bound child identities and
the positive controls.

## Running the exact product contract

Build immutable default and semantic products using the existing
`scripts/p7_stdio_build_receipt.py`, then build the `benchmark_adapters` runner
from the same source with the usual Cargo JSON compiler-artifact receipt.
The default product must report `features=[]`; the semantic product must report
`features=["semantic"]`. A `semantic-http` product does not satisfy that receipt.
Use private evidence directories and the original fixture/tool deadlines.

For each product, set `CODECORTEX_BENCH_BINARY`, `P7_017_PACKAGE_KIND`,
`P7_017_BUILD_RECEIPT`, and `CODECORTEX_BENCH_OBSERVATIONS`, then run:

```sh
python3 scripts/p7_offline_network.py --receipt /absolute/new-parent-receipt.json -- \
  /absolute/benchmark_adapters-runner \
  p7_offline::real_stdio_default_disabled_semantic_contract \
  --ignored --exact --nocapture
```

The outer wrapper explicitly supplies `P7_017_NETWORK_POLICY` and
`P7_017_NETWORK_GUARD` to the test runner. The real product still receives only
the original fixture environment. Receipt paths must not already exist.

Scoped launcher tests use
`python3 -m unittest discover -s scripts/tests -p test_p7_offline_network.py -v`.
The procfs helper is also independently compilable with
`rustc --edition 2021 --test crates/cc-eval/tests/support/p7_procfs.rs`.

## Acceptance boundary

The original two `/proc` ENOENT failures remain in
`artifacts/checkpoints/p7-017-private-build-offline-gates-20261007/`.
The new helper controls and launcher's positive controls do not themselves
complete P7-017. Acceptance additionally requires both exact products to finish
the full original tool matrix and its task dependencies. The implementation
does not edit the task ledger or claim those product runs before they occur.
