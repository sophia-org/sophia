# Complete-write custody projection

`FrameSubmit` models one authenticated, negotiated attach with two submission
ids and two payloads. `Combined=FALSE` requires Stage before Accept;
`Combined=TRUE` also permits direct Accept, while retaining Stage for per-record
fallback. Both SDKs may select the old route after a writer reopen. The existing
SFT Client model abstracts submission admission, not its two 9P writes, and is
unchanged: this model covers the lower file-custody seam.

Run from this directory with TLC2 2.19 (08 August 2024):

```
java -cp /path/to/tla2tools.jar tlc2.TLC -config FrameSubmit.cfg FrameSubmit.tla
```

Run once more with `Combined=FALSE`. Negative controls set `CheckBytes`,
`RequireWriter`, or `RequireAck` to FALSE, one at a time; they must violate
ExactCustody, LiveWriterOnSuccess, and AckBeforeReuse respectively. The checked
configuration sets all three TRUE. Terminal states are intentional, so deadlock
checking is disabled. This is bounded safety checking, not a liveness proof.

Source mapping:

- Stage / Backpressure: reusable transaction assembly and EAGAIN retention.
- Accept / Retry: `export/transaction.rs::submit_frame_record` and the existing
  `submission.rs::submit_transaction`, including exact accepted-byte retry.
- Observe: either complete Rwrite or Submitted establishes known file custody;
  journal observation remains required for ACK in the concrete implementations.
- Ack / Recycle: cumulative journal ACK releases accepted bytes for reuse.
- Clunk / Open: token expiry or clunk cannot cancel accepted custody; a new
  writer needs that custody drained first.
- Disconnect: no new submission/replay; knowledge already acquired is retained.

Abstraction limits: authenticated entry and well-formed complete records are
assumptions here. Sequence values are collapsed to monotone submission ids,
clock deadlines to a clunk/expiry transition, ACK eligibility to Observe, and
resource/journal capacity failure to retained staging. Byte parsing, actual
ACK coverage, offsets, deadlines, request permissions and independent reply
orders are checked by export/socket/SDK tests, not proved by this model.
It does not establish Rust/C refinement, rendering correctness or CPU savings.

Both modes reach the same custody states: direct acceptance adds a shortcut
transition, not new state values, and the retained-staging action coincides
with Stage in this projection. Consequently this checks common invariants,
not transport equivalence or an implementation proof of EAGAIN/error handling.
The guard-removal controls are mode-independent. No error-code parser or
staging-overlap mutation is claimed here; those are concrete test obligations.

## Lean commits (`LeanPrefix`)

`LeanPrefix` projects the bit-20 option onto one attach: the header sequence
word of a complete `frame-submit` FrameCommit is a cumulative ACK the host
validates and applies before admission; an admitted commit is recycled at
once (nothing retained, no Submitted appended); the client takes custody and
confirms the word from the exact Rwrite alone; a lost reply is a disconnect.
The first acceptance (a paired Open, sequence 1) stays retained until covered.
Run `LeanPrefix.cfg` as above. Negative controls, one at a time, must violate
the named property: `ValidateEpoch` (EpochBinding), `ValidateAck`
(AckValidation), `RequireWriter` (WriterBinding), `RequireCoveringAck`
(AckBeforeReuse), `KnownOnAnyReply` (KnownIsReal), `ConfirmOnError`
(ConfirmedIsApplied), `EmitSubmitted` (NoSubmitted); `IdleSpec` with
`PROPERTY IdleProgress` and `CHECK_DEADLOCK FALSE` passes, and fails without
`Standalone`. Bounded safety only; no refinement or CPU claim.
