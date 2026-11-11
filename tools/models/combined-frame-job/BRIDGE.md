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

## Corrections after 4c-1 (9e7723d45)

- Dependency direction. The source facts above say runtime and backend-live
  share only protocol and wake. That holds for direct dependencies only:
  backend-live already depends on sophia-renderer-live, which depends on
  sophia-runtime, and runtime is a backend dev-dependency. An optional
  direct dependency on sophia-runtime behind the feature therefore adds no
  new edge to the build graph and no cycle.
- Inbox type. 4c-1 landed `CombinedInbox<P>` as a `SyncSender` plus a wake
  handle, not a `SignalSender`: a `SignalSender` rings on drop, and a route
  dropped on a failed forward must not wake the renderer. 4c-2 changes the
  handle from `Notifier` to `sophia_wake::WakeSlot`. The renderer's command
  wake is attached by the worker thread after spawn (as
  `Captures::new` does today), so a facade attached before the thread runs
  has no `Notifier` to copy; `WakeSlot` is the type the backend already uses
  for every lazily attached wake (`result_wake`, `command_wake`). A forward
  before attachment rings nothing, and the worker's first iteration finds the
  frame because it checks inboxes before its first wait.

## 4c-2 design, frozen before source edits

Feature: `experimental-combined-frame-job` in sophia-backend-live,
`["dep:sophia-runtime", "sophia-runtime/experimental-combined-frame-job"]`,
added to session's feature of the same name. Feature off, every hunk below
is compiled out except the unchanged argument pair passed to `Captures::next`.

Types, in a new `exporter/worker/combined.rs`, all behind the feature:

- `pub struct CombinedRenderPlan` with private fields: admission `deadline:
  Instant`, `target`, `frame: PendingRenderedFrame`, `preferred_modifiers`,
  `output_format`; a `pub(crate)` constructor. 4c-3 decides the session-facing
  constructor; step 3 decides the real provisional contents.
- `pub struct CombinedRenderResult` with private fields `token:
  CombinedToken`, `pixels: ValidatedIsbPixels`, `outcome`: `Rendered(WorkerResult)`,
  `Expired`, or `Failed(detail)`. It is the owner-facing type decision 3
  names; `WorkerResult` stays private inside it.
- Per output, `WorkerOutputState.combined`: the inbox `Receiver` (bounded 1),
  at most one parked `ForwardedValidation<CombinedRenderPlan>`, and the result
  `SignalSender<CombinedRenderResult>` (bounded 1) ringing `result_wake`.
- Facade: one field holding the inbox `SyncSender`, a clone of
  `command_wake`, and the result `Receiver`; `combined_inbox()` builds a
  `CombinedInbox` for one submission, `take_combined_result()` is a
  `try_recv`. Both registration halves travel in `WorkerCommand::Register`
  under the feature.

Scheduling is a pure per-output function, `serve(state, now, render)`, with
the render step passed in so tests drive it deterministically:

1. A parked job is checked first: deadline passed, `Expired` envelope;
   otherwise `render` is attempted, and a missing slot keeps it parked.
2. With nothing parked, one frame is taken from the inbox: deadline checked
   at dequeue, then `render`, parking on a missing slot.
3. The deadline is checked again immediately before `render` runs.
4. While a job is parked the inbox is not read, so one output holds at most
   one combined job, which the facade's single Release cell already enforces
   at admission. A full result channel is a contract violation: the envelope
   is dropped and counted, never blocked on.

