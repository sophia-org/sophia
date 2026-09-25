---
id: z8jzl4oh
date: 2026-09-25
kind: investigation
status: investigating
tags: [investigation, x11, input, keyboard, private-native, t220]
---
# Private key direction: physical ownership and event delivery

The row: t220's private native key path selects both directions with the
press's masks. This note covers the design and red checkpoint the director
admitted on 2026-09-25. It traces the delivery architecture, states X11's rule
and Sophia's recipient contract, and compares the candidate designs. It adds
isolated red controls. It changes no production code, public API, ledger
semantics or wire. Neither recipient option below is approved. No
compatibility exception is claimed as success. The director reviews this
checkpoint before any production delta.

## What the private path does today

Accepted master `9ee301e7`:

- **Press.** `press_key` finds the focus client, or the active activation's
  owner. `prepare_keyboard_press` picks the recipient client (the focus, or
  a grab owner). `commit_key` then resolves one plan with `resolve_key_plan`
  (the window and the core/XI forms). The plan uses the press's bits:
  KeyPressMask, `XI_KeyPress` and KeyPress do-not-propagate. Grab masks use
  bit 0. The plan is resolved before anything is installed. On
  `NotSelected`, `commit_key` returns before the `KeyHold` is stored,
  before the recovery ticket is bound, before `permit.press`, before
  `map_evdev_key` and before `observe_pressed_key`. The executor
  (`private_execution_key.rs`) turns that into `StaleExecution` and
  drops the custody.
- **Join.** `join_key` inherits the retained recipient. It reads no focus,
  selection or grab and leaves XKB alone.
- **Release with a hold.** `release_key` calls `permit.release`. On
  `DeliverTo`, `finish_key_release` releases XKB, clears QueryKeymap,
  updates modifiers, retires the activation and computes residuals (query
  scope, mapper, selection, `ExternalLease`, synchronous thaw, activation).
  It then always builds the release emission from `hold.plan`. It keeps the
  press's window and forms and refreshes only the child, the coordinates and
  the XKB detail selection. A `StateOnly` route
  (`RecipientTerminationRequired`) settles in full and builds nothing. It is
  the existing precedent for a release that settles and owes no event
  (`PrivateDeliveryCustody::owes_event` is false without a completion).
- **Release without a hold.** The ledger answers `NotHeld`, and the release
  is recorded as owing nothing.

The press's plan therefore decides both directions. Physical ownership exists
only when a window selected the press.

The ordinary path (`routing/keyboard.rs`, `connection/writers/input.rs`)
works differently:
- XKB and QueryKeymap move on every transition, before any selection is read.
- The release is routed to the current focus or grab owner.
- The writer picks the direction's mask at write time (the t220 ordinary
  slice).

This path follows X more closely, but it has no retained recipient.

## Gaps

- **R1.** The release is written to the press's recipient even when that
  client did not select KeyRelease. It never reaches a KeyRelease selector
  that is not the press's window, such as an ancestor that selected only
  KeyRelease.
- **R2.** KeyRelease do-not-propagate (bit 1) is never consulted. KeyPress
  do-not-propagate also stops the release.
- **R3.** An active or passive grab's mask is read for KeyPress only. With
  owner_events, the owner's own selection is not tried for the release.
- **R4.** A KeyRelease-only selector (core bit 1 or `XI_KeyRelease`) never
  gets its release: the press refuses, so no hold exists.
- **Physical coupling.** A key nobody selected never enters XKB or
  QueryKeymap. An unselected Shift leaves the next selected key without its
  modifier, and QueryKeymap misses keys that are down. This follows from R4's
  cause but is a separate defect. Any task proposal goes to the director; this
  note adds no ledger row.
- **Observation, outside this slice.** The XKB StateNotify is a frame of the
  key's own emission. An unselected key therefore also withholds StateNotify
  from XKB state selectors. This note does not change that.

## X11's rule

The reference server (Xorg dix) processes a key edge in two separate steps.

1. **Device state.** Down bits, modifiers and XKB always change. A release of
   a key that is not down is discarded.
2. **Delivery.** The event is delivered at the time it happens:
   - An active grab takes it at the grab window, using the grab's mask for
     that direction. With owner_events, the grabbing client's normal delivery
     is tried first.
   - Otherwise it goes to focus. When the pointer is inside the focus, it
     starts from the pointer's descendant of the focus window. It propagates
     to the first window any client selected the direction on, and that
     direction's do-not-propagate stops it.
   - XI2 selections are per event type (`XI_KeyPress` 2, `XI_KeyRelease` 3).
   - A passive grab activates on the press. The trigger's release is
     delivered under that grab, which then ends.

Selection, focus, pointer and grab changes between press and release all
affect where the release goes. X has no retained recipient.

## Sophia's contract

The design must preserve the following:

- **Ledger** (`sophia-input-authority/src/ledger.rs`,
  `registry/execution.rs`):
  - `DeliverTo` is owed to the recipient the first press was delivered to.
  - The incarnation carries that recipient and its connection generation.
  - The barrier refuses a new press for the same recipient and generation
    while an old hold's native or recipient obligation is open.
  - Native reconciliation and recipient transport settle separately.
