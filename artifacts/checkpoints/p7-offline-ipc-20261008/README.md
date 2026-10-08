# P7 offline: actual Rust spawn IPC failure and bounded fix

The original default and semantic jobs at `2313b808d3ca95ae9917f72f6898dfd81dbae2c8` failed before the product matrix completed. This record preserves both full job logs and the default artifact’s complete trace subtree. The downloaded ZIP SHA256 matches GitHub’s published digest. The contained real product binary SHA256 also matches its build receipt; no successful product execution or zero network-attempt result is inferred from that build.

## Observed failure

The Rust adapter thread created an anonymous `AF_UNIX SOCK_SEQPACKET|SOCK_CLOEXEC` pair, then its eight-byte `recvfrom(..., 0, NULL, NULL)` acknowledgement was denied with `EPERM`. The exact Rust compiler-source commit confirms that Linux `Command::spawn` uses this pair for exec errors or successful EOF. The kernel control reproduces both data and EOF failures with the old guard.

The complete trace separately proves that the IPv4 positive probe’s Python process was killed by its first ordinary `AF_UNIX` socket after Python `-c` exec, before reaching the required IPv4 marker. Its `ldconfig` helper had already exited normally. Python site/user-home/NSS startup is a source-based explanation, not a call-stack fact from this trace. Controlled launcher and probe interpreters now use `-I -S`; the next actual probe must still reach the exact IPv4/IPv6 syscall and die there.

## Scope of the change

The only new receive exception is `recvfrom` with length 8, flags 0 and both address pointers NULL. All send syscalls, ancillary FD transfers, ordinary addressable Unix sockets, IPv4/IPv6 sockets, `connect`, `bind`, and io_uring remain denied. `pidfd_getfd` is now denied as an external descriptor-import route. The existing kernel `socketpair` domain/protocol restriction remains; the verifier admits only STREAM/SEQPACKET and the exact CLOEXEC/NONBLOCK flags.

A numeric FD is insufficient evidence. Actual strace runs now retain microsecond timestamps and kernel socket identity annotations. Pair creation must show reciprocal endpoint inodes, and every admitted receive must refer to an earlier pair in the same complete product tree, with a matching protocol and visible peer. Unknown, named, foreign, reused or future endpoint identities fail certification. Every launcher child is still included, including helpers created before the actual product exec.

## Verification and remaining work

`tests-before-fix.log` preserves the actual old failures. `tests-after-fix.log` records 32 passing methods: 11 kernel launcher controls and 21 parser/falsification controls. Controls include wrong/unknown/reused FD identities, imported descriptors, addressed receives, wrong lengths/flags, timestamp omissions, future witnesses, actual IPv4/IPv6 process termination, and a child’s SIGSYS swallowed by a normally exiting parent.

These are fixture and observer results. P7-017 is still pending the next fixed-source CI run of both real product packages, all original 14 tools, initial/reopened processes, no semantic cache, full descendant traces, and predecessor acceptance. No ledger status was edited. `failure-diagnosis.json` binds source bytes, raw evidence and pending requirements.

The failure-diagnostic inline YAML under `p7-offline-filter-baseline-20261008` also strips the new timestamp prefix for matching only; it preserves the original timestamped raw line in diagnostic output and remains non-authoritative.
