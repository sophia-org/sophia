---
id: 516tpvf5
date: 2026-10-27
kind: investigation
status: investigating
tags: [investigation, performance, session, shell-files]
---
# Wire wakes per bar frame

## Question

desktop-cpu-04 measured about 12 owner passes per 119.88 Hz bar frame, of
which about 9.06 began with 9P wire readiness from the bar peer. Which of
those wakes does the protocol require, and which follow from implementation
choices that a session-side or client-side change could remove? This note is
a design for authority review. It contains no code, and it makes no CPU claim
until the pass-cost diagnostic (`continuous-pass-stage-cost-01`) prices a
wire pass.

## Source trace of one owned-ring frame

All references are to sources pinned by desktop-cpu-04: the session at
`d9116ff09`, the SDK at `0706ea2c`, and the `gpu-bar-dma` peer at `748c325`.
This trace is derived from source and has not been observed. The SDK keeps
exactly one `Tread` outstanding on `events` (`files/drive.rs`,
`maybe_read_events`). That is SDK behavior; no normative rule found so far
forbids more than one. Submissions on one attach are sequential, and each waits
for a covering ack of the previous `Submitted` (`files/submission.rs`,
`start_next_outbound`).

The peer runs one strictly sequential exchange per frame (`main.rs`,
`ring_frame` and `present`):

| Step | Peer to session | Session to peer |
|---|---|---|
| Ring upload | image channel offer (image worker wake) | status event, slot release |
| Demand | `transaction` write + `submit` write | `Submitted`, then `FramePermit` |
| Candidate | record + submit (+ covering ack) | `Submitted`, prepared, presented |
| Retire | record + submit (+ covering ack) | `Submitted`, `ResourceReleased` |

The session answers `Submitted` inside the wire turn that carried `submit`:

1. `submit_decoded` appends it to the journal (`shell_transport/files/export/submission.rs`).
2. Once the reply is written, `retry_waiting` answers the parked `Tread` (`sophia-9p/src/unix.rs`, `write`).

The domain reply (permit, outcome, release) is appended later in the pass,
once the owner has processed the inbound record. Its `flush` finds no waiting
read, because the client has not re-armed yet.

So every session-to-peer event that is not in the same `Rread` as an earlier
one costs a separate client re-arm, sometimes coalesced with an ack or the
next record. It is not established that each such write is one session wake:
stream writes can coalesce or fragment, and a late re-arm can be served by a
pass that is already running. With three records and about eight events per
frame, eleven writes and a saving of three wakes are conditional scenarios,
not protocol bounds. The measured 9.06 is consistent with them but does not
confirm them.

## What is required and what is chosen

Required by the current protocol:
- one record and one submit per record;
- the covering ack before the next record;
- one outstanding `Tread`, re-armed after each `Rread`.

Chosen by the implementation:
- **Session:** answering a parked `Tread` with `Submitted` in the wire turn, before the same pass has produced the domain reply.
- **Bench peer:** the strictly sequential order (demand only after the previous retire is released). Lom may differ, so changing only the bench peer would make the bench unrepresentative.

## Candidates, cheapest first

**W1, session only, no protocol change.** Defer event delivery only within a
synchronous owner phase, so that `Submitted` and a domain reply produced in
that same phase leave in one `Rread`. Delivery is never held for the semantic
reply, GPU completion, the next client ack or another wake.
- Predicted saving: one client re-arm, and so one session wake, per record whose reply is produced in the same pass. That is at most 3 of the 9 per frame (demand to permit, retire to release, and candidate to prepared if prepared is decided in that pass).
- Costs:
  - `Submitted` is delivered later, by the rest of the pass;
  - the client's next record waits for the covering ack, so it is delayed by the same amount;
  - the A2 deferred-ack rejection reasoning applies, so admission timing must be priced.
- The earlier publication-flush candidate A (`c1b5ecbb`) does not do this. It only makes publication-only visits call `flush_publications` (no image input) and leaves `Server::flush`, `service_wake`, `write`, `retry_waiting` and `Submitted` delivery unchanged.

**W2, protocol.** Retire the predecessor on presentation, by explicit client
intent in a versioned, negotiated candidate field. The field names the exact
grant and resource generation; nothing is inferred from the visible
predecessor. A presented successor raises a retire *request*, not an
immediate `ResourceReleased`. The existing retire and collect rules still
apply (no consumer leases or capture custody left, retained charges, terminal
publication) before settlement. Retirement and response capacity are reserved
when the candidate is accepted, so presentation cannot strand the obligation.
The design must define the outcome for rejected, superseded and
never-presented candidates, for other outputs still holding the predecessor,
and for retries and revocation. It would remove the Retire record and its
round trip.

