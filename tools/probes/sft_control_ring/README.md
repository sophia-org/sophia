# Commit-only control carrier: device-free mechanism checkpoint

Selected scope: z8cr4n2p, commit-only, pread-first. This standalone Linux test
crate is the first executable checkpoint, **not a complete carrier comparison**.
No production host, SDK, frame loop, public protocol allocation or feature changes.
There is no timing command and no CPU/frame claim.

## Boundaries and existing code reused

The producer owns a writable `sophia_linux_peer::ProducerRing` mapping. The
consumer holds its descriptor and uses two bounded `pread`s: a private 64-byte
header and at most `SHELL_FILE_MAX_TRANSACTION_BYTES` payload bytes. Admission
requires exact extent and GROW|SHRINK seals. Short reads, EINTR and EIO are
refusals, not retries that could stitch publications together. Concurrent peer
mutation can supply arbitrary bytes during capture; no atomic-snapshot claim is
made. Private header/body identities and the existing SFT revision-13 decoder
must agree before an ACK effect. Later mutation cannot alter that private copy.

One outstanding attempt uses one SQ and one owner-written, 80-byte CQ slot.
There is no peer-written CQ head and no head-poll bookkeeping. A conforming
client consumes the receipt before publishing its next sequence. Exact
host-local sequence validation refuses gaps/duplicates and refuses before u64
wrap. A hostile early successor may discard its own unconsumed receipt; it does
not establish client knowledge or semantic ACK. The next valid SQ attempt is the
implicit permission to reuse the transport receipt slot, never a pixel release,
frame credit or cumulative journal ACK. No deeper queue is needed here.

The receipt binds sequence, epoch, writer, submission, byte count and the
attempt's ACK word. Success separately echoes the applied ACK. Every refusal
has zero confirmation; EAGAIN may have applied the ACK but does not confirm it.
`Owner::process` reserves its pending slot before invoking the ACK callback.
`publish` represents handoff to the one CQ slot; withdrawal before that handoff
suppresses delivery without erasing the accepted watermark.

`custody.rs` is an **executable projection**, not production authorization.
Live/token, deadline, admission-room and publication authority are explicit test
inputs. The existing decoder and golden FrameCommit bytes are reused, but real
fid expiry, fresh pidfd checks, grant validation, journal ACK/recycle, capacity
and partial delivery are not wired into this projection. The ACK callback is
an ordering seam, not a journal implementation. No fake journal is substituted
and no parity claim follows from these tests.

The two-process fixture passes the producer SQ descriptor using the existing
SCM_RIGHTS receiver, captures privately, then has the peer destroy its mapping.
The owner processes the private request and publishes a sealed CQ plus eventfd;
the peer polls that descriptor alongside the owner's pidfd, preads a receipt and
checks client knowledge. Startup/control markers use a Unix socket. This is one
successful exchange, not a persistent peer/owner service loop or a 9P baseline.
The ignored child test is invoked explicitly by its non-ignored parent test.

No new unsafe code: the crate forbids it. Existing ProducerRing owns the mapping
and seal/Drop implementation; consumer input is never mapped. Existing runtime
or kernel invariants are not proved by this test crate.

## Checks and remaining obligations

Tests cover bounded/malformed capture, exact seals/extent, private-copy mutation,
short/error reads, ACK-before-later-refusal, receipt identity and decoding,
reservation before effects, original EAGAIN deadline and changed-body refusal,
expiry before/after acceptance, same-attach writer reopen, duplicate EALREADY
fail-closed, cross-carrier ACK serialization, withdrawal and unknown replay,
old-epoch refusal, peer exit without a doorbell, clear-before-check notification
and closed-work-descriptor refusal. Actual syscall tests are device-free.

Before any timing, the complete probe still needs both actual transports joined
to the **same production export/journal behavior**: 9P FrameCommit Twrite/Rwrite
versus SQ/CQ, with events/standalone ACKs on 9P in both. Full/held journal, offsets,
terminal reserve, reusable-fid authority, delayed/partial delivery and persistent
loop cleanup must be exercised there. Real wait-loop notification interleavings
must be tested in that adapter. These mechanism tests do not discharge those
obligations. Trace-to-model comparison is also outstanding; the TLA projections
are not a refinement of this code. Keep timing disabled until that review.

## Standalone gates

The crate has its own `[workspace]`, like d4_owned_ring. Root `cargo xtask check`
and root fmt do **not** cover it. From the Sophia worktree root:

```
cargo-slot sophia cargo fmt --manifest-path tools/probes/sft_control_ring/Cargo.toml -- --check
cargo-slot sophia cargo test --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml
cargo-slot sophia cargo clippy --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml --all-targets -- -D warnings
cargo-slot sophia cargo doc --offline --locked --manifest-path tools/probes/sft_control_ring/Cargo.toml --no-deps
```

Run the test gate through the documented isolated wrapper (umask 022, bwrap
hiding /dev/dri and /dev/input, session/SOPHIA_* environment unset), with a bounded
outer timeout. The probe has no device-opening path. Rustix 1.1.4 and the lock's
libc/bitflags versions match the held repository versions. Pools derive from the
worktree through cargo-slot; do not copy build outputs into evidence.
