# Direct validated-job transfer: proposed contract revision 5

## Selection and objectives

Niltempus approved validation on the existing validation thread followed by a
direct validated-result handoff to the existing renderer thread. The target is
one fewer owner dependency service, NOT one fewer worker handoff. Combined
Session+peer CPU/frame and security decide adoption. Count the new handoff,
retained bytes/credits, discarded renders and busy-output work; do not price just
removed owner visits. No extra frame credit, buffering or validation on the owner.

This is a contract for review, not implemented behavior. Current validate()
returns to IsbValidationWorker::completed; current NativeGbmRendererWorker::poll
owns renderer correlation. Neither can simply be bypassed. The new routing must
be paired to one exact pending entry and maintain both sides' correlation.

## Reservation and routing

At service 1, the owner allocates one CombinedPending row keyed by connection,
grant, output, candidate, job and worker incarnation. This is the SOLE authority
for the combined operation's credit and disposition. It owns the existing
validation-credit reservation (one of four shared slots), byte/resource charges,
four journal obligations, fixed deadline and dedicated renderer inbox/result credits.
These are owned tokens, not duplicate counters. The existing validation pending
row must refer to this row or become part of it; it may not independently release
a forwarded completion. No synthetic IsbValidationCompletion may enter today's
expect-based take_completed path without its matching owned row.

Reserve one renderer-inbox entry for the experimental profile before dispatch;
there is at most one combined job pending for that output. This reservation is
NOT an EGL frame slot and NOT an exclusive render-in-flight claim. Holding the
renderer idle throughout validation would defeat validation isolation. Other
existing work may progress while validation runs. Admission refuses the new
combined path if owner-visible renderer state is already in-flight, stalled or
quarantined; that observation is not a promise it stays idle until handoff.
A later contention is handled explicitly below. Preserve existing errno/Rwrite
behavior: no new error code is introduced by this draft.

Use a dedicated bounded(1) combined input channel and bounded(1) combined result
channel per output. Ordinary commands cannot consume this capacity; do not pretend
the shared 32-entry command channel offers reservations. A combined result never
enters the ordinary WorkerResult receiver or its in_flight correlation check.
The owner drains the combined result route even when ordinary in_flight is None,
with exact job, OutputClaim and retirement_owner_identity checks. One admitted
combined row reserves both capacities until settlement; channel Full is then an
internal protocol violation, not normal host contention charged to the peer.

A host-created result route and immutable job identity travel with the payload.
Only a successful move into the bounded inbox transfers ownership; a failed or
full send returns the entire payload to its sender. No blocking send, retry spin,
new helper thread, or fresh capacity allocation on failure. Queue Full despite
reserved capacity is an internal invariant failure: return a classified failure
and retain ownership for settlement, never drop bookkeeping to force progress.

## Plan transport and accounting

Move the frozen renderer plan through the validator envelope, rather than adding
a parked-plan channel and a half-joined state. In sophia-runtime the carried plan
is a generic opaque P: Send + 'static, with no other bound, methods, Debug or
Box<dyn Any>. The validator moves it and never inspects it. Prototype review must
check this type boundary. The owner retains the composition record, display list
and transform in CombinedPending for installation; the result returns identity,
validated backing and render lease/failure, not a worker-authored display list.

Permitted plan references are value descriptors, immutable identity keys and
already-validated predecessor CPU content leases. No scanout lease, slot token,
GPU/device handle, callback or mutable authority enters the validator. Admission
must prove this eligibility; unsupported composition stays on the existing path.
Validation failure returns the opaque plan to the owner for disposal. Unwind and
queue-destruction tests must prove that dropping permitted references cannot run
device cleanup on the validator. Moving the plan adds no pixel copy; retained
predecessor bytes and metadata construction still count toward total cost.

At renderer dequeue bind private validated pixels into the plan's placeholder
only when declared width/height/stride/extent equal validated values. A mismatch
returns the owned failure to the owner. Service 2 repeats this check and owns the
peer-visible Malformed classification. The worker never emits that outcome.