- **Plan 7xqjn8rp:**
  - The last release targets the recorded recipient.
  - A focus change cannot turn cleanup into a new action.
  - Joins inherit the recipient.
  - No late reselection or restamp.
  - Release keeps its target and forms, and refreshes only coordinates and
    the child.
  - Completion identity includes the generation.
- **InputAuthorityArbitration:** routes use the presented choice, leases are
  profile-scoped, and security transitions revoke all delivery without an
  acknowledgement.
- **InputDeliveryRecovery:** NoAcceptedObligationLost, ExactlyOneTerminal
  and NoFalseFlush. Receipts match the exact connection and delivery.

## Candidate designs

### (a) Always bind the prepared client; resolve the release window late

The press always binds the prepared recipient client (the focus or grab
owner) in the ledger. That happens even when no window selected it. The
release then resolves its window and forms from that client's selections at
release time.

- **Kept:** the first recipient, the barrier and the split settlement.
- **Broken:** 7xqjn8rp's "no late reselection" and "release keeps its target
  and forms". The release's window is chosen after the fact, from current
  focus, pointer and grab state, but only within the old client.
- **Against X:** it matches selection changes within the same client. It
  still diverges when focus moves to another client. It is a partial X
  behaviour that reads current policy without the authority X gets from it.

### (b) Recipient-less physical hold; resolve the recipient at release

Physical ownership is recorded with no recipient. The release picks its
client, window and forms at release time.

- **Against X:** matches it fully.
- **Broken:**
  - `DeliverTo`'s first-recipient rule.
  - The recipient and generation in `HoldIncarnation`, the barrier's key
    and the completion identity.
  - Recovery's bind-before-effect.
  - 7xqjn8rp's rule that cleanup cannot become a new action.
- **Security:** a focus change mid-hold hands a release to a client that
  never saw the press. That leaks key timing across clients and removes the
  stuck-key protection the retained recipient provides.
- **Verdict:** not proposed.

### (c) Two direction plans at the press (proposed for review)

At the press's single guarded boundary, from the same focus, pointer,
selection and grab snapshot:

1. Resolve the recipient client exactly as today.
2. Resolve **two** optional plans with the shared rule
   (`crate::key_routing::x_key_delivery_target`):
   - the press plan: KeyPress bit 0, `XI_KeyPress`, KeyPress DNP;
   - the release plan: KeyRelease bit 1, `XI_KeyRelease`, KeyRelease DNP.
   A grab uses its mask's bit for each direction, and owner_events is tried
   per direction.
3. Install the hold whenever the key goes down physically for a known
   recipient client, including when both plans are empty. Bind recovery,
   `permit.press`, XKB and QueryKeymap as today.
4. Emit the press only when the press plan exists.
5. At release, settle everything physical and native as today. Build the
   release only from the retained release plan, refreshing the child and
   coordinates as today. When the release plan is empty, the custody owes no
   event: the `StateOnly` precedent.

What (c) keeps:
- the first recipient and generation;
- the barrier;
- the settlement split;
- bind-before-effect;
- no late reselection: both plans are decided at the press, under its guards;
- joins inherit.

The axis path is a precedent: `private_native_transient.rs` resolves both
emulated halves together, and an unselected half does not erase the other.
(c) needs no new wire and no public API.

What (c) still requires the director to decide:

- **Recipient settlement for a press or release that owes no event.** The
  ledger records `recipient_settled` through the attempt machinery. A hold
  whose release plan is empty needs its recipient obligation to settle as
  "nothing owed", without a flush or termination receipt. `StateOnly`
  currently settles through recipient termination instead. This is a
  settlement-semantics change, and it needs your approval.
- **The recovery ticket of a press that emits nothing.** The ticket is bound
  to the recipient before the effect, so it must still reach exactly one
  terminal. The pointer path's `reaches == false` handling is the existing
  shape.
- **No recipient client at all.** With no focus client and no activation,
  (c) cannot install a hold, and the key stays physically unowned. X would
  still update device state. This needs a separate decision.

## Conflicts that (c) leaves with X

These remain divergences. They are not presented as success:

- **C1. Selection change between press and release.** X honours the
  selection at release time; (c) uses the plan from the press. Controls:
  `conflict_t220_key_release_deselected_after_the_press_is_not_written` and
  `conflict_t220_key_release_selected_after_the_press_is_written`.
- **C2. Pointer or focus change between press and release.** X resolves from
  current pointer and focus; (c) keeps the press's window and recipient.
  Control: `conflict_t220_key_release_follows_the_pointer_at_release_time`.
  A focus move to another client is the same conflict with a security cost
  under X semantics (see (b)).
- **C3. A grab taken or ended between press and release.** X delivers to the
  grab now in force; (c) keeps the plan.
