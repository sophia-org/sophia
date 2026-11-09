# T4: worker-side native submission boundary

Source: `2c77581d8021fbbe1ea41149ce200b94796317d4`. This is a proposed
concurrent handoff, Category B: owner and renderer worker can interleave
authority withdrawal, resource publication and reclamation. It is not a bug
report against current Sophia. No Rust implementation or native run is selected
by this model result. M1/M2 remain deferred; file-wire custody is unchanged.

## Scope

T4 retains the owner-to-worker Render request and proposes removing the
worker-result → owner poll → DRM-submit handoff. It must preserve exact
native completion and all failure/cleanup obligations. One frame and one
physical head are modeled, with a possible authority generation change.
The existing SFT `base.tla` remains unchanged: Handoff and Submit independently
check Current(f), and close differs from revocation. This model starts at the
already-admitted, privately captured render request, so it neither proves nor
changes producer admission, copy validation, source release or terminal credit.

## Source map and scenarios

Paths below are relative to `crates/sophia-backend-live/src/` unless stated.

### 1. Authority check versus kernel submission

`production_session/native_scanout/persistent_native_scanout/singleton_tick.rs:33–70`
checks pending work, arms retirement/cursor state and constructs a borrowed
TargetPropertyDevice from owner-held card, properties and target generation.
`scanout/rendered_scanout/tracked_submit.rs:30–139` performs capacity checking,
submission and custody installation synchronously. Moving the ioctl breaks that
owner-serial interval unless a replacement serialization point is defined.

The canonical SFT contract says facts changes and revocation invalidate accepted
work's authority for new submissions; render handoff is not irrevocable submit
permission (`docs/native-frame-transactions.md`, authority section;
`validation/specula/native-frame-transaction/spec/base.tla:74–90`).

Model CheckAndReserve and KernelAccept separately. ApplyWithdrawal may run
between them in the unserialized variant. The protected variant assumes a common
gate covers both and all authority-changing paths. That gate does not exist in
the implementation. Its progress consequences are an open design question.

### 2. Submission outlives the worker reply and output facade

`scanout/rendered_scanout/exporter/worker/service.rs:86–124,199–207,306–327`
removes detached/deregistered outputs or outputs whose reply failed; shutdown
drops the worker's remaining local state. Actual GBM buffers live in its lease
map. This is not an identified defect: the current worker does not submit KMS.

After hypothetical worker-side acceptance, those paths must retain submitted
or uncertain ownership independently of the reply/facade. Existing
`scanout/rendered_scanout/custody.rs:21–32,92–115` supplies submitted/displayed/
retiring semantics, but `submission.rs:67–69,102–103` boxes owners as dyn Any,
not a transferable Send submission record. EGL/GBM Rc-owned resources stay on
the worker (`sophia-renderer-native-egl/src/gbm_platform/scanout/context.rs:30–45`
and `buffer.rs:2–19`). No unsafe Send conversion is proposed.

Model retained ownership separately from replyOpen and workerAlive. Submission
acceptance is distinct from userspace return. Drop-on-disconnect/stop controls
test why existing cleanup cannot simply be reused after moving submission.

### 3. Completion can precede submit-result consumption

`production_session/native_scanout/persistent_native_scanout/completion_wait.rs:89–108,126–158`
subscribes the card and establishes a hard-stall deadline from submitted_at;
rendering content instead requests short Service. `singleton_tick.rs:124–211`
installs exact submitted identity/content, timestamps and cursor settlement only
after the owner consumes the submission report. Removing its wake requires
pre-armed completion coverage and a durable exact submission record.

The model allows KernelFlip before PublishAccepted. Its ledger premise must be
implemented as enough retained identity/resource state to preserve an early
callback, not merely a Boolean or an eventually delivered result. Owner callback
validation and retirement remain required. A rejected commit needs a failure
wake because no page flip follows it; that progress path is not proved here.

### 4. Withdrawal progress while submission is uncertain

`sophia-session/src/live_session/owner_loop/lifecycle/seat.rs:237–295` gates
disable acknowledgement on retained native disposition and refuses timeout.
`native_owner_retirement.rs:35–40` forbids device work with inactive authority.
Backend `persistent_native_scanout/shutdown.rs:52–67` requires submitted,
displayed, prepared and cleanup owners to drain; joining a worker is not enough.

A stalled thread holding the proposed gate can prevent effective withdrawal.
RequestWithdrawal is distinct from ApplyWithdrawal; a request cannot be reported
as applied while the old worker may still submit. The progress probe includes
fair enabled submit/withdraw actions but permits a worker to remain stalled.
It does not assume the worker exits, the ioctl returns, or physical detach occurs.
This exposes an unresolved design obligation, not a new current-runtime bug or
proof that current owner-side blocking ioctls have a stronger progress guarantee.

## Requirements and configured checks

| Property | Purpose | Scenario |
|---|---|---|
| FreshSubmit | No accepted submission after effective authority withdrawal | 1 |
| PhysicalCustody | Pending or displayed frame retains physical ownership | 2 |
| SubmittedAccounted | Submit has a durable exact owner independent of result delivery | 2, 3 |
| CompletionArmed | Successful silent submit is covered before owner result processing | 3 |
| ExactCompletion | Foreign completion cannot settle the frame | 3 |
| WithdrawalEventuallyApplied | Probe whether the gated design progresses with a stalled worker | 4 |

See brief-coverage.md for every configuration and the expected negative controls.
Safety success proves these abstractions only. It does not prove the ledger's
Rust representation, safe thread transfer, the gate implementation or physical
DRM behavior. Counters, CPU saving and the removal of a Service wake are unproved.

## Required Rust/device-free checks before a native package

- Exact typed submission ticket, including owner/output/head/target/frame and
  resource capacity; stale replacement and duplicate ticket refusal.
- Current-authority gate and interleavings after rendering, before ioctl and
  after acceptance; no cached authorization or weakened active/ingress reads.
- Success followed by dropped reply, facade detach, worker panic and shutdown;
  retained uncertain ownership until exact physical cleanup proof.
- Flip before result consumption, wrong callback, commit refusal and cleanup
  failure; no early Presented, predecessor destruction or slot reuse.
- Seat/topology disposition while a submit is possible or stuck. Existing
  renderer_worker_shutdown and native_retirement fixtures must remain valid.

These are implementation tests to write after a mechanism is selected, not
claims discharged by the finite model. No trace spec is generated: no T4 Rust
transition exists to instrument or validate yet.

## Exclusions and coverage limit

No cross-head atomicity, mirrors, direct scanout, cursor transaction arbitration,
wall-time bounds, memory ordering, device-loss recovery, kernel event encoding,
or successor-frame reclamation proof. PhysicalDetach abstracts a proven physical
release, not a timeout or requested shutdown. Those exclusions prevent adopting
the model as a production custody protocol. Frame cancellation before renderer
handoff remains the existing contract and is outside this post-handoff episode.

Read current connected submit/result/readiness/retirement paths and two relevant
history checkpoints: `cc92b86bb` (result wake) and `bacfdb207` (retain stalled
renderer completions). This is bounded new-design analysis, not an exhaustive
bug archaeology or historical issue audit; no new defect in those commits is
asserted. The existing shared-worker and per-output-atomic-owner briefs provide
resource context but do not prove this new handoff.
