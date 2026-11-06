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