**W3, protocol.** Recurring frame demand: one demand yields a permit per frame
until it is cancelled, which removes the per-frame `FrameDemand` round trip.
Today `StandingDemand` is consumed by `grant_demand`, so this is new
semantics. Requirements:
- bounded outstanding permits per output;
- unique IDs and epochs, with a TTL;
- fresh pacing and admission for every permit;
- reserved response credits;
- defined behavior for cancellation races, expiry and withdrawal;
- no idle permit or wake stream to a peer that stops consuming;
- no bypass of permits or credits.

Design W3 only after W1 and W2 have evidence.

S1 (a turn-only pass for wire-only wakes) is complementary. It makes the
remaining wakes cheaper, while W1 to W3 remove them.

## Authority review (root, 2026-10-27)

The review found W1 acceptable in principle, but only under the conditions
below; implementation is not approved. Custody is committed when
`submit_decoded` appends `Submitted`. No requirement for a distinct, immediate
`Rread` was found.

Ordering and custody:
- `Submitted` still precedes the semantic event in journal order, with its original identity, replay, covering-ack and credit semantics.
- Delivery drains unconditionally before any sleep or early continue, or there is a clean close on exit.
- A withheld `Submitted` is an explicit runnable obligation. `publication_pending` is not evidence: `submit_decoded` appends to the journal directly, and that flag is set only by `ShellFileWire::append` and publications.

Do not implement W1 by skipping `retry_waiting` in `sophia-9p`:
- that function also resumes other exports and backpressured reads, and runs revocation checks;
- event `Tread`s that arrive after `submit` in the same receive must also be covered;
- keep fresh `export.check` and the guarded `write_output` / delivery checks, including partial writes and revocation;
- keep Tflush, clunk and disconnect progress;
- role flushes already happen within a pass, so identify the exact batch boundary rather than assuming a single end-of-pass flush.

W2 is worth a separate design, ahead of W3.

## Device-free proof, after both Phase 0 arms finish

This step is proof only. Add an in-process export test that counts event
sequences, `Rread`s, bytes and client writes, and proves exact custody and
order at both the current ordering and W1's. Cases:

1. `Submitted` plus a synchronous reply batch.
2. `Submitted` alone, when the reply is asynchronous or absent: it must still drain.
3. A full journal, or a covering ack needed before more work.
4. A small read count, and output backpressure.
5. Tflush and disconnect.
6. Revocation between acceptance and deferred delivery.

If one record already yields one `Rread` carrying both events, W1 is dropped.
Real SDK scheduling and native owner wakes still need separate validation.
Phase 0's wire-pass body cost ranks this work but does not price it: that
full-pass CPU includes work that may move to the passes that remain. Only a
native ABBA plus the actual counters can decide CPU.

## Baseline invariant coverage (current ordering, proof only)

Shell-specific tests are in `crates/sophia-runtime/tests/shell_event_delivery_files.rs`. For the transport, existing generic 9P tests are reused rather than duplicated.

| Case | Invariant a deferred path must keep | Covered by |
|---|---|---|
| 1 | `Submitted` precedes its same-phase reply, at consecutive sequences | `a_same_phase_reply_needs_a_second_read_after_submitted` (b2cb7b2ed) |
| 2 | `Submitted` leaves from the accepting phase alone: no later owner poll, publication or client ACK (the unconditional-drain oracle) | `submitted_without_any_reply_is_delivered_to_the_parked_read` (8d4621715) |
| 3 | Accepted custody is unique and the next staging stays busy until the covering ACK. A replay is refused or returns the same acceptance. A full journal refuses custody before accepting, and capacity returns for the same ID | `shell_file_descriptor_export::whole_candidate_custody_is_once_only_and_blocks_reopen_until_ack`, `::backpressure_refuses_custody_and_allows_same_id_after_capacity_returns`; `shell_file_transaction_reuse::reusable_writer_recycles_only_after_covering_ack`; `shell_file_transport` (`EALREADY` on replay); `shell_control_budget::full_file_inbox_refuses_custody_then_retries_the_same_id_without_loss`; `shell_file_journal` terminal-reserve tests; sophia-9p `journal::staging_appends_accepts_exact_repeats_and_refuses_gaps_and_changes` |
| 4 | Small reads rebuild events byte for byte with nothing lost or repeated, and queued data resumes when room returns without another publication | `small_event_reads_rebuild_both_events_exactly` (5029d9bad, counts 1, 7 and 29; the rebuilt bytes equal one large read of the same retained range); sophia-9p `journal::reads_span_records_wait_at_the_end_and_refuse_released_offsets`, `pending_read_retries::freeing_reply_capacity_retries_without_another_publication`, `waiting_and_bounds::a_waiting_read_is_not_retried_without_room_for_its_reply` |
| 5 | A flushed parked tag gets no later `Rread` and consumes no event. Clunk and disconnect release retained state | sophia-9p `waiting_and_bounds::a_flushed_read_is_never_answered_and_consumes_nothing`, `::clunk_answers_the_reads_waiting_on_its_fid_first`, `::a_flush_held_behind_full_output_still_wins_over_the_retry`; `pending_read_retries::socket_backpressure_preserves_a_publication_and_flush_cancels_a_waiter`; `shell_file_transaction_reuse::clunk_preserves_accepted_custody_and_revoke_fences_writer`; `shell_typed_outbox_transport::disconnect_releases_typed_records_file_input_and_snapshots` |
| 6 | Retry and queued or partial delivery keep the epoch checks: nothing protected is delivered after revocation, even while output is blocked | sophia-9p `waiting_and_bounds::revocation_ends_a_waiting_read_when_it_is_retried`, `pending_read_retries::revocation_retries_check_authority_before_consuming_data`, `guarded_delivery::revoked_reply_cannot_resume_after_real_socket_backpressure`, `authentication_wire::flush_only_cancels_the_waiting_read_and_revocation_rechecks_it`; `shell_file_authentication::shell_root_refuses_foreign_connections_and_revoked_epochs` |