`run_worker` calls `serve` for every output at the top of each iteration,
after the wake is cleared and before the next command is taken. A freed slot
(Release, a failed render's own slot) therefore serves the parked job before
any later ordinary Render. The earliest parked deadline bounds the wait.
Cancellation is output detachment: `outputs.retain` at the loop top drops a
detached output's state, inbox and parked job together with its slots and
leases, as Deregister does for ordinary work; no slot was taken by a parked
job, and the lease of a rendered result still in the channel is the
worker's own record, freed with that output state. Disconnected stays
unqualified (decision 3).

Wake, both builds, feature on:

- `WorkerControl.command_wake` and the `SignalSender` command wrapper move
  from `cfg(experimental-shell-capture)` to `cfg(any(capture, combined))`;
  `shutdown` rings it under the same cfg.
- Without capture, `run_worker` creates the command `Wake` at start, attaches
  it to `command_wake`, clears it at the top of each iteration, and replaces
  `recv()` with `try_recv`, then `Wake::wait(earliest_deadline)`. Wake
  creation failure ends the worker, as `Captures::new` failure does today; a
  clear failure falls back to a 1 ms `recv_timeout`, as `wake_failed` does.
- With capture, `Captures::next` gains `(must_poll, deadline)`. Feature off
  passes `(false, None)`, which is today's behaviour; feature on passes
  `(true, earliest)`, so the blocking `recv()` shortcut is never taken and the
  poll timeout is the earlier of `follow_up` and the parked deadline.
- Every ordinary command now rings the wake in the non-capture build. The
  mechanism capture must count that notification.

Tests (backend `tests/support/combined_frame_render.rs`, mounted from
`combined.rs`, the only new layout exception):

- `serve` with a deterministic render step: a parked job takes the next freed
  slot before a later ordinary request; expiry at dequeue, at retry, and
  immediately before render, each returning the token and pixels intact; the
  inbox is not read while parked; a full result channel drops and counts.
- `run_worker` with no device, in both capture configurations: a frame
  forwarded after the worker is idle produces a `Failed` envelope and rings
  the owner wake (lost wake); an expired frame produces `Expired` (timeout);
  shutdown while a job is parked ends the worker.

Mutants named before build: `serve` skips the parked job (priority test),
deadline check removed at retry (retry expiry test), non-capture build waits
in `recv()` (lost-wake test, no-capture run), `must_poll` ignored (lost-wake
test, capture run), inbox read while parked (bound test).

## 4c-3 facts at 30979b4fc, before any design is frozen

4c-2 was accepted on 30979b4fc (receipt f2aa281f…, acceptance 86a9700f…).
A read-only map of the join found:

- Session's validator is `ShellIsbImageOwner.worker`
  (runtime shell_transport/image_owner.rs:20), fixed to the default plan
  type, built per shell-component connection (session
  shell_component_connections/images.rs:43). Its owner turn,
  `ShellIsbImageOwner::service` (:119), reached from
  `ShellComponentConnections::turn_connected` (:685), takes no backend handle.
  Owned-ring admission and submission happen inside runtime
  (owned_copy/admission.rs, isb_jobs/owned_copy.rs).
- Renderer facades are per head, private to the backend exporter
  (exporter/discovery.rs:52), reached only through `LiveProductionNativeScanout`
  (`pub`, keyed by `OutputId`). Mirror outputs have one facade per head. The
  shell-capture forwarders are the precedent for the hop pattern (`pub` on
  the scanout, `pub(crate)` on the exporter, facade method) and refuse
  outputs spanning more than one card group.
- `CombinedRenderPlan` and `CombinedRenderResult` are not nameable outside
  `worker::combined`; render results move only on a scanout tick.

Decision 3 assumed session holds both halves. It can, but only by: (1) new
`pub` combined methods on `LiveProductionNativeScanout` with exporter
forwarders, restricted to single-head outputs, and the two types re-exported;
(2) the combined reservation and submission made where owned-ring frames are
admitted, which is inside runtime's owner turn and needs the provisional
plan that plan step 3's service 1 builds.

Options for 4c-3, for niltempus:

- (a) Forwarders only: (1), plus a session-side pending row that reserves
  both halves, drains the result on the owner turn, retains the rendered
  lease before settling the credit, and releases. Tested with a combined
  validator built in the test and the deterministic plan; no owned-ring
  admission change. The join is real, but no production frame enters it.
- (b) Fold 4c-3 into plan step 3: build (1) and (2) together where service 1
  dispatches, so the first join already admits real owned-ring frames, with
  `ShellIsbImageOwner<P>` and a native handle passed into the owner turn.

Recommendation: (a). It keeps the cross-crate API change reviewable on its
own and gives step 3 a tested join to call; (b) puts the API change and the
provisional-composition obligations into one diff.
