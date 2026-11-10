# Approved scene-only conventional fallback

Niltempus approved this clause after the CPU/frame comparison discussion. This is
an internal design contract; no implementation, native run, wire change or CPU
saving is implied. Category B: owner result decisions with asynchronous authority
changes and renderer resource ownership. The target remains three dependent owner
services on a stable scene; additional fallback services/work are counted.

## Decision and source boundaries

At owner consumption of an exact combined result, validate job/output/native owner
and OutputClaim identity, candidate grant/epoch, returned validated description,
allocation rows/scale generation, output/viewport/layout, interaction revocation,
foreign retirement claims and native availability. A description or authority
failure follows existing refusal/withdrawal rules; scene mismatch never excuses it.
Geometry-changing topology/allocation changes are not scene-only churn. The scene
comparison covers the full frozen composition, including other clients, not merely
the submitted panel. The exact eligibility/comparison implementation is still a
prototype gate, not a new scalar claimed to exist in the source.

If these checks pass and the scene is unchanged, continue the proposed
install-already-rendered path. If only scene inputs changed, consume the fallback
choice once and take the following path:

1. Transfer the unused speculative renderer lease into a dedicated combined
   retained-Release cell in NativeGbmRendererWorker, reserved with admission (one
   per output). The ordinary discarded_release Option is already used by late or
   misrouted ordinary WorkerResult and MUST NOT be overwritten. A new owner-callable
   combined-result retain operation is required; current release_discarded_result
   cannot accept this result type. It returns exact retention proof before the
   combined validation credit clears, within the same consuming owner turn. The
   model separates Settle/Retain/ClearCredit and permits a gap; it does not prove
   that the implementation performs them in one turn. No backend lease type is added to the lower
   sophia-runtime crate; the owner joins the backend disposition with the job token.
   Reuse flush_discarded_release's Full-retains semantics, with the separate cell.
   Ownership moves to the command queue only after successful send. The physical
   slot frees only when the worker processes Release or exact retirement proves
   disposition; no new owner-facing Release ACK or wake is invented. Admission
   cannot reuse an occupied combined release cell. Ordinary discard and combined
   discard may coexist. Do not simply drop the public scanout lease and infer
   delivery from Drop (its failed send is only counted). A lost renderer routes
   through qualified retirement; no callback frees resources early.
2. Preserve the validated private backing and resource-admission row. Obtain the
   actual admitted lease through existing owner reconciliation; never manufacture
   ContentRenderBundle from the peer's plan metadata. No second pixel capture,
   validation job, frame credit, or accepted Rwrite is introduced for this attempt.
3. Decline fast installation for this candidate using the one-way owner latch.
   The fast path MUST NOT change Prepared, PendingPresentation, retirement claims
   or shell_content before this decision. After ordinary resource reconciliation,
   candidates::begin_submission_inner builds the bundle and the EXISTING content
   service calls submit_bundle / project_render_bundle / set_shell_component_content.
   Add no fallback-specific submit_bundle caller and never put a submitted candidate
   back into pending. This is conventional settlement, not a re-entry. Its live
   scene/allocation snapshots and existing checks remain authoritative: a cached
   result-decision Boolean does not bypass removed-output, malformed or grant checks.
4. Preserve the existing setter's error behavior: restore prior shell content and
   retained retirement claim when queue_retained_projection returns an error.
   Preserve the caller's treatment of that error. No Prepared/Presented is invented
   on a failed installation. Existing per-output queue side effects and multi-output
   rollback semantics must be exercised in the real fixture before integration;
   settle_shell_retirement_claims runs before the error point and is not rolled
   back. Channel Full is NOT this error: WorkerQueueFull/WorkerPending are handled
   as deferral by handle_preview_refusal and queue_retained_projection. The finite
   model below only covers the two fields explicitly restored by the
   setter. No rollback follows an uncertain or accepted DRM ioctl.
5. On success, retain existing Prepared and PendingPresentation behavior.
   queue_retained_projection can return Ok(false) while it retains pending work;
   set_shell_component_content can also return Ok(false) for an identical frame.
   Neither means Presented, rejection or another fallback attempt. Reuse current
   handling: the ordinary pipeline owns subsequent scheduling, coalescing, current
   checks, errors and exact presentation. Further scene changes stay there. There
   is exactly one ENTRY to that pipeline per candidate, not a claim of exactly one
   actual renderer invocation on a changing desktop.

The fallback admission latch is owner-local and monotonic for this exact candidate.
It is set before any candidate installation and retained across pending Release and
ordinary deferral. OneFallbackEntry means one candidate transition into ordinary
submission; later ordinary render retries are permitted and counted. The speculative
Release and ordinary Render share the finite command channel and three-slot pool.
Release retained on Full can delay Render until polling flushes it and a slot frees;
this is counted latency, not a candidate refusal. An enqueued Render may precede
Release and return Deferred safely; actually reusing the still-held slot is forbidden.
Deferred returns to the owner, which parks pending_frame; resubmission requires a
later owner export cycle (worker_export::export_from_worker). It is NOT the
worker-internal retry used by a parked combined job. Count the result collection,
later export cycle and latest-wins supersession if a newer frame replaces it.
That supersession replaces pending render work, not the installed shell_content
candidate. Its content persists into the superseding composition; Presented resolves
through presented_content_list_matches on whichever frame actually presents
(shell_content_presentation_epoch), under the existing identity checks.
No transient Full redispatches the combined job or spends new response reservations.
A definitive failure keeps its ordinary classification; no fallback retry loop.