The proposed owner-local CombinedFrameJobs table in ShellIsbImageOwner holds the
CombinedPending rows. IsbValidationWorker remains the single allocator of its
shared four-slot quota; reserve_combined transfers the exact credit token into
the row, leaving a tagged reservation in the allocator, not a second freeable
credit. Forwarded success cannot use today's take_completed or IsbImageJobs::receive.
Add an explicit settle_combined(token, proof) path: proof is the exact combined
result or the qualified retirement proof below. It consumes that reservation
once, then drives the matching resource-admission row through reconciliation.
An unknown/duplicate result changes no credit. Pending resource settlement remains
owned if reconciliation is blocked; freeing validation capacity does not free
bytes or source slots. This is a proposed accounting API, not an existing one.

Before forwarding, validation or send failure returns via the ordinary validator
completion route with the opaque plan retained for the owner. take_completed
supplies the one completion proof for that case; the combined table records that
consumption and cannot also free its token. Forwarding success changes the proof
route, never frees the slot. No synthetic completion is injected into the old
expect-based path.

Each row owns a one-way atomic forwarded flag, initially false. The validator
sets it with Release after successful move into the dedicated input channel;
the owner observes it with Acquire during existing turns, with no success wake.
The flag is an ownership-routing fact, never pixel or submission authority.
A true flag plus matching CompletedNativeOwner and the qualified destruction
below permits retirement settlement. A false flag retains the credit while the
validator progresses to setting it or returns via take_completed. Validator death
inside that interval remains NotifyOnExit's I/O failure; it does not free credit.
An exact renderer result itself proves transfer even if it arrives before the
flag; consuming it once makes a later flag store irrelevant. Tests must cover
retirement/result/flag reorderings, ordinary-turn observation after retirement,
and validator death before/after send. This closes the missing forwarding witness;
the existing Transfer.tla abstracts these transitions and does not prove the
atomic implementation or scheduling latency.

## Payload ownership and cancellation

| Phase | Owns source ticket | Owns validated backing / speculative lease | Completion route |
| --- | --- | --- | --- |
| validating | validation job | validation job after capture | failure to owner, success to renderer inbox |
| inbox | source finished fact only | bounded inbox envelope | receiver or sender retains on failure |
| rendering | no source read ticket | renderer job and its ordinary slot/lease | result to owner |
| result pending | no source read ticket | result envelope or renderer-owned lease token | owner takes once |
| settled/retired | none | existing resource/scanout owners | existing journal and Release paths |

The source ticket is dropped after private capture, preserving its no-future-read
proof. Its release is NOT published from the validator. Validated backing remains
charged while queued, rendering, awaiting owner consumption or withdrawn.

Cancellation is a host-owned one-way stop flag, not authorization. Check it at
existing validation chunk boundaries, before enqueue, at renderer dequeue and
before rendering. Cancellation racing a check may permit bounded private work;
only service 2 can authorize publication. No worker can clear cancellation or
extend a deadline. Late results retain exact original epoch/incarnation identity.

On failed handoff the validator returns failure and the owned payload through
the reserved owner result route. On cancellation while queued, the renderer
returns the payload without rendering. On cancellation after rendering starts,
finish or use existing renderer failure handling; do not free GPU references on
receipt of a flag. Owner withdraws authority promptly even if workers stall.

## Busy renderer, actual slots and fairness

The renderer consumes at most the reserved single experimental envelope. Existing
control commands (Release, deregistration/shutdown) must remain serviceable.
Actual frame-slot acquisition remains on the renderer, with the existing three
slots. A ready combined job never reserves a fourth slot or steals scanout state.

