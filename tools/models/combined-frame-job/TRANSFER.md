# Direct validated-job transfer: proposed contract revision 2

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
four journal obligations, fixed deadline and renderer inbox/result credits.
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

A host-created result route and immutable job identity travel with the payload.
Only a successful move into the bounded inbox transfers ownership; a failed or
full send returns the entire payload to its sender. No blocking send, retry spin,
new helper thread, or fresh capacity allocation on failure. Queue Full despite
reserved capacity is an internal invariant failure: return a classified failure
and retain ownership for settlement, never drop bookkeeping to force progress.

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
by waking the owner or spinning. Check cancellation and the admission-fixed
pre-render deadline at each retry. The event loop must wake for that deadline
when otherwise idle. Expiry before render returns an owned failure envelope.
Renderer stall clocks retain their existing meaning: start at actual render,
not at validation dispatch. Expiry during a driver operation does not prove last
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

For a returned speculative render lease that cannot be installed, use ordinary
lease drop / Release command. Full Release queues retain the existing deferred
release obligation. Validated backing is reconciled under its original retired
epoch. Clear combined credit only on exact owner consumption with required
ownership accounted for, not on transfer, cancellation or channel disconnection.

Sender/receiver exit must ring the existing owner wake. A disconnected or
quarantined worker marks the row for recovery but proves no GPU last-use or
source-read completion. Keep that row and charges until exact late-result,
source-ticket destruction, or existing proven-detach/worker-exit evidence permits
settlement. No unlimited replacement workers. The production adapter must bind
this to the concrete detach handler before review approval; absence of that
mapping is an OPEN C6 obligation, not permission to infer proof from EOF.

## Result contents and acceptance

Result includes job and worker incarnation, declared and validated descriptions,
retained private backing, source-finished fact, full composition record/display
list, transform, rendered lease or failure, and phase timestamps/counters only
in diagnostic builds. Validated width/height/stride/extent must equal planned
metadata. Malformed mismatch retains charges until safe settlement.

Owner rechecks allocation rows/scale generation, output size/viewport/layout,
foreign retirement claims, interaction revocation, native availability and the
complete retained scene inputs. Install content, retirement claim, display list
and input bindings atomically; rollback as existing code on definitive refusal.
This does not permit rollback after uncertain or successful kernel submission.
The existing tracked-submit protocol remains a separate required proof.

Scene churn and identical-content handling are intentionally NOT decided here.
A new host-obsolete refusal reason or an explicit conventional rerender fallback
requires niltempus's decision. Consequently the full prototype gate stays held.

## Required model/prototype checks

Move success and failure preserve a unique payload owner; one pending row through
all phases; cancellation at every transition; two successive jobs with an old
displayed frame; receiver close before/after move; full result route; duplicate
and late results; stalled worker and independently progressing withdrawal; busy
renderer/slot deferral/Release/deadline; no premature resource or credit release.
Mutation controls remove move ownership, release-on-consume, cancellation gate,
and capacity reservation. Prove no intermediate validator success wake reaches
the owner; error/exit/control wakes remain legitimate and counted.
