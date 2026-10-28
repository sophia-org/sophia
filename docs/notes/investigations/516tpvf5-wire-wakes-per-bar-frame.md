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
| 4 | Small reads rebuild events byte for byte with nothing lost or repeated, and queued data resumes when room returns without another publication | `small_event_reads_rebuild_both_events_exactly` (5029d9bad, counts 1, 7 and 29); sophia-9p `journal::reads_span_records_wait_at_the_end_and_refuse_released_offsets`, `pending_read_retries::freeing_reply_capacity_retries_without_another_publication`, `waiting_and_bounds::a_waiting_read_is_not_retried_without_room_for_its_reply` |
| 5 | A flushed parked tag gets no later `Rread` and consumes no event. Clunk and disconnect release retained state | sophia-9p `waiting_and_bounds::a_flushed_read_is_never_answered_and_consumes_nothing`, `::clunk_answers_the_reads_waiting_on_its_fid_first`, `::a_flush_held_behind_full_output_still_wins_over_the_retry`; `pending_read_retries::socket_backpressure_preserves_a_publication_and_flush_cancels_a_waiter`; `shell_file_transaction_reuse::clunk_preserves_accepted_custody_and_revoke_fences_writer`; `shell_typed_outbox_transport::disconnect_releases_typed_records_file_input_and_snapshots` |
| 6 | Retry and queued or partial delivery keep the epoch checks: nothing protected is delivered after revocation, even while output is blocked | sophia-9p `waiting_and_bounds::revocation_ends_a_waiting_read_when_it_is_retried`, `pending_read_retries::revocation_retries_check_authority_before_consuming_data`, `guarded_delivery::revoked_reply_cannot_resume_after_real_socket_backpressure`, `authentication_wire::flush_only_cancels_the_waiting_read_and_revocation_rechecks_it`; `shell_file_authentication::shell_root_refuses_foreign_connections_and_revoked_epochs` |

Notes and gaps:
- **Case 3, room for the reply.** A semantic reply cannot be refused for lack of room once its record is accepted: demand admission reserves the response credits (`shell_content/candidates/demands.rs`, `response_credits`). Case 2 independently shows that `Submitted` never waits for a reply.
- **Case 6, delivery guard.** Event-read delivery is guarded only through `AuthenticatedFiles::guard_read_delivery` (`sophia-runtime/src/authenticated_files.rs:300`). The unauthenticated metadata wire relies on `export.check` at each retry. A deferred path must keep both.
- **Untested windows.** Cases 3 and 6 have no shell test for a window between acceptance and deferred delivery, because the current ordering has no such window. Any W1 prototype must add one, and case 2 must keep passing.

## Exact batch boundary in the live owner pass

Within one owner pass, a component's wire input is served once, and its role services run later in the same `service_components` call:

1. `component_service::service_components` calls `components.poll_authenticated(...)` (`component_service.rs:32`).
2. That visit ends in `processes.turn_connected()` (`metadata_shell/component_session.rs`, "Every connected wire turns once per visit"), which calls `ShellComponentTransport::service_owner_observed`. Here the submit is accepted, `Submitted` is appended, and a parked `Tread` is answered at once from `sophia-9p` `Server::write` → `retry_waiting`.
3. In the same call, the bar's role closure (`component_service.rs:245`) runs `service_indicators`, `service_actions_from`, `service_content` (which grants the demand: `metadata_shell/content.rs:268`) and `observe_presentation`. Its `poll_io` calls only drain and flush (`serve_input=false` once the owner has serviced the wire, `shell_transport.rs:407`). By then the client has not re-armed, so the permit waits for the next wire pass.

Proposed boundary (not implemented):
- Answers to event reads that become ready during the step-2 wire turn are an explicit, per-component deferred obligation.
- It is discharged by that component's first drain-and-flush after its role closure returns, inside the same `service_components` iteration.
- It is discharged unconditionally at the end of `service_components` for every component, including the `continue` paths (dock, launcher), failure and stop paths, and the stopping branch at `component_session.rs:394`.
- It is also discharged before the next owner wait as a safety net. That point should never find work, and an assertion should check it.

The deferral never spans a wait, another component's visit, the native-present or GPU phases, or a client ACK. It changes no other export's `retry_waiting`. It must cover a `Tread` received after `submit` in the same receive as well as one already parked, and must keep `export.check`, the guarded delivery and Tflush/clunk progress.

Prediction to test, not a claim: one fewer client re-arm for each record whose reply is produced by that component's role closure in the same pass. Native owner wakes may not fall by the same amount (see the corrections above).