Notes and gaps:
- **Case 3, no promise of a reply.** Demand credits keep a reserved obligation from being lost; they do not promise an immediate semantic reply, or its delivery to the socket or journal. `ShellTransportConnection::grant_content_demand` (`shell_transport/content_candidates.rs`) first requires `control_capacity_available(.., 2)` and returns `ContentQueueSaturated` otherwise. `ContentCandidates::grant_demand` then runs `grant_permit` (`shell_content/candidates.rs:297`), which has its own stale and budget checks and restores the demand and credit when it refuses. In the live bar a refusal propagates through `service_content` as a service failure and stops the component. `Submitted` is earlier still: it is appended at acceptance. So the unconditional drain (case 2) remains essential.
- **Case 6, delivery guard.** Event-read delivery is guarded only through `AuthenticatedFiles::guard_read_delivery` (`sophia-runtime/src/authenticated_files.rs:300`). The unauthenticated metadata wire relies on `export.check` at each retry. A deferred path must keep both.
- **Untested windows.** Cases 3 and 6 have no shell test for a window between acceptance and deferred delivery, because the current ordering has no such window. Any W1 prototype must add one, and case 2 must keep passing.

## Exact batch boundary in the live owner pass

The earlier version of this section claimed that a deferred answer would never span another component's visit or native work. That is false for any boundary at or after the bar's demand grant, so it is withdrawn. Intake for every wire precedes every role effect, and this design does not reorder them.

### Actual lifetime of a held answer

The ordering is today's, with slot `b` the bar and the answer held from the moment its parked `Tread` becomes answerable:

1. **Wire turn, slot `b`.** `poll_authenticated` → `finish_poll` → `processes.turn_connected()` (`shell_component_connections.rs:598`) → `service_owner_observed`. The submit is accepted, `Submitted` is appended, and today the parked `Tread` is answered here from `Server::write` → `retry_waiting`.
2. **The rest of the same loop.** That covers the wire turns of slots after `b`, every slot's image owner (`b`'s own after its wire), and, with `experimental-shell-capture`, capture completions. A failure here makes `finish_poll` stop that slot.
3. **Back in `service_components` (`component_service.rs`), before any role.** Negotiation and process records are written, then `settle_revocations(Some(runtime))` runs, then `start_next` (which may start a process), then `reconcile_catalog_connections`. Next comes the `!available` early return (with `service_execution`), and then `root`.
4. **Role closures of connected slots ordered before `b`** (`component_service.rs:160`). Dock calls `catalog.service_dock`, and launcher calls `service_actions` and `service_open_content`. Both take `native`.
5. **The bar's own closure, up to the grant.** First `service_indicators` and `service_actions_from`, then `service_content` → `LiveContentSession::service_with_catalog` (`metadata_shell/content.rs:193`). Its stages are:
   - presentation observation;
   - resources (retire → release is decided here);
   - outputs;
   - allocations;
   - `service_dismissals`, which can call `runtime.withdraw_shell_popout` with `native` (`content/actions/dismissal.rs`);
   - demands → `grant_content_demand` (`content.rs:273`), which appends the permit.
6. **After the grant (not crossed by the narrow boundary).** Candidates, then `submit_bundle` (`content/submission.rs`). There `runtime.set_shell_component_content` runs before `content_prepared`, and the backend's `queue_retained_projection` reaches `native.queue_retained_batch`.

