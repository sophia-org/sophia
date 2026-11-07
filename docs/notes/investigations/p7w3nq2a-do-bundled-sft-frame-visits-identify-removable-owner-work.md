---
id: p7w3nq2a
date: 2026-11-06
kind: investigation
status: investigating
tags: [investigation, cpu, sft]
---
# Do bundled SFT frame visits identify removable owner work

## Question

Can the changing-pixel SFT path prove a removable whole owner traversal before
another scheduling candidate or native comparison?

## Method and evidence

Base: repaired ordinary-SFT control `39bbc7b500`. Reuse the existing component
reconnect fixture and SDK image setup, adding a real `FrameStream::submit_ring`
case. Evidence, including unsuccessful harness attempts:
`~/.local/state/sophia-sft-bundled-owner-proof-01`.

The passing test drives component transport input, real copied-image admission,
resource/candidate service, supplied projection into the existing backend test
target, simulated submission/completion and the real presentation publication
consumer. It verifies exact pixels, one Accepted, one Prepared, one Presented,
no credit at Prepared, credit at Presented and independent source settlement.
Transport service iterations and wire calls are reported, not asserted as a
fixed count: the real worker completes asynchronously.

## Finding and resolution

This is a transport/lifetime regression, **not a physical-owner pass proof**.
The unmodified production component-service entry retired this synthetic
connection; the fixture's supplied allocation/projection is not a full native
owner environment. That failed attempt remains in evidence. The passing test
explicitly supplies the role-service order and backend completion. It must not
be used to justify skipping component service, a publication flush or a timer.

Local source review found no existing device-free physical-loop driver. The
loop is include-composed inside `run_session_loop_inner` and owns native,
frontend, topology and lifecycle resources. `OwnerWake` counters are meaningful
only when driven by that real loop; creating one in a test is not equivalent.
No production scheduler was changed. The implementation gate remains open:
whole-loop proof needs a faithful injected-loop boundary, or an explicit change
to the measurement strategy, before a pass-removal candidate can be admitted.

Saved native evidence supplies a separate fact without another launch:
terminal-02 interior periodic receipt intervals cover 1800 completions per arm.
Control owner passes/completion are 7.701 and 7.687; terminal-only values are
6.765 and 6.797. These are interior periodic intervals, not the exact CPUwin
window; counter and timestamp reads are not atomic. They support fewer passes
in those intervals, not a causal assignment or a per-pass CPU price. The same
block's CPU difference remains [-70,+10] us/frame, inconclusive. Therefore
removing approximately one pass alone is not sufficient evidence of savings.

## Validation and remaining work

The focused default and SFT reconnect suites, imported runtime SDK checks,
scoped strict Clippy, formatting and layout results are retained in the evidence
receipt. This checkpoint adds tests and test-only accessors, not a production
optimization. No native run, adoption, installation or performance claim.
A full-owner scheduling rewrite and native ABBA remain gated on missing proof.

## Exact exchange count and combined-call proof, November 6

The next bounded seam is the transport exchange, not another inferred owner
traversal. Evidence: `~/.local/state/sophia-sft-exchange-proof-01`. The existing
`sft-bar` owner fixture at peer `d069d445` uses the actual retained-event client
and real transport/copy worker. Authentication and completion are supplied;
its one-millisecond driver sleep is not the physical owner scheduler. The host
dependency is `2fa54bd846`; production control `39bbc7b500` differs only by the
CLI log-output repair. The SDK is `fd9330b64`.

Successful socket sends were decoded into 9P messages, with request/reply tags,
fids and paths, then journal sequences (including D4E1 ring records). Both
24- and 512-pixel-high ordinary SFT cases send one staged FrameCommit and one
submit per frame: two Twrites and two Rwrites. The record is 368 bytes for the
first frame and 392 with predecessor retirement. These writes already use the
SDK's pipelined WritePair; they are **not two sequential round trips**.