- **C4. Recipient departure or generation replacement.** X delivers the
  release to whoever holds focus. (c) settles the departed recipient through
  termination and never names the replacement. Controls:
  `t220_key_departed_press_only_recipient_still_settles_physically` and
  `t220_key_recipient_replacement_is_refused_while_the_release_is_owed`
  (guard).
  The guard shows how replacement is contained today. After the recipient
  departs and its admission is revoked, re-admitting the same client is
  refused with `AlreadyAdmitted` while the key's release is owed, because the
  lifecycle record stays owned. No replacement generation can be named during
  the debt.

One more observation, which this checkpoint does not verify. The shared rule
(`x_key_delivery_target`) deliberately offers the event to the focus window
after a do-not-propagate stop, and the private controls pin that. The
reference server appears to stop at the do-not-propagate window without that
retry. The per-direction DNP controls here nest a leaf under a selecting
window, so they do not depend on that question. The question goes to the
director as a separate item.

Existing guards that encode today's coupling and would change under (c):
- `native_key_unpublished_focus_and_missing_selection_refuse_before_xkb`
  asserts that an unselected press leaves XKB released.
- `native_key_normal_routing_stops_at_focus_and_honors_pointer_descendants`
  asserts, in cases 1, 2 and 4, a `NotSelected` refusal with the key
  released.

These existing guards match (c) and conflict with X (C1):
- `native_key_emission_survives_selection_changes_and_release_uses_current_geometry`
- `native_key_xi_selection_uses_exact_coordinates_and_does_not_emit_core`

Both deselect after the press and still expect the release.

## Red controls

`crates/sophia-x-authority/src/x11_socket/tests/private_native_key_direction.rs`,
included in the private keyboard test module. Each control asserts the
physical outcome and the protocol outcome separately:

| Control | Covers | On `9ee301e7` | Under (c) |
| --- | --- | --- | --- |
| `t220_key_press_only_selection_settles_the_release_without_an_event` | R1, settlement | red: release built | green |
| `t220_key_release_only_selection_owns_the_press_and_receives_the_release` | R4, physical ownership | red: press `NotSelected` | green |
| `t220_key_unselected_modifier_still_reaches_xkb_and_query_keymap` | physical coupling, QueryKeymap, final release | red: press `NotSelected` | green |
| `t220_key_unselected_join_survivor_and_final_release` | multi-source join, survivor, final release | red: press `NotSelected` | green |
| `t220_key_press_and_release_propagate_to_their_own_selectors` | R1 propagation | red: release at the child | green |
| `t220_key_release_do_not_propagate_stops_only_the_release` | R2 | red: release built | green |
| `t220_key_press_do_not_propagate_leaves_the_release_selected` | R2 | red: press `NotSelected` | green |
| `t220_key_xi_release_only_selection_receives_only_the_xi_release` | R4, XI2 | red: press `NotSelected` | green |
| `t220_key_grab_press_only_mask_settles_the_release_without_an_event` | R3 | red: release built | green |
| `t220_key_owner_events_grab_resolves_each_direction_separately` | R3, owner_events | red: release at the grab window (root) | green |
| `t220_key_passive_press_only_grab_retires_without_a_release_event` | R3, activation retirement | red: release built (retirement asserts pass first) | green |
| `t220_key_route_lease_stays_owed_when_the_release_writes_nothing` | lease settlement | red: release built (`ExternalLease` retained first) | green |
| `t220_key_departed_press_only_recipient_still_settles_physically` | disconnect | red: release built for the departed owner | green |
| `t220_key_recipient_replacement_is_refused_while_the_release_is_owed` | generation replacement | green (guard) | green |
| `conflict_t220_key_release_deselected_after_the_press_is_not_written` | C1 | red: release built | red (X-only) |
| `conflict_t220_key_release_selected_after_the_press_is_written` | C1 | green: the press plan is inherited | red (X-only) |
| `conflict_t220_key_release_follows_the_pointer_at_release_time` | C2 | red: release at the child | red (X-only) |

The "Under (c)" column is the design's expectation, not a result: no
production change exists.

## Red evidence

Run on `input/t220-key-direction` (parent `9ee301e7`), with only the controls
added.

- **Isolation:** device-hidden bwrap, nice 19, jobs 2 and an on-disk target.
- **Command:** `cargo test -p sophia-x-authority --lib -- t220_key`.
- **Result:** 2 passed and 15 failed, all at their intended assertion.
- **Logs:** `.artifacts/t220-key-direction/red-run-4.log`, with each
  failure's reason in `red-reasons.txt`.
- **Earlier runs:** runs 1–3 are kept for history. Each fixed one fixture
  error:
  - a grab-mask type;
  - DNP controls that first collided with the retry at focus;
  - a replacement control that re-admitted while the release was owed.

What each failure asserts:
- The ten release-side failures (seven "release built", three at the wrong
  window) reach their protocol assertion only after the physical assertions
  pass: `DeliverTo`, XKB released, QueryKeymap
  clear and `NativeReconciled`. For the lease and passive controls,
  `ExternalLease` and the activation retirement also pass first. Today's gap
  is only the event write.
- The five `NotSelected` failures are the physical-ownership prerequisite: no
  hold, XKB or QueryKeymap for a press nobody selected.
