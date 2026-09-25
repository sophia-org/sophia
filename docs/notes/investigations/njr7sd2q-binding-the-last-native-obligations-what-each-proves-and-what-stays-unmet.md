---
id: njr7sd2q
date: 2026-09-20
kind: investigation
status: resolved
tags: [investigation, x11, input, tooling]
---
# Binding the last native obligations, what each proves, and what stays unmet

## Question

After t136 closed, `cargo xtask check x11-profile --profile=native-input`
on master `21d3fced` read eight of forty obligations NORESULT: six with no
implementation and two the adapter lane had recorded as unmet with the
reason beside them
([the lane's note](zrf53aky-binding-the-native-obligations-and-what-the-last-six-need.md)).
Which of the six can be bound to a test whose assertions are the
obligation's claim, which need a test written, and which are genuinely
unmet, and why?

The rule this follows is the lane's: every binding is read before it is
bound, and a compound obligation is never bound on a partial witness, because
the runner has no partial verdict and a PASS on it would be the false green
the gate cannot see. Where a binding stops short of the whole claim, the
manifest's `evidence_scope` says so in the obligation's own words.

## Evidence

### `native_lock_order`: bound at `849a9b53`

"Guarded APIs and contending writers preserve selected lock order." The
plan's vocabulary is the selected acquisition rank
([plan, Synchronization and execution](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md));
the code's is per-edge comments and the note that says what a rank is
([w8vt0ueb](w8vt0ueb-production-entry-and-ownership-for-ordered-synthetic-input-execution.md):
a rank orders acquisitions made while another guard is held). Keyword search
reaches neither, so the tests were found by subject and read.

| witness | what its assertions establish |
| --- | --- |
| `pending_control_gets_the_next_output_lock` | a real contender already waiting on the output lock is passed by a pending control when the lock is released; both then run, in that order |
| `pending_control_gets_the_next_runtime_lock` | the same on the runtime lock through `lock_x11_control_runtime` and `wait_for_x11_control_runtime` |
| `pending_control_wins_the_request_runtime_lock_race` | the same through `lock_x11_request_runtime`, with the control registered after the request began waiting |
| `focus_encoding_takes_input_authority_before_event_selections` | with both locks poisoned, the focus writer reports the input-authority poison, so it takes input authority before core event selections: the order the input writer uses, which `375e92c7` made one order for both |
| `one_step_takes_one_item_and_marks_it_before_common` | the order's mark runs with common free and the ledger unmoved, so the mark sits above common and the queue guard is released before common is taken |

The first three are the guarded APIs under real contention; the last two are
contending writers holding to one order. What they do not establish is the
whole rank as an audit: no test walks the list. And the list itself is not
one list. The plan writes the rank as outer X runtime, coordinator, common,
surfaces, pointer state, frozen input, core subscriptions, clients, XFixes
subscriptions, then X input authority; the code documents common, bindings,
clients, surfaces, native base, exact selections, publication
(`private_execution_key.rs`), coordinator then common (`broker.rs`,
`control_transition.rs`), the ledger's guard released before common
(`private_execution.rs`), and input authority before core selections in the
focus and input writers (`focus.rs`, `375e92c7`). Clients against surfaces,
and core selections against input authority, are placed differently in the
two. The tests witness the order the code documents. Whether the plan's list
is corrected to the code or the code is an inversion of a selection is a
decision, recorded here rather than settled by a binding; the scope says so.

### `native_recipient_removal`: bound at `849a9b53`

"Route removal while recipient lives retains targeted debt."

| witness | what its assertions establish |
| --- | --- |
| `vanished_emission_target_retains_the_real_release_debt_and_does_not_reuse_press_bytes` | the recipient window is removed from the selection and hierarchy state (`XCoreEventSelectionState::remove`: windows, parents, geometry, stacking, mapping) while its client stays admitted; the release resolves `DeliverTo` the exact incarnation, the event refuses `HierarchyMissing`, the ledger still reports that incarnation as the next debt, and the press emission's slot is not overwritten |
| `lifecycle_integration_native_hold_debt_is_not_query_cleanup` | a close request on the query lifecycle, driven through retirement, leaves the native hold debt in `next_debt` and the hold in the terminal; the registration and its channels are alive throughout |

Neither makes a writer, disconnect or wire claim, and the scope says so.

By hand each of the seven prints exactly one named `ok` under the runner's
own command shape; the profile on `849a9b53` reads native-input FAIL with six
NORESULT, down from eight, and the two now PASS.

### `native_stalled_reader`: written and bound at `4c6cdca2`

"Real private stalled socket reader is contained while healthy peer
continues." The halves existed apart: the terminal-level control proves a
blocked head stops only its own connection
(`a_blocked_head_stops_its_own_connection_while_another_recipient_progresses`),
the watchdog control proves a blocked write on a real socket can be ended
(`independent_descriptor_ends_a_blocked_write_while_output_lock_is_held`),
and the M3 helper `blocked_recipient_attempt` proves one real reader that
stops is answered `TimedOut`. Nothing had put two real readers on one
running service and let one stop. The trace that preceded the test settled
what the live path does, and corrected a reading the lane and I had both
taken from a comment:

- The ordered writer chain is live. `attach_ready_workers` spawns one
  `x11-ordered-output-{client}` thread per connection, whose body serves
  through `X11OrderedServingOwner::serve_one`, `serve_one_ordered_delivery`,
  `write_one_ordered_frame` and `send_pending_frame` in `blocked_send.rs`.
  The `allow(dead_code)` attributes along that chain and the comments that
  say "not attached yet" and "exercised by controls and not yet by a
  writer" are stale; `allow` never made anything dead.
- A full socket blocks only that connection. Every send is
  `MSG_DONTWAIT | MSG_NOSIGNAL`; on `EAGAIN` the writer polls `POLLOUT` in
  50 ms slices and accrues the elapsed time to the delivery in hand; at six
  seconds the send fails `Blocked`, the delivery is answered `TimedOut`, and
  the writer shuts the socket down. The service turn never touches the
  socket: it `try_send`s onto the bounded per-client queue and retains a
  capsule the queue will not take. Other recipients' writers are their own
  threads.

The test, `a_stalled_private_reader_is_contained_while_a_healthy_peer_continues`
in `tests/support/private_stalled_reader.rs`, included beside the M3
fixtures it reuses: one service over real sockets, two admitted clients.
The stalled one has a button pair read back first, so the recipient is
established, and then never reads again; scroll events are routed to it one
receipt at a time until a receipt does not come within 300 ms. Inside that
silence the healthy peer has a `GetInputFocus` answered, a scroll and a key
press and release read back as exact core frames and settled `Flushed`.
Then the blocked delivery reads `TimedOut` no sooner than six seconds after
the fill began, the stalled socket reads to its end, the healthy peer is
served once more, and the instance stops collected. Three of three by hand
at about 6.6 s each. Measured: the buffer fills after about 140 scrolls, in
under a second.

Two things it found that are not its subject:

- **A key routed to the focused surface while the pointer is over another
  client's surface is answered `RouteRejected`**, in 13 ms, not delivered.
  After a scroll routed to the focused client's surface the same key is
  `Flushed`. The Session decides the route; whether the private source
  should refuse a key whose route names the focus while the pointer is
  elsewhere, or deliver it to the focus as the core protocol does, is a
  question about the routing model. The lane filed and closed it as t140
  (`98a9bf41`): the core rule was already in `normal_key_target`, a key
  goes to focus when the pointer's branch is disjoint from it, and
  `key_pointer_path` had been refusing before that rule was reached; it now
  answers with the root alone and the existing path delivers at focus.
- The stale attributes above. Not removed here: the lane is in the
  continuation store (t138) and found the same pattern lying to them there;
  a sweep is one change, not a side edit of a binding commit.

### `native_no_fallback`: bound beside this note, with its scope

"Unavailable or revoked instance authority refuses without ambient backend
acquisition." The refusal half has direct witnesses:
`a_revoked_admission_stops_a_later_execution` (a revoked grant is refused
`NoCurrentAdmission` before the permit, and stays refused under a
replacement admission) and
`a_boundary_nobody_can_read_is_not_a_client_nobody_admitted` (an unreadable
boundary is `Unreachable`, not nobody admitted). The two the lane's note had
named were adjacent: `a_poisoned_owner_reports_unavailable_rather_than_nothing_to_do`
is about the settlement owner, and
`desired_foreign_authority_cannot_execute_through_bound_broker` is foreign,
not unavailable or revoked.

The "without ambient backend acquisition" half holds by absence and is
asserted only by the M4 group `no_ambient_fallback`: `sophia-x-authority`
depends on no portal, bus, libei or display client and reads no `DISPLAY`,
`WAYLAND_DISPLAY` or bus address; the private host refuses ambient options
by name; the M4 group launches the real host under fabricated ambient names
and requires that refusal. That group carries `#[ignore]` and runs only
through the gate, and `m6-evidence` runs M4 on the same source as this
profile, which is where the composed claim is complete. The scope says
exactly that, so the profile's PASS on this row is read with it.

### `native_executor_order`: unmet as written, reason recorded

Of the seven producer kinds it names, two admit into the one shared order:
routed input (key and pointer, one facade, `PrivateIngress`) and control
(`PrivateControlProducer`). Repeat is admitted and refused
`RepeatUnsupported` at execution. Connection mutation, thaw and cleanup
never enter the order, and the code says so on purpose: thaw is a side
deque that does not go back through admission, cleanup runs on the
deferred-cleanup path with its class reserved and no producer, and
connection mutations are applied through the registry. What is proved for
the two that exist is written in the manifest's reason with the tests by
name: one order with exact identities at consumption, completed-send
precedence, and saturation returning the payload and settling every
accepted item once. A binding would report PASS on five producer kinds
nobody built. Splitting the row into the clauses that hold, or rewording it
to the producers that exist, is the decision.

### `native_ingress_admission`: read clause by clause; two clauses have no test

Eleven clauses, each read against its candidate's assertions rather than
its name. Seven are carried, on the ingress path where it matters:

| clause | carried by |
| --- | --- |
| capacity, apart from denial and exhaustion | `c_capacity` ("a full order is retryable saturation, not denial or exhaustion") |
| denial apart from saturation | `a_private_producer_is_told_denial_apart_from_saturation`, but the denial it makes is a closed control revision, not an authority refusal |
| unavailable or disconnected authority | `producers_are_refused_once_their_consumer_is_gone`, `c_poison`, `a_producer_is_told_which_refusal_it_met` |
| every refusal returns owned work | `work_refused_by_the_order_takes_its_reservation_back`, `a_refusal_destroys_nothing_it_was_handed`, `a_refused_control_comes_back_to_its_producer` |
| rolls back only its own credit | `work_refused_by_the_order_takes_its_reservation_back`, `c_capacity`, `c_poison` |
| preserves accepted work | `review_owner_saturation_cannot_discard_two_already_accepted_controls`, `c_poison` |
| consumer closure rejects later work and settles accepted work | `losing_a_prepared_runner_closes_its_producers_and_carries_its_hold`, `accepted_work_is_answered_when_its_consumer_goes_away` |
| queue poison is unavailable, not empty | `c_poison`, `an_unreachable_queue_is_not_reported_as_a_finished_one` |

What is not carried, and needs a test before the row can be bound:

- **Duplicate delivery identity at admission has no test anywhere.**
  `PrivateSendError::DeliveryAlreadyTracked` is produced at
  `broker.rs:146` and asserted nowhere; the candidate
  `a_reused_delivery_id_does_not_settle_the_debt_that_had_it_before` is
  about settlement and asserts that a pruned id is admitted again.
- **Sequence exhaustion on the ingress path.** All three `Exhausted`
  producers inside `PrivateIngress::submit` are unexercised; the evidence is
  on the control producer and one layer down.
- **The authority-denial arm** (`NoCurrentAdmission`/`Authority` mapped to
  `Denied`) and **the authority-unreachable arm** (`Unreachable` mapped to
  `Unavailable`) of the same fan-out have no test; the words are tested
  through a closed revision and a poisoned queue.
- **The exhaustion latch** in `SharedAdmission` ("latched here and never
  reset") has no test asserting a later submit still refuses.

Two candidates the lane's table named do not support their clause:
`a_count_that_cannot_advance_refuses_rather_than_saturating` is the
dependent counter on an accepted effect, and
`a_poisoned_owner_reports_unavailable_rather_than_nothing_to_do` is the
settlement owner. The reading rule earned its keep twice more.

**Written the same evening, and bound.** Five witnesses in
`tests/support/private_ingress_refusals.rs`, each on `PrivateIngress::submit`
over a frontend whose store the control reads the credit of:

| witness | what its assertions establish |
| --- | --- |
| `a_delivery_identity_already_live_is_refused_at_admission_and_the_live_one_is_untouched` | a second grant submitting a live delivery id is refused `DeliveryAlreadyTracked` with the route intact; the live delivery keeps the very cell its admission minted; no credit moves; the refused grant is accepted with an id of its own; both run |
| `an_ingress_whose_request_numbers_are_spent_refuses_exhausted_and_stays_exhausted` | the ingress counter at its end refuses `Exhausted`, rolls back the delivery reservation and no credit, refuses again rather than accepting, and another producer is unaffected |
| `an_order_that_exhausted_its_positions_refuses_every_later_submission_and_keeps_what_it_accepted` | the latch set as the order sets it refuses `Exhausted`, keeps what was accepted, and a drain does not reset it; the stream's counter is not driven to its end, which the scope says |
| `an_authority_nobody_can_read_refuses_a_submission_as_unavailable_not_denied` | common poisoned: `Unavailable`, route intact, reservation rolled back, no credit |
| `a_revoked_admission_is_refused_as_a_denial_at_submission_and_the_work_comes_back` | the admission revoked through the boundary: `Denied`, route intact, reservation rolled back, no credit |

The two arms whose answer was predicted from the code rather than measured
came out as predicted: a revoked admission is a decision and is refused as
one, and an unreadable authority is refused as nothing established. With
these, every clause has a witness on the path the obligation names, and
the row is bound to nineteen tests.

### `native_internal_wait`: the recorded reason no longer stands

The lane recorded it unmet because "nothing accrues to the meter from the
live path". The trace above shows the meter is fed by the live ordered
writer, so that reason is gone. What the obligation asks, that delay,
frozen work, scheduler and control waits never accrue recipient
nonresponse, is now a property the code has and a test can witness.

**Written and bound the same evening.**
`a_delivery_that_waited_behind_the_output_lock_past_the_allowance_is_still_flushed`
in `tests/support/private_internal_wait.rs`, beside the stalled reader: a
real service and one real client; the connection's output lock, the one a
control write takes, held from before a key delivery is accepted until past
the whole six-second allowance. While it is held no receipt and no bytes
come; when it goes, the delivery is answered `Flushed` and the event
arrives whole, at 7.5 s, which is the allowance plus the release. Had the
wait behind the lock counted, the delivery would have been past the
allowance and answered `TimedOut` with the socket ended. Bound with the
unit control that only measured waiting on the recipient accrues
(`a_send_counts_only_what_it_waited_on_this_recipient`). Queue, delay and
frozen waits sit before the writer takes a delivery and so before its meter
exists, by construction; the scope says they are not separately timed.

### `native_protected_action`: retracted by the lane, owed, and t139

The lane's retraction (`472bc15b`) stands: the protected chord exists as the
emergency chord, the missing half is the rule that a synthetic contribution
can neither satisfy nor taint it, and XTEST injects any keycode today. Row
t139 is opened for it. Read before anything was built, the facts are these:

- **The emergency action is recognised only from physical devices, by
  construction.** The input guard (`live_session/input_guard.rs`) is its own
  process that opens libinput itself and feeds `EmergencyChordState::observe`
  from what it polls; the owner loop keeps a second `EmergencyChordState` on
  the session's own physical input turn. Synthetic input enters through the
  private service and the X authority and has no edge to either observer.
  So a synthetic Ctrl-Alt-Backspace cannot trigger recovery, and a synthetic
  hold of one of its keys cannot keep a physical chord from triggering it,
  today, because the two paths never meet.
- **What synthetic input can do is put the chord into X clients as ordinary
  key events.** `xtest.rs` converts an admitted `FakeInput` detail to evdev
  with no filter (detail 22 is Backspace); the engine refuses to bind the
  chord to a policy client, so no shortcut fires, but the focused client
  sees the keys. Whether that is a protected action being supplied depends
  on what "protected action" is taken to mean.
- **The authority has no recognition.** `registry/execution.rs` anticipates
  one, physical-only and run before the aggregate so a recipient barrier
  cannot swallow it; nothing joins that to the ledger's physical-versus-
  synthetic distinction. That is the half the obligation's "fixture
  physical chord" names, for the headless model.

The lane, told of the guard, sharpened its own claim rather than defend it:
the recovery path is not forgeable from the adapter, and what XTEST does
circumvent is the engine's property that the chord is never available to a
policy client whatever it registers, because a client that could not
register the chord can be handed it as key events. That is the claim that
stands. The lane's advice on the decision, recorded as its own: a
construction witness proves a path does not exist for a reason that lives
nowhere near the obligation, the guard's process layout, and would go
quietly false with no test failing if that layout changed; so witness the
construction and refuse at the adapter, one filter, and the property stays
true for a reason a reader can find from the obligation.

**Decided and built, 2026-09-20 evening.** niltempus chose the adapter refusal
with the construction witnessed. Built on the executor rather than in the
adapter's plan, because the plan runs at dispatch where the seat's modifier
state is not readable, and FakeInput has no reply for an error to ride on;
the refusal is silent on the wire, which is what the reference server would
do for a press it accepted, and the observer sees the chord's press never
arrive while every other press does. `PrivateExecutionRefusal::ReservedChord`
is raised in `run_ordered_input` when a synthetic Backspace press finds
Control and Mod1 in the seat's modifier mask; the constants are exported by
`sophia-x-authority` and the session's test holds them equal to the guard's
evdev keys and the engine's reservation.

Building it found a second thing. A request the executor refuses before
entering the authority had no completion at all: the grant's one cell stayed
held, and the producer's next request was saturated for good. One refused
injection wedged its injector, for `RepeatUnsupported` and
`StateOnlyUnsupported` as much as for the new refusal, and no control had
ever followed a refusal with a second request. The fix publishes the
refusal as the request's completion through the same reserved execution the
authority's own refusals use, with a new `RegistrationError::ConsumerRefused`,
and observes it at once: the cell is freed, the waiter is woken, and the
delivery is answered `RouteRejected` on the next retirement. Scoped to the
refusals the executor makes on the request's own terms (`Unmappable`,
`StateOnlyUnsupported`, `RepeatUnsupported`, `ReservedChord`), because the
M3 controls hold, on purpose, that a request refused for a fact about the
instance, foreign keyboard history, a replaced completion, an unprepared
origin, keeps no outcome. Witnessed by
`an_execution_refusal_answers_the_delivery_and_frees_the_grant_for_the_next_request`.

Bound to the executor witness, the wedge witness and the session's constants
test; the wire case `xtest_reserved_chord` joins the xtest profile, so it
reads forty-two of forty-two when it passes.

Three ways it could have closed, kept for the record: bind the obligation to a
witness of the construction (a synthetic chord through a private instance
reaches no observer, and a fixture-physical chord is recognised beside a
synthetic hold), which is honest only if the session-level guard is what the
obligation means; refuse the chord at the adapter so the wire itself says
synthetic cannot supply it, which the probe can bind; or build the
authority's physical-only recognition the comment anticipates, which is new
behaviour and the plan says M6 adds none. The lane's advice, carried here,
was that the adapter's refusal and the executor's invariant are different
witnesses; as built, the one refusal lives in the executor and the wire case
observes it, because a plan-time refusal cannot read the seat's modifier
state and FakeInput has no reply for an error.

## Finding and resolution

All six were bound by the end of the day, three of them on tests that did
not exist in the morning: the stalled reader, the five ingress refusals,
the output-lock wait, and the reserved chord with the wedge it uncovered.
`native_internal_wait`, which the lane had recorded unmet on a stale reason,
is bound too, and `native_executor_order` was reworded by decision to the
producers that exist. The runner has no partial verdict, so the honest count
of forty is the count of rows that hold whole, and it is forty once the
profile reads the last binding.

## Validation and remaining work

- [x] `native_lock_order` and `native_recipient_removal` bound; profile on
      `849a9b53` reads six NORESULT.
- [x] `native_stalled_reader`: written and bound at `4c6cdca2`.
- [x] `native_no_fallback`: the refusal half bound; the ambient half is the
      M4 group, named in the scope.
- [x] `native_executor_order`: unmet with the reason and the split option.
- [x] `native_ingress_admission`: the five missing witnesses written and
      the row bound to nineteen tests.
- [x] `native_internal_wait`: the output-lock witness written and the row
      bound.
- [x] t139: the executor refuses the synthetic press that would complete
      the chord; the wedge it uncovered is fixed; `native_protected_action`
      bound; `xtest_reserved_chord` added to the wire profile.
- [x] Decided 2026-09-20 with niltempus: the selected rank is the code's, and
      the plan's paragraph is corrected to it; `native_executor_order` is
      reworded to the producers that exist and bound to six tests; t139
      closes by refusing the chord at the adapter and witnessing the
      construction. The key-over-another-surface route refusal is the
      lane's row.
- [ ] The stale `allow(dead_code)` and "not attached yet" comments along
      the ordered writer chain: one sweep, after t138 lands.

## A note on the commit identities above

On the evening of 2026-09-20 the shared master was rebased by the other lane
before a push, which replayed this note's commits and the bindings' onto
`1a69ccab` under new identities and flattened the three merge commits.
Nothing changed in any tree, verified by diff. The identities cited here
are the ones that exist after that: `849a9b53` (lock order and recipient
removal), `4c6cdca2` (stalled reader), `2b4dee11` (no fallback, executor
order unmet, t139 opened), `fcfc98ee` (the marker sweep), `1bfe3659`
(ingress admission), `ae98d134` (internal wait), `7daa3bc7` and `563ed9c6`
(the t139 facts). Gate reports under `.artifacts/` name the commits they
ran on, some of which now exist only as trees.

## Connections

- [Binding the native obligations, and what the last six need](zrf53aky-binding-the-native-obligations-and-what-the-last-six-need.md) --
  the lane's twelve bindings, the two unmet, and the near miss that set the
  reading rule.
- [Private native input authority and XTEST adapter](../plans/7xqjn8rp-private-native-input-authority-and-xtest-adapter.md) --
  the obligations, the M6 contract, and the rank as the plan wrote it.
- [A departing source's held key is never released to its recipient](6xaim3pn-a-departing-sources-held-key-is-never-released-to-its-recipient.md) --
  t136, closed the same day; the xtest profile this composition also needs.