Each later ordinary frame has Submitted, ResourceStatus, Prepared, Presented,
SlotReleased and predecessor ResourceReleased. In this capture they arrive in
four Rreads. ACKs are cumulative and can cross frame boundaries, so neither
ACK writes nor reads are fixed protocol costs. Terminal-only removes Prepared
but retains four event reads in this supplied-completion fixture; that is not
a native effect result. Setup and shutdown are excluded from these frame rows.

The concrete deletion candidate is a complete-record submission operation
that reuses the existing writer token and calls the existing staging and
submission implementation. It would eliminate the separate submit Twrite and
Rwrite: one request/reply pair, 58 wire bytes per FrameCommit. This is a wire
projection, **not an implemented endpoint, syscall reduction or CPU saving**.
Submitted, ACK, publication, source settlement and resource retirement remain.

`shell_frame_combined_submit_proof.rs` is a test-only adapter over the real
export. Four golden commit shapes produce the same accepted bytes, inbound
record and journal as the existing pair. Tests cover inbound/journal EAGAIN,
identical retry, changed-payload replay, covering ACK, replay after ACK,
malformed/foreign/over-limit refusal, clunk, revoke and assembly expiry. A
complete write at offset zero cannot extend an existing partial prefix, even
when the prefix is identical; fragmented writes must finish through the old
path. The live token is checked even when accepted custody survives clunk.
Accepted retry compares the bytes, not merely epoch/id/length. A late replay
after ACK retains the existing EALREADY/refused-writer behavior.

This discharges **export-local custody composition only**. Before any wire or
SDK enablement: specify optional post-negotiation discovery, authenticate the
new write with the ordinary request gate, retain the existing token/exclusion,
enforce offset and negotiated msize/iounit, define full-count/error replies,
and test Submitted-before-Rwrite, uncertain disconnect, EAGAIN backoff and
legacy fallback. Update the client model and Rust/C conformance together.
Fallback is selected before issuing a complete write, never after uncertain
custody. No new opcode, revision, node name or capability is allocated here.

The evidence receipt records tests, guard-removal failures, formatting,
Clippy and layout. Production behavior, SDKs and native packages are unchanged.
No performance candidate, adoption or new native comparison is qualified.

## Optional complete-write implementation, November 6

The next source checkpoint promotes that helper to the optional r13
`frame-submit` node under the existing export and request checks. The
[wire contract](../../sophia-frame-transactions-wire.md#optional-complete-write-submission)
now specifies discovery, one complete write, writer ownership, exact retry,
ACK reuse, and pre-send fallback. The SDK changes retain ordinary Submitted
and all frame/resource events; they do not select terminal-only notices.

The new bounded [custody model](../../../validation/tla/frame-submit/README.md)
checks paired and complete-write routes, changed-byte retry, token loss and
ACK-gated reuse. It does not modify the higher SFT Client abstraction, which
has no transport-write actions. Both SDKs may use the old route for an eligible
record; Rust currently does so for the first record after a refused writer
was clunked. That route choice is not a custody difference.

Export tests now exercise the production node. Independent Rust/C scripted
peers check one-write payload identity, discovery refusal, size fallback,
EAGAIN and short/error replies. The actual producer fixture subsequently passed all six cases. Its 16 SFT
FrameCommits have byte-identical payloads and the same frame-event kind order
as the held pair-path capture; each eliminates one Twrite/Rwrite and 58 bytes.
Both legacy cases retain their submission bytes and never probe the new node.
The local C/Rust host fixture passes all 12 cases. These use supplied authority
and completion, not a protected native session.

Evidence: `~/.local/state/sophia-sft-combined-submit-01`. The pre-signing proof
uses recorded temporary dependency paths, restored after each build; it does
not edit or relabel either verified SDK snapshot. Signed commits/imports remain
pending the signing cache. Native CPU benefit remains unmeasured.
