---
id: edc0te2v
date: 2026-11-08
kind: investigation
status: investigating
tags: [investigation, session, shell, performance, custody, sft]
---
# Which action each of the four lean owner wakes owes and which transition a consolidation would remove

## Question

The lean lane wakes the owner four times per presented frame. The accepted
post-lean decomposition (q2m7v8ka) put one question first: for each of the
four waits, which action does its wake make ready, which of those actions must
run on the owner thread, which fresh-authority, immutable-publication or
release boundary separates it from the next action, and which actual round
trip or repeated operation a proposed pass consolidation would remove. This
note answers from the current source and from the dcp-01 rows within their
instrumented scope. It selects no mechanism, changes no source, launches
nothing and claims no saving.

## Method and inputs

Source read on master `0b6a1515a`. File references below are to that tree.
Receipts, all sealed and reused read-only:

- dcp-01: package `~/.local/state/t331/dcp-01/changing` (MANIFEST
  `7e300ac7…`), root result `~/.local/state/sophia-decomp-post-lean-result-01`
  (`7a0677ef…`), independent recount
  `~/.local/state/sophia-decomp-post-lean-result-audit-claude-01`
  (`4d159153…`). One instrumented, ptrace-traced arm; 7,999 whole owner rows
  inside native completions 120..2120; 2,000 completions.
- Wait-order derivation from the same frozen rows:
  `~/.local/state/sophia-decomp-post-lean-wait-order-claude-01`
  (`wait_order.py`, `wait_order.json`, SHA256SUMS `efe0ec0c…`). It counts
  consecutive-row cause transitions and per-class row means; it reads nothing
  the result seal does not already cover.
- The component-walk map (3u93ti2w) and the obligations-on-triggers map
  (rug93yha) supply the per-pass obligation ids B1–B13 used here.
- The SFT contract (`docs/native-frame-transactions.md`) and the lean
  milestone (dcamg9hl) supply the exchange the lane runs: r13 with bits 18,
  19 and 20, one stream credit, publication hold, deferred event re-arm.

Two kinds of statement are kept apart throughout. An observation is a number
or an order read from a receipt. An association is a reading of the source
that says which code ran in a pass; dcp-01 recorded no per-visit receipt, so
every "owed action" below is an association, not an observation.

## What the rows observe

A schema 5 row spans one loop iteration. Its `cause` names the wait that
ended that iteration, so the row's pre-wait body followed the previous row's
wake, and the row's post-wait tail consumed its own wake. The phases are
exclusive thread CPU: `components` is the component walk, which runs before
the wait; `native` is both `service_native` sites, one before the wait and
one in the Timeout tail after it; `authority` includes the wait syscall and
control handling; `other` is the rest.

Every whole row carried exactly one wait. The observed wait order is a fixed
cycle:

| transition | rows |
|---|---:|
| native → wire | 2000 |
| wire → fd | 1999 |
| fd → ring+service | 1999 |
| ring+service → native | 1999 |
| wire → wire | 1 |

1,999 windows of four consecutive rows read wire, fd, ring+service, native.
One adjacent wire → wire transition occurs in the counted span; the derivation
does not join rows to frames, so it is not attributed to a frame here. The
order is an observation. That the wire wake of frame N+1 follows the native wake of frame
N because the peer waits for Presented is an association, read from the
single stream credit (below).

Per-class row means, instrumented and traced, in µs per row:

| row ends in | body followed | components | other | native | authority | total | wall |
|---|---|---:|---:|---:|---:|---:|---:|
| wire | native wake | 138.1 | 63.4 | 24.0 | 25.7 | 251.3 | 863.8 |
| fd | wire wake | 83.9 | 51.3 | 21.8 | 21.9 | 178.9 | 502.4 |
| ring+service | fd wake | 141.5 | 55.6 | 97.0 | 25.4 | 319.4 | 1033.7 |
| native | ring wake | 98.1 | 56.7 | 83.9 | 29.0 | 267.7 | 14330.3 |