Every boundary from step 5 on spans steps 2 to 4 and part of step 5, which include other wires, other components' role effects and conditional native work. A boundary "after the bar's role closure returns" also spans step 6, the native queueing for this very candidate.

### Narrower proposal

Discharge immediately after the demand loop in `service_with_catalog`, before the Candidates stage.

- The held answer spans steps 1 to 5 exactly, and never spans step 6 or any wait, client ACK or later pass.
- Saving: only the same-closure replies that come before the grant, which are the demand → permit and retire → release replies. Candidate → prepared is excluded by construction because it follows native queueing. These are two candidate reply-pair opportunities per frame under this traced recipe. That is not a measured figure, and not a general upper bound on how far owner wakes could fall.
- Whether a span over steps 2 to 4 is acceptable is root's decision. If it is not, no boundary satisfies the earlier limit without reordering, and W1 should be dropped in favour of W2.

### Every exit between hold and discharge

Each exit must drain or revoke before returning. That is a structural guarantee: `service_components` runs its current body as an inner function, and the outer function always runs `discharge_held` before returning the inner result, `Ok` or `Err`. It is not an assertion. `discharge_held` drains and flushes each held answer whose connection is still `Connected` and passes `export.check` and the guarded delivery. If that drain fails, it stops the component, which goes through `request_stop` → `connections.close` and so revokes. A connection that is already closed has its held answer revoked with it. The exits are:

| From | Exit | Handling |
|---|---|---|
| `turn_connected` | a slot's turn fails | `finish_poll` stops that slot, which revokes it; other slots keep their held answers. |
| `finish_poll` | `stop(key)?` / `record_service_failure(..)?` fails mid-loop | `poll_authenticated` returns `Err`; `service_components` logs `poll_failed` and continues; the wrapper covers it. |
| `finish_poll` | the `!available \|\| stopping` branch | The wires are already revoked, so no answer is held. |
| `finish_poll` | service construction fails (`return Err`) | This happens before `turn_connected`, so nothing is held yet. |
| `service_components` | `set_presentation_available?`, `settle_revocations?`, `reconcile?`, `root?`, `with_service?` (×3), the bar's `Completion` `return Err`, `service_execution?`/`??`, the final `settle_revocations?` | wrapper |
| `service_components` | the `!available` early `return Ok` | wrapper |
| role closures | dock, launcher or bar `Poll` failure → `components.stop(key)` | Proposed drain-before-stop; rejected (see the decision below). |
| `service_with_catalog` | every `?` before the grant, including permit-identity exhaustion and `ContentQueueSaturated` | It becomes a role `Poll` failure; the row above applies. |
| owner loop | `service_components(..)?` at `owner_loop/physical_input_loop.rs:585` | The wrapper has already run, so nothing reaches the loop still held. |

Root's answer: drain-before-stop is rejected as a blanket rule (see the decision below).

The prototype tests must add:
- the windows that cases 3 and 6 lack (revocation, and a saturated grant, between hold and discharge);
- a held answer on a slot that fails in step 2;
- a `Tread` received after `submit` in the same receive.

Case 2 must keep passing. No other export's `retry_waiting` changes, and `export.check`, guarded delivery and Tflush/clunk progress are kept.

Prediction to test, not a claim: one fewer client re-arm for each demand → permit (and retire → release) reply in the same closure. Native owner wakes may not fall by the same amount (see the corrections above).

## Decision (root, 2026-10-27): W1 shelved

NO-GO for the expanded hold window. The proposed production implementation is shelved.

- The corrected map shows that this is more than a local change of flush placement. Even with the pre-Candidates boundary, `Submitted` would be delayed across other wire and image work, `settle_revocations`, `start_next`, other roles' effects and conditional native work. "Same pass, no wait" does not prove that the delay of the acceptance notification is suitably bounded, or that failure ordering is unchanged.
- Phases are not reordered, and no outer discharge wrapper is added to recover the optimization. This is a decision about scope and tradeoff. It does not claim that every batching design is unsound.
- Failure policy:
  - Authority loss, revocation, disconnect and transport failure keep their existing stop and revoke priority. No connection is resurrected or prolonged to deliver `Submitted`.
  - For an ordinary semantic error while authority is still live, a future, separately approved design could attempt a guarded nonblocking delivery before the stop, but it could not guarantee that the client sees it.
  - "The client sees `Submitted` before the close" is not a proven guarantee today either: retry and flush can leave bytes queued under backpressure.
  - An outer wrapper cannot drain before a stop that the inner function has already made, because the connection is closed by then.
- The baseline proof (`shell_event_delivery_files.rs`) and this note are kept.

Next: W2, design-only.
