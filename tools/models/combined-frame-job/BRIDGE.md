# Stage 4c bridge: runtime credit, renderer inbox and owner join

Status: proposed design for review, frozen before any 4c source edit. Written
by Claude on 2026-11-10 with Codex offline, after stage 4b (facade Release
cell, f6ad56eef) passed its validation (seal 304053a9…, acceptance
f607f51d…, both under ~/.local/state). It selects no source change by itself.
niltempus approved the plan that places 4c behind a default-off feature; the
plan-type decision below needs niltempus's read before source edits because
it changes public runtime types.

## Source facts at f6ad56eef

- sophia-runtime and sophia-backend-live share only sophia-protocol and
  sophia-wake. Neither depends on the other. The renderer's frame type,
  `PendingRenderedFrame`, is `pub(crate)` in backend-live (discovery.rs).
- The combined route on the validation `Job` is `Option<Route<ProbePlan>>`,
  concrete and cfg(test). `take_route` returns that concrete type.
- `IsbValidationWorker` appears in 17 runtime source files along the image,
  owned-ring, lock-image and draw submission paths. Session holds it inside
  `sophia_runtime::ShellIsbImageOwner` (shell_component_connections.rs:80,
  images.rs:43; gpu_content_proof.rs:259).
- `run_worker` (worker/service.rs) serves every output from one shared
  command channel. With experimental-shell-capture the channel is a
  `SignalSender` ringing a command `Wake`, multiplexed by `Captures::next`;
  without it, the loop blocks in plain `recv()`.
- Results reach each output through its own `reply` `SignalSender`, which
  rings the owner wake. `Deregister` drops the output state, returning that
  output's buffers and slots together; a facade dropped with a Retained
  combined cell therefore loses no slot, the same as the ordinary cell.

## Decision 1: how the plan crosses the runtime

TRANSFER.md fixes the plan as opaque `P: Send + 'static`, moved through the
validator envelope, never inspected, with no `Box<dyn Any>` and no parked-plan
channel. Because the runtime cannot name a backend type, P must be a type
parameter supplied by session. Options considered:

- (a) Generic worker. `IsbValidationWorker<P = NoCombinedPlan>` and
  `ShellIsbImageOwner<P = NoCombinedPlan>` with `NoCombinedPlan` an
  uninhabited runtime enum. Only the owned-ring submission path that admits
  combined jobs takes `<P>`; every other caller keeps the default and its
  signature. Session instantiates the backend's plan type. Feature off, the
  Route field and every combined branch are compiled out; the defaulted
  parameter remains in the type, with no value of it ever constructed.
- (b) A runtime-defined plan struct whose contents backend fills. Rejected:
  the runtime would have to know renderer types, or the struct is an erased
  box in disguise.
- (c) Plan parked on the backend at dispatch, pixels forwarded alone.
  Rejected by TRANSFER.md (half-joined state).

Recommendation: (a). Reach, to be confirmed by the frozen diff: isb_worker.rs,
the owned-ring path (epoch_registry/owned_ring.rs, owned_ring/frame.rs,
owned_ring/commit.rs, owned_copy.rs, owned_copy/admission.rs,
owned_copy/frame.rs), image_owner.rs, and the session holder. Lock-image,
draw and image-offer paths keep the default. backend-live exports one plan
type, `CombinedRenderPlan`, `pub` behind the feature.

## Decision 2: renderer inbox and wake

Per TRANSFER.md: one bounded(1) combined inbox and one bounded(1) combined
result channel per output, created at Register and dropped at Deregister.

- Inbox: a `sophia_wake::SignalSender<CombinedRenderJob>` sharing the
  command wake. The validator's `Route` holds this sender; a successful
  `try_send` is the forwarding move and rings the renderer.
- The non-capture build gains the same command `Wake` the capture build has.
  `run_worker` then clears the wake, drains the ordinary command channel,
  checks each output's inbox, and waits on the wake with the earliest parked
  deadline. No `recv()` without a deadline while a combined job is parked. This
  adds one notification per ordinary command in that build; the mechanism
  capture must count it, and nothing calls it unchanged.
- A combined job parks in its one position when no slot is free. A freed
  slot for that output serves the parked job before the next ordinary Render.
  Cancellation and the admission deadline are checked at dequeue, at each
  retry and before rendering; expiry returns an owned failure envelope.
- Result: a per-output `SignalSender<CombinedRenderResult>` ringing the same
  owner wake as `reply`. A combined result never enters the ordinary
  `result_receiver` or `in_flight` correlation.

## Decision 3: owner join in session

Session holds both halves for one CombinedPending row: the runtime
`Reservation` (credit, identity, forwarding witness) and the backend facade's
cell reservation (`reserve_combined_release`). Admission takes both or
neither. On a combined result the owner:

1. checks job, output, OutputClaim and native owner incarnation carried in
   the result against the row;
2. on a scene-only mismatch or refused install, calls
   `retain_combined_result` and requires retention before
3. `settle_combined` clears the validation credit, in the same owner turn;
4. on success, hands the lease to service 2 (plan step 3).

Facade visibility: `reserve_combined_release` and `retain_combined_result`
become `pub(crate)` together with an owner-facing `CombinedRenderResult` type
that is not the private `WorkerResult`, which fixes the private-interface
failure found in 4b validation-01.

## Order of source work after review

1. 4c-1, runtime: feature `experimental-combined-frame-job`; move Route,
   Identity, forward, reserve/submit/settle from tests/support into
   src/shell_content/isb_worker/combined.rs under
   cfg(any(test, feature)); add the defaulted P. Checkpoint and ProbePlan stay
   test-only. Feature off: the existing 16 combined tests and all controls
   pass; feature on: the same tests plus a build of the lane host.
2. 4c-2, backend: inbox, result channel, the wake in the non-capture build,
   parking, priority and deadline, with lost-wake and timeout tests in both
   capture configurations, and a deterministic test plan type.
3. 4c-3, session join with the row, both reservations and the retain-before-
   clear order, tested with that deterministic plan.

`CombinedRenderPlan`'s real contents are the provisional composition of plan
step 3; 4c carries it opaque and renders only the deterministic test plan.

## Not established

No real combined render, no scene or fallback policy, no install-already-
rendered operation, no native run and no CPU claim. Disconnected stays
unqualified until retirement proof.
