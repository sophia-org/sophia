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

1. Transfer the unused speculative renderer lease to the retained Release path.
   Reuse release_discarded_result / flush_discarded_release semantics in
   exporter/worker.rs: a full command queue retains the obligation. Do not simply
   drop the public scanout lease and infer delivery from Drop (its failed send is
   only counted). No second independent owner may release the same token. A lost
   renderer routes through qualified retirement; no callback frees resources early.
2. Preserve the validated private backing and resource-admission row. Obtain the
   actual admitted lease through existing owner reconciliation; never manufacture
   ContentRenderBundle from the peer's plan metadata. No second pixel capture,
   validation job, frame credit, or accepted Rwrite is introduced for this attempt.
3. Re-enter the conventional owner submit_bundle / project_render_bundle /
   set_shell_component_content path using the live scene and current allocation
   snapshot. Repeat its existing checks at that operation, not just a cached
   Boolean from the combined-result decision. The fallback owns no exemption from
   caller-fresh checks, malformed descriptions, removed outputs or revocation.
4. Preserve the existing setter's error behavior: restore prior shell content and
   retained retirement claim when queue_retained_projection returns an error.
   Preserve the caller's treatment of that error. No Prepared/Presented is invented
   on a failed installation. Existing per-output queue side effects and multi-output
   rollback semantics must be exercised in the real fixture before integration;
   the finite model below only covers the two fields explicitly restored by the
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
It is set when routing begins and retained across pending release/queue/error
handling. A transient failure cannot redispatch the combined job or spend another
response reservation. Failed work follows ordinary bounded recovery/refusal rather
than silently growing a retry loop.

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
  omits restoring them and violates RollbackPair.
- F4: Release queue backpressure: token stays in retained release ownership;
  mutation clears its obligation without acknowledgement and violates LeaseHeld.
- F5: successful but deferred ordinary work: install may succeed and emit Prepared
  without claiming presentation. Failure must not emit Prepared; mutation violates
  PreparedAfterInstall.
- F6: authority changes after routing: conventional entry rechecks; mutation skips
  that check and violates FreshInstall.

Normal and each mutation config enable these falsifiable properties plus TypeOK.
The model makes release acknowledgement nondeterministic, never a timer proof.
The conventional pipeline is an exit state: no claim about eventual presentation,
whole-desktop rollback, GPU last use, forwarding-flag memory ordering, channel
wakeups or performance. No trace exists for the unimplemented combined path.
Required future trace fields: exact job/epoch/output, scene comparison, eligibility
check, fallback entry, speculative Release transfer/ack, ordinary installation
result, Prepared and final presentation identity. The prototype must use real
owner seams for these events, not a second admission implementation.