Wall is the row's whole monotonic span: body, wait, tail and scheduling. The
native-ended row's 14.3 ms is that whole span, not a measured vertical-blank
sleep and not a deferral any proposal would introduce. These means carry the instrumentation and
the tracer; they are not costs of an uninstrumented pass and no row says
which visit inside it did work.

## The cycle read from source

One pass is: the pre-wait body (`owner_loop/physical_input_loop.rs`: present
clocks, broker poll, policy, catalog scan, `service_components`,
`discard_readiness`, input drain, topology, lifecycle with the pre-wait
`service_native` at `lifecycle.rs:107` → `lifecycle/native_service.rs:29`,
WM phase, present clocks), then the authority phase (`authority.rs`), which
builds the native wait view, picks the Receive wait or the bounded Service
wait, waits, and on `Timeout` runs the tail: a bounded authority drain, the
paced repaint if due, and `service_native` (`authority.rs:781–940`).

The two waits differ in selection, cap and part of their contract.
`take_authority_work`
(`live_session/shutdown.rs:22`) returns Service when
`native_frame_service_should_preempt_authority` (`native_progress.rs:27`)
holds: owed native progress, not preempted on the previous cycle. The Receive
wait (`authority_receive.rs:42–85`) subscribes the metadata shell's
descriptors and its authentication cap, the component descriptors including
the image-worker eventfd, the relays, the native descriptors and the owner
ring, with the maintenance, shell-output and held-work caps, and hands the
authority receiver to `receive_observed`. The Service wait
(`authority_service_wait.rs:48–72`) subscribes the component descriptors
(since B3), the relays, the native descriptors and the ring through
`wait_for_service_observed`, capped by the short service interval, the frame
and native deadlines and the pacer; it does not mirror the metadata-shell
subscription or the receive interface. B3 equalized the component
subscription, not every source or dispatch rule. In this lane the four classes
come from subscriptions both waits hold; the statement is scoped to those. The
ring is in every wait (`owner_wake.rs:276`).

Cause bits (`owner_wait_causes.rs`) name classes, not descriptors. `wire` is a
component 9P stream or listener; `fd` groups the image listener and socket,
authentication, negotiation and the image-worker eventfd
(`shell_component_connections/fd_readiness.rs:66–88`) and is also set for any
ready descriptor no callback classified (`owner_wake.rs:307–314`); `native` is
a native completion descriptor; `ring` is the owner eventfd, which several
senders share; `service` marks a Service wait. The receipt window's role
totals support one association per class in this lane (image-worker 1800 of
1800 fd-class completions, wire 1801 of 1800), but no row records which
descriptor or which payload ended it. The four classes are four source
associations, each to a different descriptor, not four observed identities.

### Table 1. The four wakes

Columns: the wait class as observed; the descriptor the source associates
with that class in this lane; the action the next body or tail performs that the wake made due
(association, with the source path); why that action runs on the owner
thread; the freshness, immutable-publication or release boundary at that
step; and what dcp-01 observes about it.