If the renderer is busy or no slot is available, keep the envelope in its one
bounded pending position. Retry only after a relevant completion/Release, not
by waking the owner or spinning. A freed slot goes to the parked combined job of
that output before the next ordinary Render for it; no stream of ordinary renders
may continually overtake it. Fair scheduling across outputs remains required. Check cancellation and the admission-fixed
pre-render deadline at each retry. The event loop must wake for that deadline
when otherwise idle. Reuse the existing command WakeSlot for both ordinary-command
and combined-input notification, including builds without experimental-shell-capture.
Clear notification before inspecting both queues, then wait on that notifier and
the earliest parked deadline. No recv() without a deadline while a combined job is
parked, no periodic busy poll, and no new helper thread. Both capture feature
configurations need lost-wake and timeout tests. The ordinary command path gaining
a SignalSender in the non-capture build is added work: count its notifications and
CPU cost; do not call it unchanged. Preserve Release/shutdown service on all paths. Expiry before render returns an owned failure envelope.
Renderer stall clocks retain their existing meaning: start at actual render,
not at validation dispatch. Ordinary requests submitted while a combined render
runs remain accepted under current ordinary admission: their existing submitted_at
age includes the wait behind it. Count this queued delay and resulting stall
classifications; do not reset their clocks or claim responsiveness is unchanged. Expiry during a driver operation does not prove last
use; existing stall/quarantine handling controls retention. No successor combined
job can pass this one. Fairness against ordinary render work requires explicit
tests: no indefinite postponement of an admitted job, no validation-induced hold
on the normal renderer, and no control-command blockage by a full result route.

These pending/deadline/arbitration rules change run_worker; they are not current
capabilities inferred from the presence of a queue. A scheduler unable to satisfy
them is a stopping condition. Do not silently add owner retries and still report
three services.

## Owner completion and death

The owner consumes exactly one terminal combined result. It validates identity
before touching the pending row, then runs description/scene/current-authority
checks, and settles through existing candidate/resource mechanisms. Prepared is
emitted only after owner installation, not on the validator or renderer thread.
A duplicate/foreign result cannot spend another row's credit. No raw pointer to
owner state or mutable authority crosses threads.

For a returned speculative render lease that cannot be installed, use the dedicated
combined retained-Release cell specified in FALLBACK.md, separate from the ordinary
discarded_release Option. A new owner-callable retain entry is required. Full
Release queues retain that exact obligation; public lease Drop alone is insufficient. Validated backing is reconciled under its original retired
epoch. Clear combined credit only on exact owner consumption with required
ownership accounted for, not on transfer, cancellation or channel disconnection.

Sender/receiver exit must ring the existing owner wake. Record OutputClaim and
retirement_owner_identity at dispatch: a shared core can outlive an output.
Renderer dequeue rejects a detached or replaced claim and returns the intact
envelope through its carried owner route, even when that output is no longer in
the worker map. Thread liveness is never output ownership.

The existing seat ReleasePending path calls suspend_revoked_native_scanout,
close_native_owner(DeviceRevoked), then NativeRetirement::poll_seat_release.
NativeRetirement::poll records CompletedNativeOwner only after poll_shutdown,
disposition and actual owner destruction. Mode, EOF, quarantine, timeout and
RetirementFailure do not establish it. Retain combined rows in the accounting
held by RetirementFailure and forbid replacement over unresolved rows.

For proven-forwarded rows in inbox/waiting/rendering/result phases, a matching
CompletedNativeOwner may supply the classified-failure proof to settle_combined
only when the new channels/payload owners participate in that exact destruction
and disposition. A late result cannot settle twice. This extension needs tests;
the current receipt does not already cover new channels. Ambiguous enqueue races
retain the row until ownership is established. Validating rows are untouched:
they return through validation failure or detached-claim/disconnected-send failure.
Seat acknowledgement may precede a still-running validator, which holds no device
resource. NotifyOnExit with outstanding validation stays the current I/O failure,
not a completion proof. Never infer source-read completion from native retirement.

## Source settlement and held-peer cadence

For this first contract, retain the existing settlement coupling: finish private
capture drops OwnedRingRead, but source settlement/publication is deferred until
the combined result is consumed at service 2 (or its exact failure is reconciled).
No new validation-success owner wake and no independent source-settlement path.
The ticket finished flag is the source-read fact; publish_releases still journals
the exact ResourceStatus before confirm_release. EAGAIN retains the work for a
later owner visit, but its existence does not prove peer ACK capacity pressure:
admission already reserves this frame's four responses before Rwrite. The current
owned-copy path has no identified unreserved record consuming that capacity; the
EAGAIN here concerns ordering. Retired epochs never publish into a replacement
journal.

