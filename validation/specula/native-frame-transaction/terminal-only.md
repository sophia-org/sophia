# Proposed terminal-only SFT: model checkpoint

Status: design experiment, not implemented or negotiated. The r13 wire contract
and all existing models remain unchanged. No revision or capability is allocated.
This follows the approved protocol/model-first investigation, not an enablement.

## 1. Source and scope

Category A: message-passing ownership protocol, with asynchronous copy and native
completion. Reuse `spec/base.tla` and `spec/Intake.tla`; do not duplicate resource,
revocation, credit, source-ticket, mirror or refusal machinery.

Sources are host `29ee6d99f` and peer `67fd3f7e`. On the captured changing-pixel
path, `tools/sft-bar/src/sft.rs:257-280` observes Prepared only to log it. The loop
exit at line 362 requires accepted resource, Presented, source release,
predecessor release and frame credit, not Prepared. The imported Rust
`frame_stream.rs:317-333` accepts a matching terminal without first observing
Prepared; observing Prepared does make a subsequent request refusal a protocol
fault. That distinction must remain enforced by host admission/terminal rules.

The accepted `sft-profile-01/changing` client journal contains 2,120 Prepared
records and 2,120 terminal records, one of each per generation. This establishes
the record exists on the measured path. It does not establish a separate ACK,
wire wake, input turn or physical owner pass for that record.

## 2. Scenarios

S1: an explicitly negotiated mode omits only the nonterminal Prepared wire event.
`content/submission.rs:34` must still call the internal preparation transition.
`candidates/presentation.rs:5-43` must still set `candidate.prepared`, validate
identity, start the presentation deadline and consume the reserved response
obligation. `presented_inner` still requires that internal state. Do not simply
skip `content_prepared`, and do not defer its event until presentation.

S2: no early credit, source reuse or predecessor retirement. Source capture and
its independent release remain unchanged. ResourceStatus/SlotReleased can occur
before terminal; suppressing Prepared does not combine those obligations.

S3: complete validated scene and fresh authority still precede every native
submission; mirrors retain independent physical leases. Close/revoke/expiry
must not strand accepted work, replay into another epoch or replenish a closed
stream. Reopen remains drain-before-reopen.

S4: input fragments, full-budget continuation, nonfatal refusal and publication
backpressure still progress. Keep the conservative existing admission reservation
count in this first model. At internal preparation, release the now-unused
Prepared reservation without appending a queue record. This decreases occupancy,
not frame credit. Changing admission capacity/refund tables is a later, separate
implementation proof; no larger concurrency claim follows.

## 3. Model delta

`TerminalOnly.tla` replaces only PublishPrepared with LocalPrepared in its own
Next and fairness definitions. All other base actions are reused. Base's
PreparedOrder explicitly requires a wire notice, so the proposed mode instead
checks PreparationSafe (handed off and all heads prepared before Presented).
PreparedBeforeSubmit checks the obligation has been consumed before any submit;
NoPreparedEvents checks the new mode's queue. The explicit ordering invariant
and its guard-removed mutation close an independent review finding: checking
only the final presented state could miss an early submit before Terminal
cleared that obligation. The old PreparedOrder is not
silently redefined or claimed preserved.

`TerminalOnlyIntake.tla` replaces only the prepared publication action by an
internal obligation release with the same no-reader/no-parts prerequisites.
Its FIFO contains both refusal ids and strings; NoPreparedEvents compares their
TLC string representations to avoid an invalid cross-type equality. The first
check exposed that predicate type error; it was a model error, not a runtime bug.

The inherited models are bounded abstractions, not instruction-level runtime
traces. In particular source obligations and terminal transfer are abstracted
as in the existing checkpoint. These results do not prove wire byte accounting
or real control/journal reservation arithmetic.

## 4. Checks and falsification

Run `tools/check_terminal_only_frame_model.sh` with the existing pinned Specula
jar. Checks reuse the existing finite frame/head/credit configurations: ordinary,
mirror, two credits, reopen, fair lifecycle progress, intake safety and fair
intake progress. No state/depth cutoff is applied. Expected failures inject
credit return at LocalPrepared and a Prepared FIFO entry. They must respectively
violate CreditSafe and NoPreparedEvents. A third mutation removes only the
Prepared-obligation guard from Submit and must violate PreparedBeforeSubmit. Passing finite enumeration is not an
unbounded proof or a latency/performance result.

## 5. Coverage self-audit

| Scenario | Enabled configurations and properties |
| --- | --- |
| S1 internal preparation, no wire Prepared | TerminalOnly/Mirror/TwoCredits/Reopen: PreparationSafe, PreparedBeforeSubmit, NoPreparedEvents, SceneSafe; WireMutation and OrderMutation must fail |
| S2 exact credit and independent ownership | Same four: CreditSafe, SourceSafe, PhysicalSettlement, RetirementSafe; CreditMutation must fail |
| S3 authority, close and reopening | Same four: AuthoritySafe, JournalSafe, StreamBound, ReopenSafe, TerminalSafe; Liveness: Settles, ResponsesDrain |
| S4 bounded intake/refusal/backpressure | TerminalOnlyIntake and Progress: BudgetSafe, CreditSafe, ReaderSafe, AdmissionSafe, RefusalSafe, SettlementSafe, FragmentsEnabled, ContinuationEnabled; InputDrains, AcceptedSettles |

No implementation trace spec is generated yet: there is no negotiated
terminal-only implementation to trace. Existing r13 traces cannot be relabeled
as this mode. This is an explicit open gate, not a successful trace validation.

## 6. Implementation and measurement gate

Before any native package: specify negotiation and C/Rust parity; pin internal
Prepared/deadline and exact response-reservation release (including refusal,
renderer failure, revocation and backpressure); update the runner's event
expectations explicitly. Instrument real copied-frame SDK/owner controls for
records, coalesced ACKs, wire wake writes and input/flush turns. Preserve default
r13 and all fallback modes. No unconditional Prepared suppression is authorized
by this model result alone.

Compare one exact frame with and without the *negotiated* event. If it removes
only a buffered record and no meaningful service work, stop before native ABBA.
If it removes material work, use the normal matched comparison with combined
CPU primary and independent cadence/latency/identity/cleanup gates. No inferred
CPU saving or extrapolation from record count; no native launch at this gate.

This is different from shelved W1 delayed publication: no remaining event is
held or its failure ordering delayed. It is also not a standing-credit rewrite;
SFT's existing credit and source/retirement separation remain.