| wake (observed class) | source-associated ready source | owed action after the wake (association) | owner-only reason | boundary at this step | observed in dcp-01 |
|---|---|---|---|---|---|
| **wire**, 2001 rows | the bar's 9P stream, associated with the peer's FrameCommit Twrite; the payload of any given turn is not recorded | In the next body's component visit, `turn_connected` (`shell_component_connections.rs:685`) hands the wire its observation; `shell_transport/files.rs service` polls custody, runs `turn_observed`, polls custody again, expires; the turn decodes the commit (`files/export/frame.rs decode_sft_input`); then `owner.service` → `propose_frame_commit` (`owned_copy/frame.rs:6`) checks the ring identity, reserves the offer, admits the commit against bounded capacity, submits the copy job to the validation worker, flushes any ready frame events. | The 9P server and its export are served inline on the owner and cannot ring (`authority_receive.rs:42–46`); the candidate store, credits and registry are owner-owned; custody is caller-fresh on the acting thread (`supervisor/launch_custody.rs:55–73`). | Fresh authority before and after the turn (`files.rs:214–227`, `service_turn` at 232); per-request `check` on the authenticated export (`authenticated_files.rs:34, 277`); credit, copy and response capacity reserved before input is consumed (SFT contract); the exact Rwrite is custody under lean (bit 20); a stale ring identity refuses (`Stale`). | wire waits 2001; `component_ready_wire` 1801 over the 1800 receipt completions; `component_turns_observed` 4 per completion, blind 0; owner reads 23 per completion of which 17 failed, errno not captured. Which turn decoded the commit is not recorded. |
| **fd**, 1999 rows | the image-worker eventfd (`image_owner.rs:42 readiness`), subscribed only while a job is pending (`shell_component_connections/images.rs:172`); the receipt window's `component_ready_image_worker` supports the association, the row does not name the descriptor | In the next body's component visit, `owner.service` (`image_owner.rs:113`) takes the completion, re-validates it against the current grant (lost grants withdraw), reconciles, records the settlement; the bar's `service_content` → `service_with_catalog` (`content.rs`) takes the admitted candidate, `submit_bundle` (`content/submission.rs:6`) projects it and installs it as runtime shell content (`set_shell_component_content`, `compositor_graphics/shell_content.rs:17`), records Prepared without a wire notice (bit 19, `content_candidates.rs:276–310`); then the pre-wait `service_native` reduces `SubmitPendingFrame` → `run_native_pending_output` (`production_visual_runtime/native.rs:601`) → `run_tick` → `WorkerCommand::Render` (`rendered_scanout/exporter/worker.rs:567`), moving the content to `rendering_content` (`persistent_native_scanout/state.rs:668`). | The worker completion is paired to this owner alone (`image_owner.rs:139`); the runtime's shell content map and the native scanout's heads are owner-owned; the render itself is on the exporter worker thread. | Validation accepted only for the exact grant and epoch; the copy is a private immutable capture of the slot, its SlotReleased journaled when the worker finished reading but held under lean until the terminal (`files/export/frame.rs:80`, `frame_input.rs:69`); submission validates grant, output and candidate images (`compositor_graphics/shell_content.rs:60–80`); cancellation stops at renderer handoff. | fd waits 1999; `component_ready_image_worker` 1800 over the 1800 receipt completions; validation worker pread64 3, clock_gettime 2, write 1, futex 1 per completion. The admission and the render submission are not separately timed; the row's `native` mean of 97.0 µs covers the pre-wait submission and the tail below. |
| **ring+service**, 1999 rows | the shared owner eventfd, associated with the exporter worker's `result_wake` on render completion (`rendered_scanout/exporter/worker.rs:357, 460`) because the Service reasons name an in-flight render; the sender of any given ring is not recorded. The wait was a Service wait because the request showed owed native progress | In the Timeout tail of the same row, `service_native` pumps completions; the exporter poll takes the result and validates its output, request id and correlation (`rendered_scanout/exporter/worker.rs:604–650`); the head's scanout custody submits the primary-plane commit and records `submitted_at`, which puts the card descriptor and the hard-stall deadline into the next wait (`persistent_native_scanout/completion_wait.rs:89–135`). | The DRM atomic commit and the head state are owned by the native scanout on the owner; the worker returns a buffer, it does not present. | A misrouted or mismatched result is refused and quarantined; native owners already submitted must drain (SFT contract); frame slots are leased from the worker's pool. | Service waits 1800 in the receipt window, every one with reasons head_rendering_content, exporter_pending_frame and exporter_worker_in_flight, `expired_service` unchanged across the window; `owner_ring_ready` 1 per completion; `native_service_waits` 1 per completion, which is the Service-path counter (`owner_wake.rs:217 service_waits`), not a native cause count; render-command completed scopes 288.3 ms over 1,938 completions. Which tail submitted the flip is inferred. |
| **native**, 2000 rows | the DRM card descriptor, associated with the page-flip event (`persistent_native_scanout/completion_wait.rs:89 observe_group`) | In the Timeout tail of the same row, `service_native` → `pump_native_completions` (`completion_pump.rs:125`) → `retire_native_scanout_output_with_mode` (`production_visual_runtime/native.rs:714`) records the presentation epoch and releases the displayed predecessor. In the **next** body's component visit, `bar.observe_presentation` (`content.rs:334`) finds the epoch, `content_presented_at` (`content_candidates.rs:372`) validates grant and capacity, settles the terminal, returns the credit, finishes the frame images, and `flush_content_frame_events` journals the Outcome; journaling a `FrameOutcome` ends the lean hold (`files/export.rs:470`) so the held source records become readable in the same `events` read; the turn's `write_ready` sends them. | The completion is read from the owner-held card fd; the candidate store, credit and journal are owner-owned; the Outcome is a 9P reply the owner alone writes. | Exact native completion is the terminal: predecessor retirement commits only with the exact Presented transfer, reclamation waits for the last lease (SFT contract); capacity is checked before the candidate state changes (`content_candidates.rs:418–428`). | native waits 2000; owner writes 10.02 per completion; peer recvfrom 17 (15 failed), write 7, ppoll 2, sendto 1 per completion. Which turn wrote the Outcome is not recorded. |