The held peer 273d98c2e frame() returns only after accepted, presented, predecessor
released, source_released and frame credit. It cannot offer n+1 while n's source
release is unknown, so a delayed-release n+2 Budget refusal is NOT a held-lane
prediction. With settlement at service 2 before Presented at service 3, release
delay need not add next-offer delay. Keep two source slots and one frame credit,
and preserve the reserved response capacity and ordering in the eventual composed
model. SourceRelease.tla's arbitrary room=false until the five-second WAIT omits
that reservation and is an over-approximation outside this contract, not a reachable
held-lane timeout claim. The earlier wording implying ACK could consume this room
is withdrawn. Validation/park/render deadlines still require implementation tests.
Count capture/validation/park/render/consumption/release/Presented intervals and
actual publication-order delays in any later capture; no delay or refusal may be
hidden by counting only successful fast-path frames. No capture is selected here.

## Result contents and acceptance

Result includes job and worker incarnation, declared and validated descriptions,
retained private backing, source-finished fact, rendered lease or failure, and phase timestamps/counters only
in diagnostic builds. Validated width/height/stride/extent must equal planned
metadata. Malformed mismatch retains charges until safe settlement.

Owner rechecks allocation rows/scale generation, output size/viewport/layout,
foreign retirement claims, interaction revocation, native availability and the
complete retained scene inputs. Install content, retirement claim, display list
and input bindings atomically; rollback as existing code on definitive refusal.
This does not permit rollback after uncertain or successful kernel submission.
The existing tracked-submit protocol remains a separate required proof.

Niltempus approved conventional fallback for scene-only churn, with authority and
validated description still valid. FALLBACK.md fixes the one-shot transition and
its source obligations. No new host-obsolete reason, peer retry protocol, or
cross-epoch fallback is selected. Exact host-known identical content may avoid
new work before dispatch; peer-declared equality is never sufficient. The full
prototype gate still requires the remaining composition and implementation proofs.

## Required model/prototype checks

Move success and failure preserve a unique payload owner; one pending row through
all phases; cancellation at every transition; two successive jobs with an old
displayed frame; receiver close before/after move; full result route; duplicate
and late results; stalled worker and independently progressing withdrawal; busy
renderer/slot deferral/Release/deadline; no premature resource or credit release.
Mutation controls remove move ownership, release-on-consume, cancellation gate,
and capacity reservation. Prove no intermediate validator success wake reaches
the owner; error/exit/control wakes remain legitimate and counted.

## Revision 3 review disposition

F1/F4: separate one-deep input/result channels, result drain independent of ordinary
in_flight. F2: opaque moved plan, no parked-plan join. F3: explicit proof-consuming
combined quota settlement. F5: common notification/deadline wait in both capture
configs, same-output priority and ordinary stall age counted. F6: retain coupled
source settlement, model release-gated held-peer offers and journal/ACK delay.
C6a-c: output/native identity, exact retirement disposition and independent source
ticket proof. These are design requirements awaiting implementation tests, not
existing APIs. No change to the frozen transfer model or its evidence.

The approved scene-change policy is a single conventional fallback using returned
validated backing, only when description and authority remain valid. This avoids
requesting another peer copy/validation and avoids a new peer-visible reason for
host scene activity, but does not establish lower CPU or remove timing channels.
Count the discarded speculative render and all fallback work. The approval selects
the internal contract clause, not a native run or production adoption. Identical-content fast rejection of unnecessary work must
use host-known exact immutable identity before dispatch, never trust declared
geometry/pixels as proof of equality; otherwise preserve the owner result check.

## Revision 4 corrections

Review 58230480 accepts revision 3 facts as sufficient for the scene decision,
subject to the forwarding witness and reserved-journal qualification above. The
source-release witness remains frozen design evidence, not a native timeout risk.
Security choices do not depend on pretending this abstract schedule is reachable.