## Accounting, security and responsiveness

Keep the four response reservations established at admission. Prepared, terminal,
resource status and resource release keep their existing meanings; no host-scene
refusal reason or extra event is added. Source reuse remains finished-ticket plus
journal-first confirmation; a renderer result or fallback latch is not that proof.

Record the fallback once, discarded speculative renders, actual conventional render
calls, owner services, Release retries, retained bytes/slot age, and ordinary stall
age. Include all those costs in combined Session+peer CPU per completed frame; keep
refused/timed-out attempts and latency/cadence visible. Reusing validated backing
avoids asking the peer to recapture/revalidate for this scene change; it does not
prove that this treatment beats the current pipeline. High churn can erase savings.

No new frame-granularity scene-change reason is exposed. Existing timing channels
remain. An authority change is never masked as scene churn. All speculative and
ordinary leases stay charged until their actual disposition; entering fallback
cannot retire the displayed predecessor. Validation still runs on its own thread.

## Scenarios and finite decision projection

Fallback.tla begins with one completed validation/speculative render and a retained
previous installed content/claim. It is not composed with Transfer or FrameJob and
not a Rust execution model. The owner decision is abstracted as an atomic step;
the real fresh-check/submission intervals and the GPU kernel are outside it.

- F1: scene changes plus grant/description failure: refuse; mutation bypasses the
  fallback eligibility guard and violates EligibleFallback.
- F2: another scene change after routing: no recursive combined retry; mutation
  re-enters fallback and violates OneFallbackEntry.
- F3: conventional queue error: preserve the two prior owner fields; mutation
  omits restoring them and violates RollbackPair. Install(FALSE,FALSE) abstracts a
  definitive error, never command-channel Full.
- F4: Release queue backpressure: token stays in retained release ownership;
  mutation clears its obligation without disposition and violates LeaseHeld.
- F5: successful but deferred ordinary work: install may succeed and emit Prepared
  without claiming presentation. Failure must not emit Prepared; mutation violates
  PreparedAfterInstall.
- F6: authority changes after routing: conventional entry rechecks; mutation skips
  that check and violates FreshInstall.

Normal and each mutation config enable these falsifiable properties plus TypeOK.
The old model's ReleaseAck is abstract physical disposition, not an existing
worker acknowledgement. The implementation has no Release ACK. The new owner-turn
projection explicitly separates retained token, successful command transfer and
worker processing; quota clearance does not wait for physical slot freedom.
The conventional pipeline is an exit state: no claim about eventual presentation,
whole-desktop rollback, GPU last use, forwarding-flag memory ordering, channel
wakeups or performance. No trace exists for the unimplemented combined path.
Required future trace fields: exact job/epoch/output, scene comparison, eligibility
check, fallback entry, speculative Release retention/transfer/disposition, ordinary installation
result, Prepared and final presentation identity. The prototype must use real
owner seams for these events, not a second admission implementation.

## Next owner-turn/channel projection

OwnerTurn.tla composes result consumption, settlement bookkeeping, dedicated
retained Release, validation-credit clearance, fast-path-decline latch and ordinary
candidate entry. Its bounded command queue is capacity 2 to expose Full (the real
shared queue is 32); physical slots are exactly 3. Other commands initially fill
the queue and drain; ordinary Render can defer, return to the owner, and retry on
a later export cycle, with one optional pending-frame supersession. Release transfer is not
slot freedom. It permits a separately proved prior-slot release as an abstract
external fact, not a new last-use rule. No driver stall, output replacement, full
scene predicate, journal, actual teardown or arbitrary multi-output fairness is
proved. The existing eligibility model supplies only a separate interface
obligation, not an automatically composed authority proof.

Release proof at quota clearance is token held in the dedicated cell OR successful
send transferring it to WorkerCommand (or a separately established final disposition).
Disconnected is a counted failed enqueue, not a successful transfer or physical
release; worker retirement still needs its qualified disposition. OwnerTurn assumes
a live command receiver and omits disconnection/retirement. No ACK is added.

Owner-turn review 2350ce57 accepts this narrow composition. Initial queue noise is
bounded: other-output commands cannot arrive again in OwnerTurn, while on a shared
core they can refill the real queue indefinitely. Recurrent Full is outside this
finite check. CreditProtected does not cover send to a dead receiver; native
retirement belongs to Transfer's separate RetireNative/SettleRetired projection,
not a composed proof here. Test-only seam authorization remains pending; see
SEAM-PROPOSAL.md. The accepted model and evidence are unchanged.