The body that follows the ring wake, the one whose row ends in the native
wait, has no owed frame action in this reading. Its component visit still
runs every per-pass obligation: custody polls on each entry (B6), reap
`try_wait` at three sites (B7), settlement and reconcile twice (B8), topology
retry, WM cancellation and chords (B9), one observed-or-blind turn per wire
(B10), `discard_readiness` (B11), and the pre-wait `service_native` (B12).
Its components mean, 98.1 µs per row, is conditional work in rug93yha's sense:
work the pass does by construction. It is not an idle figure and not a
removable cost; nothing in dcp-01 says which of those calls had work.

### Table 2. Round trips and the transition each consolidation would remove

Counted as directed transitions, the frame has two one-way peer legs (peer to
owner: commit; owner to peer: Presented) and three request/completion
exchanges (owner to validation worker and back, owner to exporter worker and
back, owner to kernel and back). The wire wait is associated with the first
leg; the fd, ring+service and native waits with the completion half of one
exchange each. A consolidation must say whether it removes a loop body, a
wait, or a leg of an exchange, and what it must keep; these are different
effects. None is selected here.

| candidate | transition removed | what it touches | what must stay | counters that would move |
|---|---|---|---|---|
| T1: after the Timeout tail that submitted the flip, skip the pre-wait body and go to a fresh Receive wait on the card descriptor | one loop body (the native-ended row's body: component visit, lifecycle, WM phase, present clocks, input drain), not a wait. The ring wait and the native wait both still occur; no ready event is removed. | every per-pass obligation of that body runs later, after the native wake instead of before it: B6 custody polls (pending M1/M2), B7 reap, B8 settlement, B9, B11, B12. How much later is not measured; the 14.3 ms is whole-row wall, not the deferral. This is S1's shape widened from role closures to the whole body; S1 regressed and its finding stands. | a freshly computed wait plan and held work (`owner_held_work.rs:1–2` reads held work after every phase that can create or retire it), the Receive wait's control, metadata-shell and readiness handling, and the owed actions on return. Existing caps (B1–B5, B13, held work at 1 ms) do not prove that skipping those phases preserves the plan; that is a proof obligation, per rug93yha Table A, obligation by obligation. | conditional on the design: if the skipped body keeps its own iteration, pass and wait counts are unchanged and only component visits fall; if two waits fold into one iteration, schema 5 records a `waits=2` row with a union cause and the wait histogram must still count both. "passes 4 → 3" is a hypothesis, not a reading of the source table. |
| T2: skip the pre-wait `service_native` when the request shows nothing owed | one `service_native` call on the bodies after the native and wire wakes | the collection guard `75ff298ab` is this candidate and closed inconclusive; the request is rebuilt several times per pass already | the Timeout-tail call, parked-candidate service when nothing is pending, feedback before an authority-only batch | the `native` phase on wire-ended and fd-ended rows (24.0 and 21.8 µs means) is the whole native phase of those rows, both call sites, instrumented; it is not a price for the pre-wait call |
| T3: validate the committed slot on the owner instead of the worker | the owner-to-validation-worker round trip, hence the fd wait | moves bounded validation of untrusted bytes onto the owner thread; the worker exists to bound that work (`shell_content/isb_worker.rs`) | the private capture, exact grant check at completion, the copy budget | fd waits 1 → 0 per completion; not a pass consolidation but a thread-model change |
| T4: let the exporter worker submit the DRM commit for its own result | the intermediate handoff worker-result → owner poll → DRM submit (`rendered_scanout/exporter/worker.rs:604–650` consumes and validates the result today). The outbound Render request (`worker.rs:567–592`, with its in-flight request and correlation) remains. | moves DRM commit custody off the owner; the native scanout's head state is owner-owned; owner-side completion and error handling, frame-slot lifetime, `submitted_at` and drain, and retirement all need a defined return or transfer mechanism, since Table 1 places them on the owner | atomic presentation per output at the native completion boundary, misroute refusal, drain of submitted owners, an ownership and failure protocol for the transferred state | removal of the separate Service wake is conditional on that protocol: the owner notification could be delayed or coalesced into the native completion, or remain as a wake. A custody change, not a pass consolidation. |
| T5: a second stream credit | the dependency of commit N+1 on Presented N (`frame_events.rs:27`, `credits: 1`) | the SFT first implementation profile of one outstanding frame; store, credit and predecessor semantics; peer SDKs | one credit per outstanding frame, exact Presented terminal, predecessor retirement rule | unknown pending a design and a measurement: a second credit changes interleaving and combined readiness, so pass count, wait classes and per-row owed actions may all move |

Of these five shapes, T1 is the only one that skips a loop body without
moving custody or validation off the owner, and it removes no wait. What it
removes is the per-pass servicing of obligations that have no trigger of
their own; so its precondition is exactly rug93yha's question, obligation by
obligation, with M1/M2 first. T2 is a call, not a pass. T3 removes an
exchange by moving validation onto the owner. T4 removes an intermediate
handoff by moving DRM submission to the worker, and only conditionally a
wake. Both are authority questions before they are cost questions. T5 is a
protocol change with unknown effect on passes.

## What the counts do not prove

The transition table is an observed order; it does not prove that the body
after each wake did the action Table 1 assigns, because no receipt names the
visit that ran. The per-class means do not price any single call. The 98.1 µs
mean on the native-ended rows does not show emptiness: custody polls,
settlement and reap ran with their own purposes. The owner's 44 ppoll, 23
read, 10 write, 12 wait4, 7 recvmsg and 3 accept4 per completion are role
aggregates without errno or fd attribution. None of this measures a saving
for T1–T5, and none of it sets a floor.

## Limits

- One instrumented, traced arm, one topology, one host; every CPU figure
  carries the clock reads and the tracer.
- Associations are read from source at `0b6a1515a`; a later change to the
  reducer, the Service wait predicate or the lean hold moves them.
- The one adjacent wire → wire transition in the counted span is reported, not
  attributed to a frame or explained.
- B6 remains an authority question (M1/M2 pending); nothing here designs it.
- No native run, build, source change or merge follows from this note.

## Connections

- [What the post-lean decomposition says about the next native frame change](q2m7v8ka-what-the-post-lean-decomposition-says-about-the-next-native-frame-change.md)
  posed the question and holds the accepted numbers and limits.
- [Which component-walk visits do useful work per ready source](3u93ti2w-which-component-walk-visits-do-useful-work-per-ready-source.md)
  and [which per-pass obligations can move to their own trigger](rug93yha-which-per-pass-obligations-can-move-to-their-own-trigger-without-weakening-custody.md)
  supply the obligation ids and the trigger analysis T1 depends on.
- [Readiness must name an obligation](../concepts/readiness-readiness-must-name-an-obligation.md)
  is the standard Table 1 applies per wake.
- [Portable Plan 9 interfaces with native frame mechanisms](../concepts/fp9o6pdb-portable-plan-9-interfaces-with-native-frame-mechanisms.md)
  is the context for T5; this note does not select a ring.
- Selection and order remain in [the plan](../plans/4rifczhw-reduce-continuous-frame-cpu-without-weakening-custody.md)
  and `todo.md` t344, recorded by Codex.
