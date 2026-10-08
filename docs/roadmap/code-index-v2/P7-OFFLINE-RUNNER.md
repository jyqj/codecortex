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
seccomp filter before exec**. A product cannot swallow its own EPERM, but a
surviving parent can swallow a descendant's SIGSYS. Exit status alone therefore
does not establish zero attempts for the product tree.
The filter denies network socket operations and io_uring, rejects alternate
ABIs with process termination, closes inherited socket descriptors, and rejects
socket standard streams. The sole creation exception is anonymous
`socketpair(AF_UNIX, ..., 0, ...)` for local signal IPC. It supplies no external
address; ordinary Unix `socket`, `bind` and `connect`, other socketpair domains
and nonzero protocols remain denied. Trace acceptance admits only successful
`SOCK_STREAM` pairs with optional `SOCK_CLOEXEC`/`SOCK_NONBLOCK` flags, records
their exact calls separately as `local_anonymous_ipc`, and does not ignore
other socket operations. The policy changes only the launched process and its
descendants.

Two sacrificial exec children must print that they reached their IPv4/IPv6
socket probe and then terminate with SIGSYS. An earlier crash or normal exit
does not pass. Their receipts and the product pre-exec receipts identify the
launcher script, Python executable and actual exec target separately. The
original product build-receipt check is unchanged and runs again after the
matrix; the launcher is never presented as the product binary.

The runner retains ownership of each actual child and requires normal exit 0
within the original five-second close deadline. Raw observations include both
exits, both pre-exec security receipts, bound child identities and the positive
controls; their `network_socket_attempts` remains null until the separate trace
verification succeeds.

`scripts/p7_offline_trace.py` runs the unchanged product matrix under `strace
-ff` with process, network and io_uring calls selected. The verifier binds each
of the four product roots to its namespace-mapped PID and the successful exact
product `execve`, then follows every clone, fork, vfork and thread edge. It also
includes launcher children created before exec, so an overlapping helper cannot
escape observation. Every trace must have a complete terminal event. Missing
child files, unknown records, truncated exec witnesses, PID reuse, namespace
changes, overlapping product trees and SIGSYS in a product descendant fail
closed. Unknown selected syscalls are rejected. All selected network attempts
count, including failed calls and calls whose error or child death is swallowed.
The two deliberate IPv4/IPv6 probes are excluded only after their exact execs,
socket calls and SIGSYS exits are observed outside every product tree.

Only a complete passing tree report records zero network attempts. This does
not claim enabled semantic networking, machine-wide isolation, or a syscall
trace when the host denies ptrace. Original tool deadlines remain unchanged.

## Running the exact product contract

Build immutable default and semantic products using the existing
`scripts/p7_stdio_build_receipt.py`, then build the `benchmark_adapters` runner
from the same source with the usual Cargo JSON compiler-artifact receipt.
The default product must report `features=[]`; the semantic product must report
`features=["semantic"]`. A `semantic-http` product does not satisfy that receipt.
Use private evidence directories and the original fixture/tool deadlines.

On a host that permits tracing its own children, run the real negative
controls and then each product matrix:

```sh
python3 scripts/p7_offline_trace.py controls --output /absolute/new-trace-controls
python3 scripts/p7_offline_trace.py run --package-kind default \
  --product /absolute/default-product/codecortex \
  --build-receipt /absolute/default-product/build-receipt.json \
  --runner /absolute/benchmark_adapters-runner \
  --runner-receipt /absolute/runner-identity.json \
  --output /absolute/new-default-matrix
```

Use `--package-kind semantic` and the matching separately built product for the
second matrix. The trace runner supplies the four environment variables and
the outer wrapper explicitly supplies `P7_017_NETWORK_POLICY` and
`P7_017_NETWORK_GUARD` to the test runner. The real product receives the original
fixture environment. Output directories must not already exist.

The runner receipt contains its original Cargo compiler artifact, executable
SHA256, and `source_before`/`source_after` snapshots from
`p7_build_identity.source_snapshot`, omitting only the `inputs` map. Take these
snapshots before and after compiling the runner and require equality. They must
match the product build receipt and current committed crate/Cargo content.
Full commands, tracer version/hash, binary/receipt/script hashes, unmodified
per-PID logs and observations are retained under the new output directory.

The real trace controls include a clean forked child that uses the actual libc
anonymous socketpair/read/write IPC boundary, and a child that attempts
an IPv4 socket, receives SIGSYS, and is waited by a parent that exits zero. The
second case must be detected by the verifier. Python's high-level socket
wrapper also probes socket metadata, so it does not stand in for mio's direct
libc signal-pair construction. Parser fixtures are separate and
cannot substitute for these real controls or the real product matrices.

Scoped launcher tests use
`python3 -m unittest discover -s scripts/tests -p test_p7_offline_network.py -v`.
Parser controls use `test_p7_offline_trace.py` with the same unittest command.
The procfs helper is also independently compilable with
`rustc --edition 2021 --test crates/cc-eval/tests/support/p7_procfs.rs`.

## Acceptance boundary

The original two `/proc` ENOENT failures remain in
`artifacts/checkpoints/p7-017-private-build-offline-gates-20261007/`.
The new helper controls and launcher's positive controls do not themselves
complete P7-017. Acceptance additionally requires real trace controls, both
exact products to finish the full original tool matrix, complete descendant
trace verification, and the task dependencies. The implementation
does not edit the task ledger or claim those product runs before they occur.
